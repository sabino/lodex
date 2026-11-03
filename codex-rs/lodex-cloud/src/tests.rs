use super::*;
use codex_api::{AuthError, AuthHeadersFuture, AuthProvider, AuthRefreshFuture};
use codex_http_client::OutboundProxyPolicy;
use http::{HeaderMap, HeaderValue};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_json, header, method, path},
};

struct FixtureAuth {
    refreshes: AtomicUsize,
}
impl AuthProvider for FixtureAuth {
    fn add_auth_headers(&self, headers: &mut HeaderMap) -> Result<(), AuthError> {
        headers.insert(
            http::header::AUTHORIZATION,
            HeaderValue::from_static(if self.refreshes.load(Ordering::SeqCst) == 0 {
                "Bearer synthetic-fixture"
            } else {
                "Bearer synthetic-fixture-refreshed"
            }),
        );
        Ok(())
    }
    fn apply_auth_headers(&self, mut headers: HeaderMap) -> AuthHeadersFuture<'_> {
        Box::pin(async move {
            self.add_auth_headers(&mut headers)?;
            Ok(headers)
        })
    }
    fn refresh_after_unauthorized(&self, _headers: HeaderMap) -> AuthRefreshFuture<'_> {
        Box::pin(async move {
            self.refreshes.fetch_add(1, Ordering::SeqCst);
            true
        })
    }
}
fn client(server: &MockServer) -> (CloudClient, Arc<FixtureAuth>) {
    let auth = Arc::new(FixtureAuth {
        refreshes: AtomicUsize::new(0),
    });
    let client = CloudClient::with_base(
        Url::parse(&(server.uri() + "/")).expect("fixture URL"),
        HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
        auth.clone(),
    );
    (client, auth)
}

#[test]
fn trusted_origins_reject_credential_forwarding_targets() {
    for url in [
        "http://chatgpt.com",
        "https://chatgpt.com.evil.test",
        "https://user:secret@chatgpt.com",
        "https://chatgpt.com:444",
        "https://chatgpt.com/backend-api?token=x",
        "https://chatgpt.com/#fragment",
        "https://chatgpt.com/other",
    ] {
        assert!(validate_backend(url).is_err(), "{url}");
    }
    assert_eq!(
        validate_backend("https://chatgpt.com")
            .expect("trusted")
            .as_str(),
        "https://chatgpt.com/backend-api/"
    );
}

#[tokio::test]
async fn environment_projection_ignores_unselected_data() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wham/environments/by-repo/github/example/router"))
        .and(header("authorization", "Bearer synthetic-fixture"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!([{"id":"env_selected","label":"router", "private_setup":"not-returned"}]),
        ))
        .expect(1)
        .mount(&server)
        .await;
    let rows = client(&server)
        .0
        .environments(Some("example/router"))
        .await
        .expect("rows");
    let output = serde_json::to_string(&rows).expect("JSON");
    assert_eq!(rows.len(), 1);
    assert!(!output.contains("private_setup"));
}

#[tokio::test]
async fn explicit_submission_has_no_ambient_context_and_requires_ref() {
    let server = MockServer::start().await;
    let task = CloudTask::new(
        "env_selected".into(),
        Some("selected-branch".into()),
        "synthetic task".into(),
    )
    .expect("valid");
    let expected: serde_json::Value =
        serde_json::from_slice(&encode_cloud_submission(&task).expect("wire")).expect("JSON");
    Mock::given(method("POST"))
        .and(path("/wham/tasks"))
        .and(body_json(expected))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"task":{"id":"task_fixture"}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let client = client(&server).0;
    assert_eq!(client.submit(&task).await.expect("created"), "task_fixture");
    let invalid = CloudTask::new("env_selected".into(), None, "synthetic".into()).expect("input");
    assert_eq!(
        client.submit(&invalid).await.err(),
        Some(CloudError::InvalidSubmission)
    );
}

#[tokio::test]
async fn status_projection_drops_parent_prompt_and_diffs() {
    let server = MockServer::start().await;
    Mock::given(path("/wham/tasks/task_fixture")).respond_with(ResponseTemplate::new(200).set_body_json(json!({
        "task":{"id":"task_fixture","title":"private title"},
        "current_assistant_turn":{"id":"turn_fixture","turn_status":"completed","input_items":[{"text":"private parent prompt"}],"output_items":[{"diff":"private diff"}]}
    }))).mount(&server).await;
    let status = client(&server)
        .0
        .status("task_fixture")
        .await
        .expect("status");
    assert_eq!(status.phase, Phase::Completed);
    let output = serde_json::to_string(&status).expect("JSON");
    assert!(!output.contains("private"));
}

