use crate::envelope::{read_envelope, write_envelope, Envelope, EnvelopeKind};
use agentverse::Agent;
use std::collections::HashMap;
use tokio::io::BufReader;
use tokio::net::UnixListener;
use tracing::info;

/// Runs the agent in Aether Unix socket mode.
///
/// Reads AETHER_SOCKET_PATH from the environment, binds a Unix socket,
/// skips probe connections (readiness checks that close without data),
/// handles one real Invoke/Ping, then exits.
/// This matches PerRequest spawn semantics: one process per call.
pub async fn run_unix(agent: Agent, model_name: String, provider_name: String) {
    let socket_path =
        std::env::var("AETHER_SOCKET_PATH").expect("AETHER_SOCKET_PATH must be set");

    let _ = std::fs::remove_file(&socket_path);

    let listener = UnixListener::bind(&socket_path).expect("failed to bind Unix socket");

    for _ in 0..200 {
        let (stream, _) = match listener.accept().await {
            Ok(conn) => conn,
            Err(e) => {
                eprintln!("accept error: {e}");
                break;
            }
        };

        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);

        match read_envelope(&mut reader).await {
            Ok(None) => continue, // probe connection — no data
            Ok(Some(env)) => {
                info!(id = %env.id, kind = ?env.kind, "envelope received");
                let response = dispatch(&env, &agent, &model_name, &provider_name).await;
                if let Err(e) = write_envelope(&mut write_half, &response).await {
                    eprintln!("envelope write error: {e}");
                }
                break;
            }
            Err(e) => {
                eprintln!("envelope read error: {e}");
                break;
            }
        }
    }

    let _ = std::fs::remove_file(&socket_path);
}

async fn dispatch(
    env: &Envelope,
    agent: &Agent,
    model_name: &str,
    provider_name: &str,
) -> Envelope {
    match env.kind {
        EnvelopeKind::Ping => Envelope {
            id: env.id,
            kind: EnvelopeKind::Pong,
            payload: serde_json::Value::Null,
            metadata: HashMap::new(),
        },
        EnvelopeKind::Invoke => {
            let message = env
                .payload
                .get("message")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| env.payload.to_string());

            match agent.invoke("aether", &message).await {
                Ok(response) => {
                    let mut metadata = response_metadata(model_name, provider_name);
                    for key in ["trace_id", "workflow_id", "node"] {
                        if let Some(v) = env.metadata.get(key) {
                            metadata.insert(key.to_string(), v.clone());
                        }
                    }
                    Envelope {
                        id: env.id,
                        kind: EnvelopeKind::Result,
                        payload: serde_json::json!({ "message": response }),
                        metadata,
                    }
                }
                Err(e) => Envelope {
                    id: env.id,
                    kind: EnvelopeKind::Error,
                    payload: serde_json::json!({ "error": e.to_string() }),
                    metadata: response_metadata(model_name, provider_name),
                },
            }
        }
        _ => Envelope {
            id: env.id,
            kind: EnvelopeKind::Error,
            payload: serde_json::json!({ "error": format!("unexpected kind: {:?}", env.kind) }),
            metadata: HashMap::new(),
        },
    }
}

fn response_metadata(model_name: &str, provider_name: &str) -> HashMap<String, String> {
    HashMap::from([
        ("model".to_string(), model_name.to_string()),
        ("provider".to_string(), provider_name.to_string()),
    ])
}
