use crate::context::SubAgentContext;
use crate::handle::SubAgentHandle;
use crate::result::{SubAgentError, SubAgentResult};
use crate::spec::{ModelOverride, SubAgentSpec};
use agentverse::{
    AgentError, ConnectionManager, LlmRunner, Message, MessageRole, ModelError, PromptRegistry,
    UsageStats,
};
use agentverse_react::parse::parse_response;
use agentverse_react::{CycleAction, CycleSkeleton};
use agentverse_tools::ToolRegistry;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::oneshot;
use tokio::task::JoinSet;
use uuid::Uuid;

#[derive(Clone)]
pub struct SubAgentExecutor {
    connection_manager: Arc<ConnectionManager>,
    parent_tools: Arc<ToolRegistry>,
    prompts: Arc<PromptRegistry>,
    max_depth: usize,
}

impl SubAgentExecutor {
    pub fn new(
        connection_manager: Arc<ConnectionManager>,
        parent_tools: Arc<ToolRegistry>,
        prompts: Arc<PromptRegistry>,
    ) -> Self {
        Self {
            connection_manager,
            parent_tools,
            prompts,
            max_depth: 1,
        }
    }

    pub async fn run(
        &self,
        spec: &SubAgentSpec,
        ctx: SubAgentContext,
    ) -> Result<SubAgentResult, SubAgentError> {
        if ctx.depth >= self.max_depth {
            return Err(SubAgentError::DepthExceeded);
        }

        // Scope tools — spawn_subagent excluded by filter_by_names
        let scoped_tools = self.parent_tools.filter_by_names(&spec.allowed_tools);

        // Resolve model
        let runner = Arc::new(match &spec.model {
            None => LlmRunner::new(Arc::clone(&self.connection_manager)),
            Some(override_) => {
                let model_name = resolve_model_name(override_);
                LlmRunner::new(Arc::new(self.connection_manager.with_model(&model_name)))
            }
        });

        let skeleton = CycleSkeleton::new(
            Arc::clone(&runner),
            Arc::clone(&self.prompts),
            scoped_tools,
            spec.budget.max_steps,
        );

        let messages = build_initial_messages(spec, &ctx);
        let budget = spec.budget.clone();
        let start = Instant::now();

        match tokio::time::timeout(budget.timeout, run_cycle(&skeleton, messages, &budget)).await {
            Err(_) => Err(SubAgentError::Timeout {
                elapsed: start.elapsed(),
            }),
            Ok(inner) => inner,
        }
    }

    pub async fn run_many(
        &self,
        tasks: Vec<(SubAgentSpec, SubAgentContext)>,
    ) -> Vec<Result<SubAgentResult, SubAgentError>> {
        let mut set = JoinSet::new();
        for (spec, ctx) in tasks {
            let executor = self.clone();
            set.spawn(async move { executor.run(&spec, ctx).await });
        }
        let mut results = Vec::new();
        while let Some(outcome) = set.join_next().await {
            results.push(outcome.unwrap_or_else(|e| Err(SubAgentError::Panic(e.to_string()))));
        }
        results
    }

    pub fn spawn(&self, spec: SubAgentSpec, ctx: SubAgentContext) -> SubAgentHandle {
        let (tx, rx) = oneshot::channel();
        let executor = self.clone();
        tokio::spawn(async move {
            let _ = tx.send(executor.run(&spec, ctx).await);
        });
        SubAgentHandle::from_parts(Uuid::new_v4(), rx)
    }

    /// Register a `SubAgentTool` into `registry` so the LLM can invoke subagents via tool calls.
    pub fn register_tool(executor: &Arc<Self>, registry: &Arc<ToolRegistry>) {
        registry.register(crate::tool::SubAgentTool::new(Arc::clone(executor), 0));
    }
}

// ── internal helpers ──────────────────────────────────────────────────────────

