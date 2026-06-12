use crate::error::HitlError;
use crate::queue::ApprovalQueue;
use crate::types::{ApprovalDecision, ApprovalId, ApprovalRequest, ApprovalStatus};
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Mutex;

struct Entry {
    request: ApprovalRequest,
    status:  ApprovalStatus,
}

pub struct InMemoryQueue {
    entries: Mutex<HashMap<ApprovalId, Entry>>,
}

impl InMemoryQueue {
    pub fn new() -> Self {
        Self { entries: Mutex::new(HashMap::new()) }
    }
}

impl Default for InMemoryQueue {
    fn default() -> Self { Self::new() }
}

#[async_trait::async_trait]
impl ApprovalQueue for InMemoryQueue {
    async fn submit(&self, req: ApprovalRequest) -> Result<ApprovalId, HitlError> {
        let id = req.id;
        self.entries.lock().unwrap().insert(id, Entry { request: req, status: ApprovalStatus::Pending });
        Ok(id)
    }

    async fn resolve(&self, id: ApprovalId, decision: ApprovalDecision) -> Result<(), HitlError> {
        let mut entries = self.entries.lock().unwrap();
        let entry = entries.get_mut(&id).ok_or(HitlError::NotFound(id))?;
        if entry.status != ApprovalStatus::Pending {
            return Err(HitlError::AlreadyResolved(id));
        }
        entry.status = ApprovalStatus::Resolved(decision);
        Ok(())
    }

    async fn poll(&self, id: ApprovalId) -> Result<ApprovalStatus, HitlError> {
        self.entries
            .lock().unwrap()
            .get(&id)
            .map(|e| e.status.clone())
            .ok_or(HitlError::NotFound(id))
    }

    async fn sweep_expired(&self) -> Result<u64, HitlError> {
        let now = Utc::now();
        let mut count = 0u64;
        for entry in self.entries.lock().unwrap().values_mut() {
            if entry.status == ApprovalStatus::Pending {
                if let Some(exp) = entry.request.expires_at {
                    if exp < now {
                        entry.status = ApprovalStatus::Expired;
                        count += 1;
                    }
                }
            }
        }
        Ok(count)
    }
}
