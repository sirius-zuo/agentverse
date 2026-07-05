use agentverse_memory::{Embedder, EmbedderRegistry};
use httpmock::prelude::*;
use std::collections::HashMap;

fn settings(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[tokio::test]
async fn openai_embedder_batches_and_orders() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(POST).path("/embeddings");
        then.status(200).json_body(serde_json::json!({
            "data": [
                {"index": 1, "embedding": [0.4, 0.5, 0.6]},
                {"index": 0, "embedding": [0.1, 0.2, 0.3]}
            ]
        }));
    });
    let reg = EmbedderRegistry::with_builtins();
    let e = reg
        .build(
            "openai",
            &settings(&[
                ("model_name", "nomic-embed-text"),
                ("base_url", &server.url("")),
                ("dimensions", "3"),
            ]),
        )
        .unwrap(); // no api_key: local base_url rule
    let out = e.embed(&["a".into(), "b".into()]).await.unwrap();
    assert_eq!(out[0], vec![0.1, 0.2, 0.3]); // index-sorted
    assert_eq!(e.dimensions(), 3);
    m.assert();
}

#[tokio::test]
async fn openai_requires_key_without_base_url() {
    let reg = EmbedderRegistry::with_builtins();
    assert!(reg
        .build(
            "openai",
            &settings(&[("model_name", "m"), ("dimensions", "3")])
        )
        .is_err());
}

#[tokio::test]
async fn dimension_mismatch_is_error() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/embeddings");
        then.status(200).json_body(serde_json::json!({
            "data": [{"index": 0, "embedding": [0.1, 0.2]}]
        }));
    });
    let reg = EmbedderRegistry::with_builtins();
    let e = reg
        .build(
            "openai",
            &settings(&[
                ("model_name", "m"),
                ("base_url", &server.url("")),
                ("dimensions", "3"),
            ]),
        )
        .unwrap();
    assert!(e.embed(&["a".into()]).await.is_err());
}

#[tokio::test]
async fn gemini_embedder_works() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path_contains(":batchEmbedContents");
        then.status(200).json_body(serde_json::json!({
            "embeddings": [{"values": [1.0, 2.0]}]
        }));
    });
    let reg = EmbedderRegistry::with_builtins();
    let e = reg
        .build(
            "gemini",
            &settings(&[
                ("model_name", "text-embedding-004"),
                ("api_key", "k"),
                ("base_url", &server.url("")),
                ("dimensions", "2"),
            ]),
        )
        .unwrap();
    assert_eq!(e.embed(&["a".into()]).await.unwrap()[0], vec![1.0, 2.0]);
}

#[test]
fn unknown_provider_errors() {
    assert!(EmbedderRegistry::with_builtins()
        .build("nope", &HashMap::new())
        .is_err());
}
