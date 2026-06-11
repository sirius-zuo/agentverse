use crate::result::{SubAgentError, SubAgentResult};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use uuid::Uuid;

pub struct SubAgentHandle {
    pub id: Uuid,
    receiver: Option<oneshot::Receiver<Result<SubAgentResult, SubAgentError>>>,
    handle: JoinHandle<()>,
}

impl SubAgentHandle {
    pub async fn await_result(mut self) -> Result<SubAgentResult, SubAgentError> {
        let rx = self.receiver.take().expect("await_result called twice");
        rx.await
            .unwrap_or_else(|_| Err(SubAgentError::Panic("sender dropped".into())))
    }

    pub fn from_parts(
        id: Uuid,
        receiver: oneshot::Receiver<Result<SubAgentResult, SubAgentError>>,
        handle: JoinHandle<()>,
    ) -> Self {
        Self { id, receiver: Some(receiver), handle }
    }
}

impl Drop for SubAgentHandle {
    fn drop(&mut self) {
        self.handle.abort();
    }
}
