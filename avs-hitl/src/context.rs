use crate::policy::HitlPolicy;
use crate::queue::ApprovalQueue;
use crate::types::{ApprovalRequest, InterruptKind};
use agentverse::hitl::{ApprovalId, HitlHook};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

pub struct HitlContext {
    pub session_id: Uuid,
    pub skill_id: Option<String>,
    policy: HitlPolicy,
    queue: Arc<dyn ApprovalQueue>,
}

impl HitlContext {
    pub fn new(
        session_id: Uuid,
        skill_id: Option<String>,
        policy: HitlPolicy,
        queue: Arc<dyn ApprovalQueue>,
    ) -> Self {
        Self {
            session_id,
            skill_id,
            policy,
            queue,
        }
    }
}

#[async_trait::async_trait]
impl HitlHook for HitlContext {
    async fn check_tool(&self, tool_name: &str, args: &Value) -> Option<(ApprovalId, String)> {
        let kind = if HitlPolicy::is_checkpoint_tool(tool_name) {
            let name = args["name"].as_str().unwrap_or("unknown").to_string();
            let payload = args["payload"].clone();
            InterruptKind::SkillCheckpoint {
                checkpoint_name: name,
                payload,
            }
        } else if self
            .policy
            .requires_tool_approval(self.skill_id.as_deref(), tool_name)
        {
            InterruptKind::ToolApproval {
                tool_name: tool_name.to_string(),
                args: args.clone(),
            }
        } else {
            return None;
        };

        let kind_json = serde_json::to_string(&kind).unwrap_or_default();
        let req = ApprovalRequest::new(self.session_id, kind);
        match self.queue.submit(req.clone()).await {
            Ok(id) => Some((id, kind_json)),
            Err(e) => {
                tracing::error!(
                    error = %e,
                    tool = tool_name,
                    "HITL queue submit failed; blocking tool as fail-safe (sentinel approval_id)"
                );
                // Return a sentinel id that won't be in the queue.
                // The tool is blocked. Any resume attempt will surface HitlError::NotFound,
                // making the queue failure visible to the operator.
                Some((uuid::Uuid::new_v4(), kind_json))
            }
        }
    }
}
