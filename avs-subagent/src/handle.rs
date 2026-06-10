use crate::result::{SubAgentError, SubAgentResult};
use tokio::sync::oneshot;
use uuid::Uuid;

pub struct SubAgentHandle {
    pub id: Uuid,
    receiver: oneshot::Receiver<Result<SubAgentResult, SubAgentError>>,
}

impl SubAgentHandle {
    pub async fn await_result(self) -> Result<SubAgentResult, SubAgentError> {
        self.receiver
            .await
            .unwrap_or_else(|_| Err(SubAgentError::Panic("sender dropped".into())))
    }

    pub fn is_finished(&self) -> bool {
        false
    }

    pub fn from_parts(
        id: Uuid,
        receiver: oneshot::Receiver<Result<SubAgentResult, SubAgentError>>,
    ) -> Self {
        Self { id, receiver }
    }
}
