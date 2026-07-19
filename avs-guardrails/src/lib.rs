// avs-guardrails/src/lib.rs
pub mod action_guard;
pub mod output_guard;
pub mod prompt_guard;
pub mod rate_limiter;

/// Deprecated compatibility re-export. Runtime tool interception uses
/// `HitlContext` plus `ToolRegistry::execute_many_hitl` through
/// `Agent::invoke`/ReAct.
#[deprecated(
    since = "0.1.0",
    note = "ActionGuard is retained for compatibility; use HitlContext with ToolRegistry::execute_many_hitl through Agent::invoke/ReAct instead"
)]
#[allow(deprecated)] // Re-export the deprecated type for source compatibility.
pub use action_guard::ActionGuard;
pub use output_guard::check_output;
pub use prompt_guard::check_prompt;
pub use prompt_guard::GuardrailError;
pub use rate_limiter::RateLimiter;
