use crate::context::SubAgentContext;
use crate::handle::SubAgentHandle;
use crate::result::{SubAgentError, SubAgentResult};
use crate::spec::{ModelOverride, SubAgentSpec};
use agentverse::{
    AgentError, ConnectionManager, LlmRunner, Message, MessageRole, ModelError, PromptRegistry,
    UsageStats,
};
use agentverse_react::cycle::CycleAction;
use agentverse_react::CycleSkeleton;
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
        // Primary depth guard: filter_by_names already excludes spawn_subagent so a
        // subagent can't structurally nest. This check is defense-in-depth for callers
        // that construct SubAgentContext directly (e.g. tests).
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
                let registry = agentverse::ProviderRegistry::with_builtins();
                let cm = self
                    .connection_manager
                    .with_model(&model_name, &registry)
                    .map_err(agentverse::AgentError::Model)?;
                LlmRunner::new(Arc::new(cm))
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

    /// Run multiple SubAgents concurrently and collect all results.
    ///
    /// **Note:** Results are returned in completion order, not input order.
    /// Callers that need to correlate results with inputs should include an
    /// identifier in the spec name or objective.
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
        let handle = tokio::spawn(async move {
            let _ = tx.send(executor.run(&spec, ctx).await);
        });
        SubAgentHandle::from_parts(Uuid::new_v4(), rx, handle)
    }

    /// Register a `SubAgentTool` into `registry` so the LLM can invoke subagents via tool calls.
    pub fn register_tool(executor: &Arc<Self>, registry: &Arc<ToolRegistry>) {
        // depth=0: tools registered at the agent root always spawn top-level subagents
        registry.register(crate::tool::SubAgentTool::new(Arc::clone(executor), 0));
    }

    /// Atomically register the root `SubAgentTool` if one is not already present.
    pub fn register_tool_if_absent(executor: &Arc<Self>, registry: &Arc<ToolRegistry>) -> bool {
        registry.register_if_absent(crate::tool::SubAgentTool::new(Arc::clone(executor), 0))
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
            other => {
                tracing::warn!(
                    alias = other,
                    "unknown model alias — passing through as raw model ID"
                );
                other.to_string()
            }
        },
    }
}

fn build_initial_messages(spec: &SubAgentSpec, ctx: &SubAgentContext) -> Vec<Message> {
    let mut msgs = Vec::new();
    if let Some(sys) = &spec.system_prompt {
        msgs.push(Message::text(MessageRole::System, sys.clone()));
    }
    let mut user_content = format!("Objective: {}", spec.objective);
    if !ctx.resources.is_empty() {
        user_content.push_str("\n\n## Context\n");
        for r in &ctx.resources {
            user_content.push_str(&format!("\n### {}\n{}\n", r.label, r.content));
        }
    }
    msgs.push(Message::text(MessageRole::User, user_content));
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
    let active_tool_names = skeleton.tools.tool_names();

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

        let definitions = skeleton
            .tools
            .tool_definitions_for(&active_tool_names)
            .map_err(|e| {
                SubAgentError::Llm(AgentError::Config(agentverse::ConfigError::Invalid(
                    format!("invalid tool schema for active tools: {e}"),
                )))
            })?;
        // buf.clone() is O(n) in the message history. For realistic budgets
        // (≤20 steps) this is negligible; revisit if budgets grow large.
        let response = if definitions.is_empty() {
            skeleton.runner.invoke(buf.clone()).await
        } else {
            skeleton
                .runner
                .invoke_with_tools(buf.clone(), definitions)
                .await
        }
        .map_err(SubAgentError::Llm)?;

        total_usage += response.usage;
        skeleton
            .check_output_guardrail(&response.as_text())
            .map_err(SubAgentError::Llm)?;

        tracing::debug!(step = steps, "subagent step");

        match agentverse_react::cycle::action_from_response(&response) {
            CycleAction::ToolCalls { calls } => {
                buf.push(Message {
                    role: MessageRole::Assistant,
                    content: response.content.clone(),
                });
                let order: Vec<(String, String)> = calls
                    .iter()
                    .map(|c| (c.id.clone(), c.name.clone()))
                    .collect();
                let results = skeleton
                    .execute_many(calls)
                    .await
                    .map_err(SubAgentError::Llm)?;
                let results = agentverse_react::cycle::reconcile_and_order_results(results, &order);
                tracing::debug!(count = results.len(), "subagent tool calls executed");
                buf.push(Message {
                    role: MessageRole::Tool,
                    content: agentverse_react::cycle::results_to_tool_result_blocks(results),
                });
            }
            CycleAction::Done { answer } => {
                tracing::info!(steps, "subagent completed");
                return Ok(SubAgentResult {
                    answer,
                    usage: total_usage,
                    steps,
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
