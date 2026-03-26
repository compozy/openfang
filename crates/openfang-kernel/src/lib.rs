//! Core kernel for the OpenFang Agent Operating System.
//!
//! The kernel manages agent lifecycles, memory, permissions, scheduling,
//! and inter-agent communication.

pub mod approval;
pub mod auth;
pub mod auto_reply;
pub mod background;
pub mod capabilities;
pub mod config;
pub mod config_reload;
pub mod cron;
mod db;
mod db_migration;
pub mod error;
pub mod event_bus;
pub mod heartbeat;
pub mod kernel;
pub mod looper;
pub mod metering;
pub mod pack_installer;
pub mod pack_registry;
pub mod pairing;
pub mod registry;
pub mod scheduler;
pub mod supervisor;
pub mod trigger_v2;
pub mod triggers;
pub mod whatsapp_gateway;
pub mod wizard;
pub mod workflow;
pub mod workflow_compiler;

pub use db::DatabaseHealth;
pub use kernel::AgentMessageDispatch;
pub use kernel::DeliveryTracker;
pub use kernel::OpenFangKernel;