#[tokio::test]
async fn results_only_include_assistant_text() {
    let server = MockServer::start().await;
    Mock::given(path("/wham/tasks/task_fixture")).respond_with(ResponseTemplate::new(200).set_body_json(json!({
        "task":{"id":"task_fixture"},"current_assistant_turn":{"id":"turn_fixture","turn_status":"completed",
            "input_items":[{"content":"private input"}],"output_items":[
                {"type":"message","role":"assistant","content":[{"content_type":"text","text":"checked result"}]},
                {"type":"message","role":"user","content":["private user output"]},
                {"type":"output_diff","diff":"private diff"}],
            "worklog":{"messages":[{"author":{"role":"assistant"},"content":{"parts":["checked worklog"]}},
                {"author":{"role":"user"},"content":{"parts":["private user log"]}}]}}
    }))).mount(&server).await;
    let result = client(&server)
        .0
        .result("task_fixture")
        .await
        .expect("result");
    assert_eq!(result.messages, ["checked result", "checked worklog"]);
    assert!(
        !serde_json::to_string(&result)
            .expect("JSON")
            .contains("private")
    );
}

#[tokio::test]
async fn body_and_result_text_limits_are_explicit_errors() {
    let server = MockServer::start().await;
    Mock::given(path("/wham/environments"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'x'; MAX_METADATA_BYTES + 1]))
        .mount(&server)
        .await;
    assert_eq!(
        client(&server).0.environments(None).await.err(),
        Some(CloudError::ResponseTooLarge(MAX_METADATA_BYTES))
    );
    Mock::given(path("/wham/tasks/task_fixture")).respond_with(ResponseTemplate::new(200).set_body_json(json!({
        "task":{"id":"task_fixture"},"current_assistant_turn":{"id":"turn_fixture","turn_status":"completed","output_items":[{"type":"message","content":["x".repeat(MAX_RESULT_TEXT_BYTES+1)]}]}
    }))).mount(&server).await;
    assert_eq!(
        client(&server).0.result("task_fixture").await.err(),
        Some(CloudError::ResponseTooLarge(MAX_RESULT_TEXT_BYTES))
    );
}

#[tokio::test]
async fn auth_refresh_is_bounded_and_diagnostics_omit_server_body() {
    let server = MockServer::start().await;
    Mock::given(path("/wham/environments"))
        .respond_with(ResponseTemplate::new(401).set_body_string("private diagnostic"))
        .expect(2)
        .mount(&server)
        .await;
    let (client, auth) = client(&server);
    let error = client.environments(None).await.err().expect("error");
    assert_eq!(error, CloudError::Http(401));
    assert_eq!(auth.refreshes.load(Ordering::SeqCst), 1);
    assert!(!format!("{error:?} {error}").contains("private"));
}

