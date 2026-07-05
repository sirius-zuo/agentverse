pub mod manager;

pub use manager::SessionManager;

// Session-memory storage lives in agentverse-memory (the home of all memory
// tiers); re-exported here so session-lifecycle consumers keep one import path.
pub use agentverse_memory::session::{
    InterruptedState, Session, SessionId, SessionMemory, SessionMemoryError, SessionStatus,
    SqliteSessionMemory,
};

// Submodule aliases for existing qualified import paths (e.g.
// `agentverse_session::session::Session`, `agentverse_session::sqlite::SqliteSessionMemory`,
// `agentverse_session::store::SessionMemory`), so pre-existing consumers that
// import via these paths keep compiling unchanged.
pub mod session {
    pub use agentverse_memory::session::{Session, SessionId, SessionStatus};
}
pub mod sqlite {
    pub use agentverse_memory::session::SqliteSessionMemory;
}
pub mod store {
    pub use agentverse_memory::session::{InterruptedState, SessionMemory, SessionMemoryError};
}
