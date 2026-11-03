use crate::{CloudError, REQUEST_TIMEOUT};
use codex_http_client::{
    ClientRouteClass, HttpClientFactory, HttpTransport, Request, ReqwestTransport,
    RouteAwareClientPool, TransportError,
};
use http::{HeaderValue, Method, StatusCode};
use url::Url;

pub(crate) enum Credentials {
    Api(HeaderValue),
    #[cfg(feature = "chatgpt")]
    ChatGpt(codex_api::SharedAuthProvider),
}
pub(crate) struct ControllerHttp {
    pub(crate) base: Url,
    transport: ReqwestTransport,
    auth: Credentials,
}
impl ControllerHttp {
    pub(crate) fn new(base: Url, factory: HttpClientFactory, auth: Credentials) -> Self {
        Self {
            base,
            transport: ReqwestTransport::from_route_aware_client_pool(
                RouteAwareClientPool::new_without_redirects_or_request_logging(
                    factory,
                    ClientRouteClass::Api,
                ),
            ),
            auth,
        }
    }
    pub(crate) fn url(&self, segments: &[&str]) -> Result<String, CloudError> {
        let mut url = self.base.clone();
        url.path_segments_mut()
            .map_err(|_| CloudError::UntrustedOrigin)?
            .pop_if_empty()
            .extend(segments);
        Ok(url.into())
    }
    pub(crate) async fn request(
        &self,
        method: Method,
        url: String,
        body: Option<Vec<u8>>,
        limit: usize,
    ) -> Result<bytes::Bytes, CloudError> {
        let body = body.map(bytes::Bytes::from);
        let operation = async {
            for attempt in 0..2 {
                let mut request = Request::new(method.clone(), url.clone());
                request.timeout = Some(REQUEST_TIMEOUT);
                request.response_body_limit_bytes = Some(limit);
                request.headers.insert(
                    http::header::ACCEPT,
                    HeaderValue::from_static("application/json"),
                );
                request.headers.insert(
                    http::header::USER_AGENT,
                    HeaderValue::from_static("lodex-cloud/0.166.0"),
                );
                if let Some(body) = body.as_ref() {
                    request = request.with_raw_body(body.clone());
                    request.headers.insert(
                        http::header::CONTENT_TYPE,
                        HeaderValue::from_static("application/json"),
                    );
                }
                let request = match &self.auth {
                    Credentials::Api(key) => {
                        request
                            .headers
                            .insert(http::header::AUTHORIZATION, key.clone());
                        request
                            .headers
                            .insert("openai-beta", HeaderValue::from_static("agents=v1"));
                        request
                    }
                    #[cfg(feature = "chatgpt")]
                    Credentials::ChatGpt(provider) => provider
                        .apply_auth(request)
                        .await
                        .map_err(|_| CloudError::Authentication)?,
                };
                #[cfg(feature = "chatgpt")]
                let rejected = request.headers.clone();
                match self.transport.execute(request).await {
                    Ok(response) => return Ok(response.body),
                    Err(TransportError::Http {
                        status: StatusCode::UNAUTHORIZED,
                        ..
                    }) if attempt == 0 => {
                        let refreshed = match &self.auth {
                            Credentials::Api(_) => false,
                            #[cfg(feature = "chatgpt")]
                            Credentials::ChatGpt(provider) => {
                                provider.refresh_after_unauthorized(rejected).await
                            }
                        };
                        if !refreshed {
                            return Err(CloudError::Http(401));
                        }
                    }
                    Err(TransportError::Http { status, .. }) => {
                        return Err(CloudError::Http(status.as_u16()));
                    }
                    Err(TransportError::ResponseTooLarge { max_bytes }) => {
                        return Err(CloudError::ResponseTooLarge(max_bytes));
                    }
                    Err(_) => return Err(CloudError::Transport),
                }
            }
            Err(CloudError::Http(401))
        };
        tokio::time::timeout(REQUEST_TIMEOUT, operation)
            .await
            .map_err(|_| CloudError::Timeout)?
    }
}
