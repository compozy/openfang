//! Typed runtime configuration for the Codex provider.

use std::{collections::BTreeMap, path::PathBuf, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ApprovalMode;

/// Process-launch behavior for Codex runtimes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexProcessConfig {
    /// Whether `npx -y @openai/codex` may be used as a fallback.
    pub allow_npx: bool,
    /// Whether inherited environment variables should be sanitized.
    pub sanitize_environment: bool,
    /// Whether to enable experimental app-server APIs.
    pub experimental_api: bool,
}

impl Default for CodexProcessConfig {
    fn default() -> Self {
        Self {
            allow_npx: true,
            sanitize_environment: true,
            experimental_api: false,
        }
    }
}

/// Sandbox exclusion rules for Codex runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CodexSandboxExclusions {
    /// Whether the sandbox should exclude the tmpdir environment variable.
    pub sandbox_exclude_tmpdir_env_var: bool,
    /// Whether the sandbox should exclude `/tmp`.
    pub sandbox_exclude_slash_tmp: bool,
}

/// Sandbox-related Codex runtime settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CodexSandboxConfig {
    /// Optional sandbox mode override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_mode: Option<String>,
    /// Additional writable roots granted to the sandbox.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sandbox_writable_roots: Vec<PathBuf>,
    /// Optional network access override for sandboxed runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_network_access: Option<bool>,
    /// Whether full-auto mode is requested.
    pub full_auto: bool,
    /// Whether approvals and sandboxing may be bypassed entirely.
    pub dangerously_bypass_approvals_and_sandbox: bool,
    /// Sandbox exclusion rules.
    #[serde(flatten)]
    pub exclusions: CodexSandboxExclusions,
}

/// Workspace interaction toggles for Codex runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CodexWorkspaceConfig {
    /// Whether git-repo checks should be skipped.
    pub skip_git_repo_check: bool,
    /// Whether the plan tool should be enabled.
    pub include_plan_tool: bool,
    /// Whether the last Codex session should be resumed automatically.
    pub resume_last: bool,
}

/// Capability toggles and integrations for Codex runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CodexCapabilityConfig {
    /// Optional feature-flag overrides.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub feature_flags: BTreeMap<String, bool>,
    /// Whether OSS mode is enabled.
    pub oss: bool,
    /// Whether web search is enabled.
    pub web_search: bool,
    /// Flattened MCP server settings.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub mcp_servers: BTreeMap<String, Value>,
    /// Whether the Rust MCP client should be enabled.
    pub rmcp_client: bool,
}

/// Runtime configuration for the Codex provider and shared app-server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexProviderConfig {
    /// Preferred binary path or command name.
    pub binary: String,
    /// Process-launch behavior.
    #[serde(flatten)]
    pub process: CodexProcessConfig,
    /// Optional logical key used to share one app-server across providers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_app_server_key: Option<String>,
    /// Optional working directory for spawned processes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    /// Additional directories exposed to the Codex runtime.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub add_dirs: Vec<PathBuf>,
    /// Environment overrides applied to every subprocess.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Arguments used when validating the binary.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub version_args: Vec<String>,
    /// Arguments used to launch the app-server.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub app_server_args: Vec<String>,
    /// Per-request JSON-RPC timeout.
    #[serde(with = "duration_millis")]
    pub request_timeout: Duration,
    /// Scheduler acquire timeout.
    #[serde(with = "duration_millis")]
    pub scheduler_timeout: Duration,
    /// Maximum time allowed for app-server startup.
    #[serde(with = "duration_millis")]
    pub startup_timeout: Duration,
    /// Idle timeout before an unused shared server is shut down.
    #[serde(with = "duration_millis")]
    pub idle_shutdown_timeout: Duration,
    /// TTL for cached `model/list` results.
    #[serde(with = "duration_millis")]
    pub model_cache_ttl: Duration,
    /// Maximum concurrent in-flight RPC requests.
    pub max_in_flight_requests: usize,
    /// Maximum queued RPC requests.
    pub max_queued_requests: usize,
    /// Approval behavior for server-initiated requests.
    #[serde(with = "approval_mode_serde")]
    pub approval_mode: ApprovalMode,
    /// Approval policy sent with `turn/start`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_policy: Option<String>,
    /// Client identity used in the initialize handshake.
    pub client_name: String,
    /// Client version used in the initialize handshake.
    pub client_version: String,
    /// Sandbox-related settings.
    #[serde(flatten)]
    pub sandbox: CodexSandboxConfig,
    /// Optional color mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Optional file to write the last assistant message into.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_last_message_file: Option<PathBuf>,
    /// Optional exec-policy rules file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exec_policy_rules_path: Option<PathBuf>,
    /// Optional compaction token limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compaction_token_limit: Option<u64>,
    /// Optional model context window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_context_window: Option<u64>,
    /// Optional compaction prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact_prompt: Option<String>,
    /// Optional system prompt override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    /// Optional appended system prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub append_system_prompt: Option<String>,
    /// Optional reasoning effort default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    /// Optional reasoning summary default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_summary: Option<String>,
    /// Optional reasoning summary format default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_summary_format: Option<String>,
    /// Optional model verbosity default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_verbosity: Option<String>,
    /// Workspace interaction toggles.
    #[serde(flatten)]
    pub workspace: CodexWorkspaceConfig,
    /// Optional profile name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// Capability toggles and integrations.
    #[serde(flatten)]
    pub capability: CodexCapabilityConfig,
    /// Additional config override values merged into each turn.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub config_overrides: BTreeMap<String, Value>,
}

