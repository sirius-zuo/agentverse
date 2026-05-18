use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub enum EnvelopeKind {
    Invoke,
    Result,
    Error,
    Ping,
    Pong,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct Envelope {
    pub id: Uuid,
    pub kind: EnvelopeKind,
    pub payload: serde_json::Value,
    pub metadata: HashMap<String, String>,
}

#[allow(dead_code)]
pub async fn write_envelope<W>(writer: &mut W, env: &Envelope) -> std::io::Result<()>
where
    W: AsyncWriteExt + Unpin,
{
    let mut line = serde_json::to_string(env)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    line.push('\n');
    writer.write_all(line.as_bytes()).await?;
    writer.flush().await
}

#[allow(dead_code)]
pub async fn read_envelope<R>(reader: &mut R) -> std::io::Result<Option<Envelope>>
where
    R: AsyncBufReadExt + Unpin,
{
    let mut line = String::new();
    let n = reader.read_line(&mut line).await?;
    if n == 0 {
        return Ok(None);
    }
    let env = serde_json::from_str(line.trim())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(Some(env))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn roundtrip_invoke() {
        let env = Envelope {
            id: uuid::Uuid::new_v4(),
            kind: EnvelopeKind::Invoke,
            payload: serde_json::json!({"task": "hello"}),
            metadata: std::collections::HashMap::new(),
        };
        let mut buf = Vec::new();
        write_envelope(&mut buf, &env).await.unwrap();
        assert!(buf.ends_with(b"\n"));
        let mut reader = tokio::io::BufReader::new(buf.as_slice());
        let got = read_envelope(&mut reader).await.unwrap();
        assert_eq!(got.map(|e| e.id), Some(env.id));
    }

    #[tokio::test]
    async fn read_eof_returns_none() {
        let mut reader = tokio::io::BufReader::new(b"".as_slice());
        let got = read_envelope(&mut reader).await.unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn kind_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&EnvelopeKind::Invoke).unwrap(),
            "\"invoke\""
        );
        assert_eq!(
            serde_json::to_string(&EnvelopeKind::Pong).unwrap(),
            "\"pong\""
        );
    }
}
