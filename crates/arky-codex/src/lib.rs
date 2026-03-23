//! Codex App Server provider implementation for Arky.
//!
//! This crate speaks newline-delimited JSON-RPC with the Codex app server and
//! exposes the transport, notification routing, approval, and thread/session
//! primitives needed by the higher-level Arky runtime.

mod accumulator;
mod app_server;
mod approval;
mod config;
mod dedup;
mod dispatcher;
mod model_service;
mod notification;
mod pipeline;
mod provider;
mod registry;
mod rpc;
mod scheduler;
mod thread;
mod tool_payloads;

pub use crate::{
    accumulator::{TextAccumulator, ToolRuntimeState, ToolTracker},
    app_server::CodexAppServer,
    approval::{ApprovalDecision, ApprovalHandler, ApprovalMode, ApprovalRequest},
    config::{
        CodexCapabilityConfig, CodexProcessConfig, CodexProviderConfig, CodexSandboxConfig,
        CodexSandboxExclusions, CodexWorkspaceConfig,
    },
    dedup::{FingerprintDeduper, fingerprint_notification},
    dispatcher::{CodexEventDispatcher, NormalizedNotification},
    model_service::{CodexModelDescriptor, CodexModelService, ModelListPage},
    notification::{
        CodexNotification, NotificationRouter, extract_scope_id_from_value,
        extract_thread_id_from_value,
    },
    pipeline::{CodexStreamPipeline, CodexStreamState},
    provider::CodexProvider,
    registry::{CodexServerLease, CodexServerRegistry},
    rpc::{
        CodexServerRequest, InitializeCapabilities, InitializeClientInfo, InitializeParams,
        InitializeResponse, JsonRpcErrorObject, JsonRpcId, RpcTransport, RpcTransportConfig,
    },
    scheduler::{Scheduler, SchedulerPermit},
    thread::{
        CompactThreadParams, ThreadManager, ThreadOpenParams, ThreadStartResult,
        TurnNotificationStream, TurnStartParams,
    },
    tool_payloads::{
        build_tool_input_payload, build_tool_result_payload, canonical_tool_name, payload_has_error,
    },
};