impl CodexProviderConfig {
    /// Builds the sharing key used by the server registry.
    #[must_use]
    pub fn registry_key(&self) -> String {
        if let Some(key) = &self.shared_app_server_key {
            return format!("shared:{key}");
        }

        serde_json::json!({
            "binary": self.binary,
            "allow_npx": self.process.allow_npx,
            "cwd": self.cwd,
            "env": self.env,
            "sanitize_environment": self.process.sanitize_environment,
            "app_server_args": self.app_server_args,
            "request_timeout_ms": duration_millis::to_u64(self.request_timeout),
            "startup_timeout_ms": duration_millis::to_u64(self.startup_timeout),
            "max_in_flight_requests": self.max_in_flight_requests,
            "max_queued_requests": self.max_queued_requests,
            "model_cache_ttl_ms": duration_millis::to_u64(self.model_cache_ttl),
            "approval_mode": approval_mode_serde::to_string(&self.approval_mode),
            "compaction_token_limit": self.compaction_token_limit,
            "model_context_window": self.model_context_window,
            "compact_prompt": self.compact_prompt,
        })
        .to_string()
    }
}

impl Default for CodexProviderConfig {
    fn default() -> Self {
        Self {
            binary: "codex".to_owned(),
            process: CodexProcessConfig::default(),
            shared_app_server_key: None,
            cwd: None,
            add_dirs: Vec::new(),
            env: BTreeMap::new(),
            version_args: vec!["--version".to_owned()],
            app_server_args: vec![
                "app-server".to_owned(),
                "--listen".to_owned(),
                "stdio://".to_owned(),
            ],
            request_timeout: Duration::from_secs(30),
            scheduler_timeout: Duration::from_secs(300),
            startup_timeout: Duration::from_secs(60),
            idle_shutdown_timeout: Duration::from_secs(60),
            model_cache_ttl: Duration::from_secs(300),
            max_in_flight_requests: 8,
            max_queued_requests: 64,
            approval_mode: ApprovalMode::AutoApprove,
            approval_policy: Some("never".to_owned()),
            client_name: "arky-codex".to_owned(),
            client_version: env!("CARGO_PKG_VERSION").to_owned(),
            sandbox: CodexSandboxConfig::default(),
            color: None,
            output_last_message_file: None,
            exec_policy_rules_path: None,
            compaction_token_limit: None,
            model_context_window: None,
            compact_prompt: None,
            system_prompt: None,
            append_system_prompt: None,
            reasoning_effort: Some("medium".to_owned()),
            reasoning_summary: None,
            reasoning_summary_format: None,
            model_verbosity: None,
            workspace: CodexWorkspaceConfig::default(),
            profile: None,
            capability: CodexCapabilityConfig::default(),
            config_overrides: BTreeMap::new(),
        }
    }
}

mod duration_millis {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(to_u64(*duration))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let millis = u64::deserialize(deserializer)?;
        Ok(Duration::from_millis(millis))
    }

    pub fn to_u64(duration: Duration) -> u64 {
        u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
    }
}

mod approval_mode_serde {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use serde_json::Value;

    use crate::ApprovalMode;

    pub fn serialize<S>(mode: &ApprovalMode, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        Value::String(to_string(mode)).serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<ApprovalMode, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match value {
            Value::String(mode) => match mode.as_str() {
                "auto-approve" | "never" => Ok(ApprovalMode::AutoApprove),
                "auto-deny" | "untrusted" => Ok(ApprovalMode::AutoDeny),
                "manual" => Ok(ApprovalMode::Manual {
                    timeout: Duration::from_secs(30),
                }),
                other => Err(serde::de::Error::custom(format!(
                    "unknown approval mode `{other}`"
                ))),
            },
            _ => Err(serde::de::Error::custom("approval mode must be a string")),
        }
    }

    pub fn to_string(mode: &ApprovalMode) -> String {
        match mode {
            ApprovalMode::AutoApprove => "never".to_owned(),
            ApprovalMode::AutoDeny => "untrusted".to_owned(),
            ApprovalMode::Manual { .. } => "manual".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::{CodexCapabilityConfig, CodexProcessConfig, CodexProviderConfig};

    #[test]
    fn config_registry_key_should_ignore_cwd_when_shared_key_is_set() {
        let mut left = CodexProviderConfig {
            shared_app_server_key: Some("shared".to_owned()),
            cwd: Some("/tmp/left".into()),
            ..CodexProviderConfig::default()
        };
        let right = CodexProviderConfig {
            shared_app_server_key: Some("shared".to_owned()),
            cwd: Some("/tmp/right".into()),
            ..CodexProviderConfig::default()
        };
        left.idle_shutdown_timeout = std::time::Duration::from_secs(5);

        assert_eq!(left.registry_key(), right.registry_key());
    }

    #[test]
    fn config_should_round_trip_through_serde() {
        let config = CodexProviderConfig {
            binary: "codex-dev".to_owned(),
            process: CodexProcessConfig {
                experimental_api: true,
                ..CodexProcessConfig::default()
            },
            shared_app_server_key: Some("shared".to_owned()),
            reasoning_effort: Some("high".to_owned()),
            capability: CodexCapabilityConfig {
                mcp_servers: BTreeMap::from_iter([(
                    "local".to_owned(),
                    json!({"transport": "stdio"}),
                )]),
                ..CodexCapabilityConfig::default()
            },
            config_overrides: BTreeMap::from_iter([("model".to_owned(), json!("gpt-5"))]),
            ..CodexProviderConfig::default()
        };

        let value = serde_json::to_value(&config).expect("config should serialize");
        let decoded: CodexProviderConfig =
            serde_json::from_value(value).expect("config should deserialize");

        assert_eq!(decoded, config);
    }
}
