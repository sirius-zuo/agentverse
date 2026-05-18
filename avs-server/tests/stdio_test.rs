//! Integration test: spawn the agentverse binary with --stdio and verify
//! it speaks the Envelope protocol correctly.

use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::time::timeout;
use uuid::Uuid;

fn binary_path() -> std::path::PathBuf {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let mut path = std::path::PathBuf::from(manifest);
    path.push("../target/debug/agentverse");
    path
}

fn make_envelope(kind: &str, payload: serde_json::Value) -> String {
    let env = serde_json::json!({
        "id": Uuid::new_v4().to_string(),
        "kind": kind,
        "payload": payload,
        "metadata": {}
    });
    let mut s = serde_json::to_string(&env).unwrap();
    s.push('\n');
    s
}

#[tokio::test]
async fn ping_returns_pong() {
    let binary = binary_path();
    if !binary.exists() {
        eprintln!("Binary not found at {:?} — run `cargo build` first", binary);
        return;
    }

    let mut child = Command::new(&binary)
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .env("MODEL_API_KEY", "test-key")
        .spawn()
        .expect("failed to spawn agentverse binary");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    let ping = make_envelope("ping", serde_json::Value::Null);
    stdin.write_all(ping.as_bytes()).await.unwrap();
    stdin.flush().await.unwrap();

    let mut line = String::new();
    timeout(Duration::from_secs(2), reader.read_line(&mut line))
        .await
        .expect("timed out waiting for Pong")
        .unwrap();

    let response: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(response["kind"], "pong");

    drop(stdin);
    let _ = timeout(Duration::from_secs(2), child.wait()).await;
}

#[tokio::test]
async fn invoke_returns_result_or_error_envelope() {
    let binary = binary_path();
    if !binary.exists() {
        eprintln!("Binary not found at {:?} — run `cargo build` first", binary);
        return;
    }

    let mut child = Command::new(&binary)
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .env("MODEL_API_KEY", "test-key")
        .spawn()
        .expect("failed to spawn agentverse binary");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    let invoke = make_envelope("invoke", serde_json::json!({"message": "Hello, agent!"}));
    stdin.write_all(invoke.as_bytes()).await.unwrap();
    stdin.flush().await.unwrap();

    let mut line = String::new();
    timeout(Duration::from_secs(5), reader.read_line(&mut line))
        .await
        .expect("timed out waiting for response")
        .unwrap();

    let response: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
    let kind = response["kind"].as_str().unwrap();
    assert!(
        kind == "result" || kind == "error",
        "unexpected kind: {kind}"
    );
    assert!(response["metadata"]["model"].is_string());
    assert!(response["metadata"]["provider"].is_string());

    drop(stdin);
    let _ = timeout(Duration::from_secs(2), child.wait()).await;
}

#[tokio::test]
async fn eof_causes_clean_exit() {
    let binary = binary_path();
    if !binary.exists() {
        return;
    }

    let mut child = Command::new(&binary)
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .env("MODEL_API_KEY", "test-key")
        .spawn()
        .expect("failed to spawn agentverse binary");

    // Drop stdin immediately → EOF
    drop(child.stdin.take());

    let status = timeout(Duration::from_secs(3), child.wait())
        .await
        .expect("binary did not exit after EOF");

    // Process must have exited
    assert!(status.is_ok());
}