#[tokio::test]
async fn redirects_do_not_receive_credentials_and_cancel_sends_nothing() {
    let first = MockServer::start().await;
    let second = MockServer::start().await;
    Mock::given(path("/wham/environments"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", second.uri()))
        .expect(1)
        .mount(&first)
        .await;
    let client = client(&first).0;
    assert_eq!(
        client.environments(None).await.err(),
        Some(CloudError::Http(302))
    );
    assert!(
        second
            .received_requests()
            .await
            .expect("requests")
            .is_empty()
    );
    assert_eq!(
        client.cancel("task_fixture"),
        Err(CloudError::CancellationUnavailable)
    );
    assert_eq!(first.received_requests().await.expect("requests").len(), 1);
}

#[tokio::test]
async fn successful_but_unparseable_submission_is_not_retried() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not a task id"))
        .expect(1)
        .mount(&server)
        .await;
    let task =
        CloudTask::new("env".into(), Some("ref".into()), "synthetic task".into()).expect("input");
    assert_eq!(
        client(&server).0.submit(&task).await.err(),
        Some(CloudError::SubmissionOutcomeUnknown)
    );
}

#[tokio::test]
async fn legacy_nested_status_and_null_arrays_are_supported() {
    let server = MockServer::start().await;
    Mock::given(path("/wham/tasks/task_fixture")).respond_with(ResponseTemplate::new(200).set_body_json(json!({
        "task":{"id":"task_fixture","task_status_display":{"latest_turn_status_display":{"turn_status":"cancelled"}}},
        "current_assistant_turn":null
    }))).mount(&server).await;
    let client = client(&server).0;
    assert_eq!(
        client.status("task_fixture").await.expect("status").phase,
        Phase::Cancelled
    );
    assert_eq!(
        client
            .result("task_fixture")
            .await
            .expect("result")
            .status
            .phase,
        Phase::Cancelled
    );
    server.reset().await;
    Mock::given(path("/wham/tasks/task_fixture")).respond_with(ResponseTemplate::new(200).set_body_json(json!({
        "task":{"id":"task_fixture"},"current_assistant_turn":{"id":"turn_fixture","turn_status":"pending","output_items":null,"worklog":{"messages":null}}
    }))).mount(&server).await;
    let result = client.result("task_fixture").await.expect("null arrays");
    assert_eq!(result.status.phase, Phase::Pending);
    assert!(result.messages.is_empty());
}

#[tokio::test]
async fn server_failure_after_submission_has_unknown_outcome_and_no_retry() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503).set_body_string("private backend diagnostic"))
        .expect(1)
        .mount(&server)
        .await;
    let task = CloudTask::new(
        "env".into(),
        Some("explicit-ref".into()),
        "synthetic task".into(),
    )
    .expect("input");
    assert_eq!(
        client(&server).0.submit(&task).await.err(),
        Some(CloudError::SubmissionOutcomeUnknown)
    );
    assert_eq!(
        client(&server).0.status("..").await.err(),
        Some(CloudError::InvalidIdentifier)
    );
}

#[tokio::test]
async fn explicit_unauthorized_refresh_retries_with_new_headers() {
    let server = MockServer::start().await;
    Mock::given(path("/wham/environments"))
        .and(header("authorization", "Bearer synthetic-fixture"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/wham/environments"))
        .and(header(
            "authorization",
            "Bearer synthetic-fixture-refreshed",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&server)
        .await;
    let (client, auth) = client(&server);
    assert!(client.environments(None).await.expect("retry").is_empty());
    assert_eq!(auth.refreshes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn pinned_upstream_result_fixture_matches_its_assistant_text() {
    let server = MockServer::start().await;
    Mock::given(path("/wham/tasks/task_123"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            include_str!("../../backend-client/tests/fixtures/task_details_with_diff.json"),
            "application/json",
        ))
        .mount(&server)
        .await;
    let result = client(&server)
        .0
        .result("task_123")
        .await
        .expect("upstream fixture");
    assert_eq!(result.messages, ["Assistant response"]);
    let output = serde_json::to_string(&result).expect("JSON");
    assert!(!output.contains("First line"));
    assert!(!output.contains("diff --git"));
}

#[tokio::test]
async fn composite_turn_identifiers_are_bounded_output_not_url_paths() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/wham/tasks/task_fixture"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "task":{"id":"task_fixture","task_status_display":{"latest_turn_status_display":{"turn_id":"task_fixture:turn_fixture","turn_status":"completed"}}},
            "current_assistant_turn":{"id":"task_fixture:turn_fixture","turn_status":"completed","output_items":[{"type":"message","role":"assistant","content":[{"content_type":"text","text":"verified"}]}]}
        }))).mount(&server).await;
    let client = client(&server).0;
    assert_eq!(
        client
            .status("task_fixture")
            .await
            .expect("status")
            .turn_id
            .as_deref(),
        Some("task_fixture:turn_fixture")
    );
    assert_eq!(
        client
            .result("task_fixture")
            .await
            .expect("result")
            .messages,
        vec!["verified"]
    );
    assert!(!identifier("task_fixture:turn_fixture"));
    assert!(!turn_identifier("turn\nunsafe"));
    assert!(!turn_identifier(&"x".repeat(257)));
    let fallback:StatusDetails=serde_json::from_value(json!({"task":{"id":"task_fixture"},"task_status_display":{"latest_turn_status_display":{"turn_id":"task_fixture:turn_fixture","turn_status":"completed"}}})).expect("fallback");
    assert_eq!(
        fallback
            .status("task_fixture")
            .expect("fallback status")
            .turn_id
            .as_deref(),
        Some("task_fixture:turn_fixture")
    );
}
