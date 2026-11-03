use super::*;
use codex_http_client::OutboundProxyPolicy;
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_json, header, method, path, query_param},
};
fn client(server: &MockServer) -> AgentsClient {
    let mut client = AgentsClient::new(
        "synthetic-api-fixture",
        HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
    )
    .expect("client");
    client.0.base = url::Url::parse(&(server.uri() + "/v1/")).expect("test URL");
    client
}
fn create() -> Create {
    serde_json::from_value(json!({"agent":{"model":"explicit-fixture-model"},"environment":{"type":"self_hosted","workspace_directory":"/workspace","capability_directories":["/capabilities"]},"input":"selected task"})).expect("input")
}
#[test]
fn selected_payload_only_and_bounded() {
    assert!(
        !String::from_utf8(create().encode().expect("encoded"))
            .expect("UTF8")
            .contains("memory")
    );
    assert!(serde_json::from_value::<Create>(json!({"agent":{"model":"x"},"environment":{"type":"none"},"input":"x","parent_thread":"secret"})).is_err());
    let mut value = create();
    value.input = "x".repeat(32769);
    assert!(value.encode().is_err());
    value = create();
    value.input = "\u{0001}".repeat(32768);
    assert!(value.encode().is_err());
    value = create();
    value.agent.model.clear();
    assert!(value.encode().is_err());
    assert!(
        AgentsClient::new(
            "bad\nkey",
            HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault)
        )
        .is_err()
    );
}
#[tokio::test]
async fn create_uses_only_api_key_and_beta_protocol() {
    let server = MockServer::start().await;
    Mock::given(method("POST")).and(path("/v1/agents/sessions")).and(header("authorization","Bearer synthetic-api-fixture")).and(header("openai-beta","agents=v1")).and(body_json(serde_json::to_value(create()).expect("wire")))
    .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":"session_fixture","status":"requires_action","environment":{"type":"self_hosted","id":"env_fixture","remote_url":"https://api.openai.com/v1/agents/environments"},"required_actions":[{"type":"environment_connection","private_extra":"ignored"}],"agent":{"instructions":"private instruction"}}))).expect(1).mount(&server).await;
    let result = client(&server).create(&create()).await.expect("created");
    let json = serde_json::to_string(&result).expect("JSON");
    assert!(!json.contains("private"));
    assert_eq!(result.status, "requires_action");
}
#[tokio::test]
async fn mutation_failure_is_not_blindly_retried() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503).set_body_string("private server diagnostic"))
        .expect(1)
        .mount(&server)
        .await;
    assert!(matches!(
        client(&server).create(&create()).await,
        Err(CloudError::SubmissionOutcomeUnknown)
    ));
}
#[tokio::test]
async fn api_unauthorized_never_uses_chatgpt_refresh() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;
    assert!(matches!(
        client(&server).status("session_fixture").await,
        Err(CloudError::Http(401))
    ));
}
#[tokio::test]
async fn cancel_is_an_event_and_does_not_delete_the_session() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/agents/sessions/session_fixture/events"))
        .and(body_json(
            json!({"events":[{"type":"agent.session.input.cancel"}]}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;
    client(&server)
        .cancel("session_fixture")
        .await
        .expect("accepted");
}
#[tokio::test]
async fn assistant_only_page_with_explicit_cursor() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/v1/agents/sessions/session_fixture/items")).and(query_param("order","asc")).and(query_param("limit","100")).and(query_param("after","previous_item"))
    .respond_with(ResponseTemplate::new(200).set_body_json(json!({"has_more":true,"last_id":"item_last","data":[
        {"type":"message","role":"user","turn_id":"turn_fixture","status":"completed","content":[{"type":"input_text","text":"private prompt"}]},
        {"type":"function_call","arguments":{"private":"argument"},"content":{"private":"tool body"}},
        {"type":"message","role":"assistant","id":"item_fixture","turn_id":"turn_fixture","status":"completed","phase":"final_answer","content":[{"type":"output_text","text":"checked result"}]}]}))).expect(1).mount(&server).await;
    let result = client(&server)
        .result("session_fixture", Some("previous_item"))
        .await
        .expect("page");
    assert_eq!(result.next_after.as_deref(), Some("item_last"));
    assert_eq!(result.messages.len(), 1);
    assert!(
        !serde_json::to_string(&result)
            .expect("JSON")
            .contains("private")
    );
}
#[test]
fn oversized_or_inconsistent_results_fail_explicitly() {
    assert!(ResultPage::parse(br#"{"has_more":true,"last_id":null,"data":[]}"#).is_err());
    let value = json!({"has_more":false,"data":[{"type":"message","role":"assistant","turn_id":"turn_fixture","status":"completed","content":[{"type":"output_text","text":"x".repeat(MAX_RESULT_TEXT_BYTES+1)}]}]});
    assert!(matches!(
        ResultPage::parse(&serde_json::to_vec(&value).expect("JSON")),
        Err(CloudError::ResponseTooLarge(_))
    ));
    assert!(
        Session::parse(
            br#"{"id":"other_session","status":"idle"}"#,
            Some("session_fixture")
        )
        .is_err()
    );
    assert!(Session::parse(br#"{"id":"session_fixture","status":"idle","environment":{"type":"self_hosted","remote_url":"https://attacker.test/"}}"#,None).is_err());
    assert_eq!(
        Session::parse(br#"{"id":"session_fixture","status":"idle"}"#, None)
            .expect("idle")
            .status,
        "idle"
    );
}
