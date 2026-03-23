//! Memory substrate for the OpenFang Agent Operating System.
//!
//! Provides a unified memory API over three storage backends:
//! - **Structured store** (SQLite): Key-value pairs, sessions, agent state
//! - **Semantic store**: Text-based search (Phase 1: LIKE matching, Phase 2: Qdrant vectors)
//! - **Knowledge graph** (SQLite): Entities and relations
//!
//! Agents interact with a single `Memory` trait that abstracts over all three stores.

pub mod consolidation;
pub mod knowledge;
pub mod migration;
pub mod runtime_store;
pub mod semantic;
pub mod session;
pub mod structured;
pub mod usage;

mod substrate;
pub use runtime_store::{
    AgentMessageRecord, AgentMessageStore, AgentRuntimeRecord, AgentRuntimeStore,
    AgentSessionRecord, AgentSessionStore, RuntimeStoreSet, ScheduleExecutionRecord,
    ScheduleExecutionStore, ScheduleRuntimeRecord, ScheduleRuntimeStore,
    AGENT_RUNTIME_CORE_MIGRATION_SQL, AGENT_SESSIONS_AND_MESSAGES_MIGRATION_SQL,
    SCHEDULE_RUNTIME_CORE_MIGRATION_SQL,
};
pub use substrate::MemorySubstrate;