fn resolve_model_name(override_: &ModelOverride) -> String {
    match override_ {
        ModelOverride::Id(id) => id.clone(),
        ModelOverride::Alias(alias) => match alias.as_str() {
            "haiku" => "claude-haiku-4-5-20251001".to_string(),
            "sonnet" => "claude-sonnet-4-6".to_string(),
            "opus" => "claude-opus-4-8".to_string(),
            other => other.to_string(),
        },
    }
}

fn build_initial_messages(spec: &SubAgentSpec, ctx: &SubAgentContext) -> Vec<Message> {
    let mut msgs = Vec::new();
    if let Some(sys) = &spec.system_prompt {
        msgs.push(Message {
            role: MessageRole::System,
            content: sys.clone(),
        });
    }
    let mut user_content = format!("Objective: {}", spec.objective);
    if !ctx.resources.is_empty() {
        user_content.push_str("\n\n## Context\n");
        for r in &ctx.resources {
            user_content.push_str(&format!("\n### {}\n{}\n", r.label, r.content));
        }
    }
    msgs.push(Message {
        role: MessageRole::User,
        content: user_content,
    });
    msgs
}

async fn run_cycle(
    skeleton: &CycleSkeleton,
    messages: Vec<Message>,
    budget: &crate::spec::Budget,
) -> Result<SubAgentResult, SubAgentError> {
    let mut buf = skeleton.prepare_buffer(messages);
    let mut total_usage = UsageStats::default();
    let mut steps = 0usize;

    loop {
        let tokens_used = total_usage.input_tokens + total_usage.output_tokens;
        if tokens_used > budget.max_tokens {
            return Err(SubAgentError::TokenBudgetExceeded {
                used: tokens_used,
                limit: budget.max_tokens,
            });
        }
        if steps >= budget.max_steps {
            return Err(SubAgentError::StepBudgetExceeded { steps });
        }
        steps += 1;

        let response = skeleton
            .runner
            .invoke(buf.clone())
            .await
            .map_err(SubAgentError::Llm)?;

        total_usage += response.usage;
        skeleton
            .check_output_guardrail(&response.content)
            .map_err(SubAgentError::Llm)?;

        tracing::debug!(step = steps, "subagent step");

        match parse_response(&response.content) {
            CycleAction::Done { answer } => {
                tracing::info!(steps, "subagent completed");
                return Ok(SubAgentResult {
                    answer,
                    usage: total_usage,
                    steps,
                });
            }
            CycleAction::Continue { thought } => {
                buf.push(Message {
                    role: MessageRole::Assistant,
                    content: format!("Thought: {thought}"),
                });
                buf.push(Message {
                    role: MessageRole::User,
                    content: "Either call a tool or give your final answer (Answer: ...).".into(),
                });
            }
            CycleAction::ToolCall { tool_name, args } => {
                buf.push(Message {
                    role: MessageRole::Assistant,
                    content: response.content.clone(),
                });
                let result = skeleton
                    .execute_tool(&tool_name, args)
                    .await
                    .map_err(SubAgentError::Llm)?;
                tracing::debug!(tool = %tool_name, "subagent tool call");
                buf.push(Message {
                    role: MessageRole::User,
                    content: format!("Tool: {tool_name}\nResult: {result}"),
                });
            }
            CycleAction::ToolCalls { calls } => {
                buf.push(Message {
                    role: MessageRole::Assistant,
                    content: response.content.clone(),
                });
                let results = skeleton
                    .execute_many(calls)
                    .await
                    .map_err(SubAgentError::Llm)?;
                let obs = results
                    .iter()
                    .map(|r| {
                        let v = match &r.result {
                            Ok(v) => v.to_string(),
                            Err(e) => format!("Error: {e}"),
                        };
                        format!("Tool: {}\nResult: {v}", r.name)
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n");
                buf.push(Message {
                    role: MessageRole::User,
                    content: obs,
                });
            }
            CycleAction::Error { message } => {
                return Err(SubAgentError::Llm(AgentError::Model(
                    ModelError::InvalidResponse(message),
                )));
            }
        }
    }
}
