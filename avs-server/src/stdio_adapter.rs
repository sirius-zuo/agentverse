use std::collections::HashMap;
use tokio::io::BufReader;
use tracing::info;
use agentverse::Agent;
use crate::envelope::{read_envelope, write_envelope, Envelope, EnvelopeKind};

pub async fn run_stdio(agent: Agent, model_name: String, provider_name: String) {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin);
    let mut writer = stdout;

    loop {
        let env = match read_envelope(&mut reader).await {
            Ok(Some(e)) => e,
            Ok(None) => break, // EOF — clean exit
            Err(e) => {
                eprintln!("envelope read error: {e}");
                break;
            }
        };

        let response = dispatch(&env, &agent, &model_name, &provider_name).await;
        if let Err(e) = write_envelope(&mut writer, &response).await {
            eprintln!("envelope write error: {e}");
            break;
        }
    }
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
            let message = env.payload.get("message")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| env.payload.to_string());

            info!(id = %env.id, "Invoke received");

            match agent.invoke("aether", &message).await {
                Ok(response) => {
                    let mut metadata = response_metadata(model_name, provider_name, 0, 0);
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
                Err(e) => {
                    let metadata = response_metadata(model_name, provider_name, 0, 0);
                    Envelope {
                        id: env.id,
                        kind: EnvelopeKind::Error,
                        payload: serde_json::json!({ "error": e.to_string() }),
                        metadata,
                    }
                }
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

fn response_metadata(
    model_name: &str,
    provider_name: &str,
    tokens_in: u64,
    tokens_out: u64,
) -> HashMap<String, String> {
    HashMap::from([
        ("model".to_string(), model_name.to_string()),
        ("provider".to_string(), provider_name.to_string()),
        ("tokens_input".to_string(), tokens_in.to_string()),
        ("tokens_output".to_string(), tokens_out.to_string()),
    ])
}
