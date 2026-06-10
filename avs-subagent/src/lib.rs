pub mod context;
pub mod executor;
pub mod handle;
pub mod result;
pub mod skill;
pub mod spec;
pub mod tool;

pub use context::{ResourceContent, SubAgentContext};
pub use executor::SubAgentExecutor;
pub use handle::SubAgentHandle;
pub use result::{SubAgentError, SubAgentResult};
pub use skill::load_skill_subagent_spec;
pub use spec::{Budget, ModelOverride, SubAgentSpec};
pub use tool::{SubAgentArgs, SubAgentTool};
