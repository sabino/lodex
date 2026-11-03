#![cfg(feature = "api-cli")]
use std::io::Write;
use std::process::{Command, Output, Stdio};
fn run(args: &[&str], input: &[u8], key: bool, home: &std::path::Path) -> std::io::Result<Output> {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_lodex-agents"));
    cmd.args(args)
        .env_remove("OPENAI_API_KEY")
        .env("CODEX_HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if key {
        cmd.env("OPENAI_API_KEY", "synthetic-api-cli-fixture");
    }
    let mut child = cmd.spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        let written = stdin.write_all(input);
        if let Err(error) = written
            && error.kind() != std::io::ErrorKind::BrokenPipe
        {
            return Err(error);
        }
    }
    child.wait_with_output()
}
#[test]
fn standalone_ignores_codex_home_and_does_not_export_profile_content() {
    let home = tempfile::tempdir().expect("isolated home");
    let canary = "private-profile-cli-canary";
    std::fs::write(
        home.path().join("config.toml"),
        "[ invalid codex configuration",
    )
    .expect("fixture config");
    std::fs::write(home.path().join("SOUL.md"), canary).expect("soul");
    std::fs::write(home.path().join("MEMORY.md"), canary).expect("memory");
    let checked = run(&["check"], b"", true, home.path()).expect("check exit");
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&checked.stdout).expect("JSON");
    assert_eq!(value["configuration"], "http_environment");
    assert_eq!(value["credentials_verified"], false);
    assert_eq!(value["agents_api_request_sent"], false);
    assert!(!String::from_utf8_lossy(&checked.stdout).contains(canary));
    assert_eq!(
        std::fs::read_to_string(home.path().join("SOUL.md")).expect("unchanged soul"),
        canary
    );
    #[cfg(feature = "cli")]
    {
        let full = Command::new(env!("CARGO_BIN_EXE_lodex-cloud"))
            .args(["api", "check"])
            .env("CODEX_HOME", home.path())
            .env("OPENAI_API_KEY", "synthetic-api-cli-fixture")
            .output()
            .expect("full frontend");
        assert!(
            !full.status.success(),
            "full frontend must keep its Codex config validation"
        );
        assert!(!String::from_utf8_lossy(&full.stderr).contains(canary));
    }
}
#[test]
fn no_key_and_invalid_payloads_fail_before_any_request() {
    let home = tempfile::tempdir().expect("isolated home");
    let cases:[(&[&str],Vec<u8>,&str);4]=[
        (&["check"],vec![],"separate OPENAI_API_KEY"),
        (&["create"],br#"{"agent":{"model":"fixture"},"environment":{"type":"none"},"input":"selected","memory":"private-payload-canary"}"#.to_vec(),"API submission requires"),
        (&["create"],vec![b'x';65537],"API submission requires"),
        (&["result","session_fixture","--after","../outside"],vec![],"invalid bounded cloud identifier"),
    ];
    for (args, input, expected) in cases {
        let result = run(args, &input, false, home.path()).expect("preflight exit");
        assert!(!result.status.success());
        let err = String::from_utf8_lossy(&result.stderr);
        assert!(err.contains(expected), "{err}");
        assert!(!err.contains("private-payload-canary"));
    }
}
#[test]
fn capabilities_are_available_without_credentials_or_codex_configuration() {
    let home = tempfile::tempdir().expect("isolated home");
    let result = run(&["capabilities"], b"", false, home.path()).expect("capabilities exit");
    assert!(result.status.success());
    let value: serde_json::Value = serde_json::from_slice(&result.stdout).expect("JSON");
    assert_eq!(value["adapter"], "agents_api_v1");
    assert_eq!(value["configuration"], "http_environment");
    assert!(
        !run(&["-c", "anything=ignored", "check"], b"", true, home.path())
            .expect("configuration flag exit")
            .status
            .success()
    );
}
