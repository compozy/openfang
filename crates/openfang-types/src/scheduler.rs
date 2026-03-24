//! Cron/scheduled job types for the OpenFang scheduler.
//!
//! Defines the core types for recurring and one-shot scheduled jobs that can
//! trigger agent turns, system events, or webhook deliveries.

use crate::agent::AgentId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

/// Maximum number of scheduled jobs per agent.
pub const MAX_JOBS_PER_AGENT: usize = 50;

/// Maximum name length in characters.
const MAX_NAME_LEN: usize = 128;

/// Minimum interval for recurring jobs (seconds).
const MIN_EVERY_SECS: u64 = 60;

/// Maximum interval for recurring jobs (seconds) = 24 hours.
const MAX_EVERY_SECS: u64 = 86_400;

/// Maximum future horizon for one-shot `At` jobs (seconds) = 1 year.
const MAX_AT_HORIZON_SECS: i64 = 365 * 24 * 3600;

/// Maximum length of SystemEvent text.
const MAX_EVENT_TEXT_LEN: usize = 4096;

/// Maximum length of AgentTurn message.
const MAX_TURN_MESSAGE_LEN: usize = 16_384;

/// Minimum timeout for AgentTurn (seconds).
const MIN_TIMEOUT_SECS: u64 = 10;

/// Maximum timeout for AgentTurn (seconds).
const MAX_TIMEOUT_SECS: u64 = 600;

/// Maximum webhook URL length.
const MAX_WEBHOOK_URL_LEN: usize = 2048;

/// Maximum serialized JSON payload length for schedule actions.
const MAX_ACTION_PAYLOAD_LEN: usize = 16_384;

/// Stable origin metadata for persisted schedule definitions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CronDefinitionOriginKind {
    /// User-authored schedule definition.
    User,
    /// Pack-managed schedule definition.
    Pack,
}

/// Origin metadata stored with a schedule definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct CronDefinitionOrigin {
    pub kind: CronDefinitionOriginKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pack_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pack_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

impl CronDefinitionOrigin {
    /// Build user-owned origin metadata.
    #[must_use]
    pub fn user() -> Self {
        Self {
            kind: CronDefinitionOriginKind::User,
            pack_id: None,
            pack_version: None,
            source: None,
        }
    }
}

/// Provenance metadata for a forked schedule definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct CronDefinitionForkedFrom {
    pub kind: CronDefinitionOriginKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pack_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pack_version: Option<String>,
    pub resource_type: String,
    pub resource_id: String,
}

/// Structured text input for scheduled agent turns.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct CronTextInputPayload {
    #[serde(default)]
    pub items: Vec<CronTextInputItem>,
}

/// One structured text input item for a scheduled agent turn.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct CronTextInputItem {
    #[serde(rename = "type")]
    pub item_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// Destination selector for scheduled workflow signals.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct CronWorkflowSignalSelector {
    pub workflow_id: String,
}

// ---------------------------------------------------------------------------
// CronJobId
// ---------------------------------------------------------------------------

/// Unique identifier for a scheduled job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CronJobId(pub Uuid);

impl CronJobId {
    /// Generate a new random CronJobId.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for CronJobId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for CronJobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for CronJobId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

// ---------------------------------------------------------------------------
// CronSchedule
// ---------------------------------------------------------------------------

/// When a scheduled job fires.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CronSchedule {
    /// Fire once at a specific time.
    At {
        /// The exact UTC time to fire.
        at: DateTime<Utc>,
    },
    /// Fire on a fixed interval.
    Every {
        /// Interval in seconds (60..=86400).
        every_secs: u64,
    },
    /// Fire on a cron expression (5-field standard cron).
    Cron {
        /// Cron expression, e.g. `"0 9 * * 1-5"`.
        expr: String,
        /// Optional IANA timezone (e.g. `"America/New_York"`). Defaults to UTC.
        tz: Option<String>,
    },
}

// ---------------------------------------------------------------------------
// CronAction
// ---------------------------------------------------------------------------

/// What a scheduled job does when it fires.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CronAction {
    /// Publish a system event.
    SystemEvent {
        /// Event name published through the event bus.
        #[serde(alias = "text")]
        event: String,
        /// Structured JSON payload carried with the event.
        #[serde(default, skip_serializing_if = "JsonValue::is_null")]
        payload: JsonValue,
    },
    /// Trigger an agent conversation turn.
    AgentTurn {
        /// Legacy single-message text preserved for backwards compatibility.
        #[serde(default, skip_serializing)]
        message: Option<String>,
        /// Structured text input delivered to the agent runtime.
        #[serde(default)]
        input: CronTextInputPayload,
        /// Optional model override for this turn.
        model_override: Option<String>,
        /// Timeout in seconds (10..=600).
        timeout_secs: Option<u64>,
    },
    /// Run a workflow by ID or name.
    WorkflowRun {
        /// Workflow UUID or name (resolved by name if not a valid UUID).
        workflow_id: String,
        /// Initial input to the workflow (default: empty).
        input: Option<JsonValue>,
        /// Timeout in seconds (10..=3600, default: 120).
        timeout_secs: Option<u64>,
    },
    /// Submit a durable workflow signal to waiting runs of one definition.
    WorkflowSignal {
        /// Signal name expected by the waiting workflow step.
        signal: String,
        /// Target workflow definition selector.
        selector: CronWorkflowSignalSelector,
        /// Structured signal payload.
        #[serde(default, skip_serializing_if = "JsonValue::is_null")]
        payload: JsonValue,
    },
}

// ---------------------------------------------------------------------------
// CronDelivery
// ---------------------------------------------------------------------------

/// Where the job's output is delivered.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CronDelivery {
    /// No delivery — fire and forget.
    None,
    /// Deliver to a specific channel and recipient.
    Channel {
        /// Channel identifier (e.g. `"telegram"`, `"slack"`).
        channel: String,
        /// Recipient in the channel.
        to: String,
    },
    /// Deliver to the last channel the agent interacted on.
    LastChannel,
    /// Deliver via HTTP webhook.
    Webhook {
        /// Webhook URL (must start with `http://` or `https://`).
        url: String,
    },
}

// ---------------------------------------------------------------------------
// CronJob
// ---------------------------------------------------------------------------

/// A scheduled job belonging to a specific agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    /// Unique job identifier.
    pub id: CronJobId,
    /// Owning agent.
    pub agent_id: AgentId,
    /// Human-readable name (max 128 chars, alphanumeric + spaces/hyphens/underscores).
    pub name: String,
    /// Whether the job is active.
    pub enabled: bool,
    /// When to fire.
    pub schedule: CronSchedule,
    /// What to do when fired.
    pub action: CronAction,
    /// Where to deliver the result.
    pub delivery: CronDelivery,
    /// When the job was created.
    pub created_at: DateTime<Utc>,
    /// When the job last fired (if ever).
    pub last_run: Option<DateTime<Utc>>,
    /// When the job is next expected to fire.
    pub next_run: Option<DateTime<Utc>>,
}

impl CronJob {
    /// Validate this job's fields.
    ///
    /// `existing_count` is the number of jobs the owning agent already has
    /// (excluding this job if it already exists). Returns `Ok(())` or an
    /// error message describing the first validation failure.
    pub fn validate(&self, existing_count: usize) -> Result<(), String> {
        // -- job count cap --
        if existing_count >= MAX_JOBS_PER_AGENT {
            return Err(format!(
                "agent already has {existing_count} jobs (max {MAX_JOBS_PER_AGENT})"
            ));
        }

        // -- name --
        if self.name.is_empty() {
            return Err("name must not be empty".into());
        }
        if self.name.len() > MAX_NAME_LEN {
            return Err(format!(
                "name too long ({} chars, max {MAX_NAME_LEN})",
                self.name.len()
            ));
        }
        if !self
            .name
            .chars()
            .all(|c| c.is_alphanumeric() || c == ' ' || c == '-' || c == '_')
        {
            return Err(
                "name may only contain alphanumeric characters, spaces, hyphens, and underscores"
                    .into(),
            );
        }

        // -- schedule --
        self.validate_schedule()?;

        // -- action --
        self.validate_action()?;

        // -- delivery --
        self.validate_delivery()?;

        Ok(())
    }

    fn validate_schedule(&self) -> Result<(), String> {
        match &self.schedule {
            CronSchedule::Every { every_secs } => {
                if *every_secs < MIN_EVERY_SECS {
                    return Err(format!(
                        "every_secs too small ({every_secs}, min {MIN_EVERY_SECS})"
                    ));
                }
                if *every_secs > MAX_EVERY_SECS {
                    return Err(format!(
                        "every_secs too large ({every_secs}, max {MAX_EVERY_SECS})"
                    ));
                }
            }
            CronSchedule::At { at } => {
                let now = Utc::now();
                if *at <= now {
                    return Err("scheduled time must be in the future".into());
                }
                let delta = (*at - now).num_seconds();
                if delta > MAX_AT_HORIZON_SECS {
                    return Err(format!(
                        "scheduled time too far in the future (max {MAX_AT_HORIZON_SECS}s / ~1 year)"
                    ));
                }
            }
            CronSchedule::Cron { expr, tz } => {
                validate_cron_expr(expr)?;
                if let Some(tz) = tz {
                    if !tz.trim().is_empty() && tz.parse::<chrono_tz::Tz>().is_err() {
                        return Err(format!("invalid timezone: {tz}"));
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_action(&self) -> Result<(), String> {
        match &self.action {
            CronAction::SystemEvent { event, payload } => {
                if event.trim().is_empty() {
                    return Err("system event name must not be empty".into());
                }
                if event.len() > MAX_EVENT_TEXT_LEN {
                    return Err(format!(
                        "system event name too long ({} chars, max {MAX_EVENT_TEXT_LEN})",
                        event.len()
                    ));
                }
                validate_json_payload_len(payload, "system event payload")?;
            }
            CronAction::AgentTurn {
                message,
                input,
                timeout_secs,
                ..
            } => {
                let has_structured_input = !input.items.is_empty();
                let has_legacy_message = message.as_deref().is_some_and(|value| !value.is_empty());

                if !has_structured_input && !has_legacy_message {
                    return Err("agent turn input must not be empty".into());
                }

                if let Some(message) = message {
                    if message.len() > MAX_TURN_MESSAGE_LEN {
                        return Err(format!(
                            "agent turn message too long ({} chars, max {MAX_TURN_MESSAGE_LEN})",
                            message.len()
                        ));
                    }
                }

                for (index, item) in input.items.iter().enumerate() {
                    if item.item_type != "text" {
                        return Err(format!(
                            "agent turn input item {index} has unsupported type '{}'",
                            item.item_type
                        ));
                    }
                    let Some(text) = item.text.as_deref() else {
                        return Err(format!("agent turn input item {index} must include text"));
                    };
                    if text.is_empty() {
                        return Err(format!("agent turn input item {index} must not be empty"));
                    }
                    if text.len() > MAX_TURN_MESSAGE_LEN {
                        return Err(format!(
                            "agent turn input item {index} too long ({} chars, max {MAX_TURN_MESSAGE_LEN})",
                            text.len()
                        ));
                    }
                }
                if let Some(t) = timeout_secs {
                    if *t < MIN_TIMEOUT_SECS {
                        return Err(format!(
                            "timeout_secs too small ({t}, min {MIN_TIMEOUT_SECS})"
                        ));
                    }
                    if *t > MAX_TIMEOUT_SECS {
                        return Err(format!(
                            "timeout_secs too large ({t}, max {MAX_TIMEOUT_SECS})"
                        ));
                    }
                }
            }
            CronAction::WorkflowRun {
                workflow_id,
                input,
                timeout_secs,
            } => {
                if workflow_id.trim().is_empty() {
                    return Err("workflow_id must not be empty".into());
                }
                if let Some(i) = input {
                    validate_json_payload_len(i, "workflow input")?;
                }
                if let Some(t) = timeout_secs {
                    if *t < MIN_TIMEOUT_SECS {
                        return Err(format!(
                            "timeout_secs too small ({t}, min {MIN_TIMEOUT_SECS})"
                        ));
                    }
                    // Workflows can run longer than agent turns (max 3600s = 1h)
                    if *t > 3600 {
                        return Err(format!("timeout_secs too large ({t}, max 3600)"));
                    }
                }
            }
            CronAction::WorkflowSignal {
                signal,
                selector,
                payload,
            } => {
                if signal.trim().is_empty() {
                    return Err("workflow signal name must not be empty".into());
                }
                if selector.workflow_id.trim().is_empty() {
                    return Err("workflow signal selector.workflow_id must not be empty".into());
                }
                validate_json_payload_len(payload, "workflow signal payload")?;
            }
        }
        Ok(())
    }

    fn validate_delivery(&self) -> Result<(), String> {
        match &self.delivery {
            CronDelivery::Channel { channel, to } => {
                if channel.is_empty() {
                    return Err("delivery channel must not be empty".into());
                }
                if to.is_empty() {
                    return Err("delivery recipient must not be empty".into());
                }
            }
            CronDelivery::Webhook { url } => {
                if !url.starts_with("http://") && !url.starts_with("https://") {
                    return Err("webhook URL must start with http:// or https://".into());
                }
                if url.len() > MAX_WEBHOOK_URL_LEN {
                    return Err(format!(
                        "webhook URL too long ({} chars, max {MAX_WEBHOOK_URL_LEN})",
                        url.len()
                    ));
                }
            }
            CronDelivery::None | CronDelivery::LastChannel => {}
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Cron expression basic format validation
// ---------------------------------------------------------------------------

/// Basic cron expression validation for standard 5-field expressions.
fn validate_cron_expr(expr: &str) -> Result<(), String> {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return Err("cron expression must not be empty".into());
    }
    let fields: Vec<&str> = trimmed.split_whitespace().collect();
    if fields.len() != 5 {
        return Err(format!(
            "cron expression must have exactly 5 fields (got {}): \"{}\"",
            fields.len(),
            trimmed
        ));
    }
    // Basic character validation per field — allow digits, *, /, -, and ,.
    for (i, field) in fields.iter().enumerate() {
        if field.is_empty() {
            return Err(format!("cron field {i} is empty"));
        }
        if !field
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '*' | '/' | '-' | ',' | '?'))
        {
            return Err(format!(
                "cron field {i} contains invalid characters: \"{field}\""
            ));
        }
    }

    let seven_field = format!("0 {trimmed} *");
    seven_field
        .parse::<cron::Schedule>()
        .map_err(|error| format!("invalid cron expression: {error}"))?;
    Ok(())
}

fn validate_json_payload_len(payload: &JsonValue, label: &str) -> Result<(), String> {
    let serialized = serde_json::to_string(payload)
        .map_err(|error| format!("failed to serialize {label}: {error}"))?;
    if serialized.len() > MAX_ACTION_PAYLOAD_LEN {
        return Err(format!(
            "{label} too long ({} chars, max {MAX_ACTION_PAYLOAD_LEN})",
            serialized.len()
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn text_input(text: &str) -> CronTextInputPayload {
        CronTextInputPayload {
            items: vec![CronTextInputItem {
                item_type: "text".to_string(),
                text: Some(text.to_string()),
            }],
        }
    }

    /// Helper: build a minimal valid CronJob.
    fn valid_job() -> CronJob {
        CronJob {
            id: CronJobId::new(),
            agent_id: AgentId::new(),
            name: "daily-report".into(),
            enabled: true,
            schedule: CronSchedule::Every { every_secs: 3600 },
            action: CronAction::SystemEvent {
                event: "daily.report".into(),
                payload: JsonValue::Null,
            },
            delivery: CronDelivery::None,
            created_at: Utc::now(),
            last_run: None,
            next_run: None,
        }
    }

    // -- CronJobId --

    #[test]
    fn cron_job_id_display_roundtrip() {
        let id = CronJobId::new();
        let s = id.to_string();
        let parsed: CronJobId = s.parse().unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn cron_job_id_default() {
        let a = CronJobId::default();
        let b = CronJobId::default();
        assert_ne!(a, b);
    }

    // -- Valid job --

    #[test]
    fn valid_job_passes() {
        assert!(valid_job().validate(0).is_ok());
    }

    // -- Name validation --

    #[test]
    fn empty_name_rejected() {
        let mut job = valid_job();
        job.name = String::new();
        let err = job.validate(0).unwrap_err();
        assert!(err.contains("empty"), "{err}");
    }

    #[test]
    fn long_name_rejected() {
        let mut job = valid_job();
        job.name = "a".repeat(129);
        let err = job.validate(0).unwrap_err();
        assert!(err.contains("too long"), "{err}");
    }

    #[test]
    fn name_128_chars_ok() {
        let mut job = valid_job();
        job.name = "a".repeat(128);
        assert!(job.validate(0).is_ok());
    }

    #[test]
    fn name_special_chars_rejected() {
        let mut job = valid_job();
        job.name = "my job!".into();
        let err = job.validate(0).unwrap_err();
        assert!(err.contains("alphanumeric"), "{err}");
    }

    #[test]
    fn name_with_spaces_hyphens_underscores_ok() {
        let mut job = valid_job();
        job.name = "My Daily-Report_v2".into();
        assert!(job.validate(0).is_ok());
    }

    // -- Job count cap --

    #[test]
    fn max_jobs_rejected() {
        let job = valid_job();
        let err = job.validate(50).unwrap_err();
        assert!(err.contains("50"), "{err}");
    }

    #[test]
    fn under_max_jobs_ok() {
        let job = valid_job();
        assert!(job.validate(49).is_ok());
    }

    // -- Schedule: Every --

    #[test]
    fn every_too_small() {
        let mut job = valid_job();
        job.schedule = CronSchedule::Every { every_secs: 59 };
        let err = job.validate(0).unwrap_err();
        assert!(err.contains("too small"), "{err}");
    }

    #[test]
    fn every_too_large() {
        let mut job = valid_job();
        job.schedule = CronSchedule::Every { every_secs: 86_401 };
        let err = job.validate(0).unwrap_err();
        assert!(err.contains("too large"), "{err}");
    }

    #[test]
    fn every_min_boundary_ok() {
        let mut job = valid_job();
        job.schedule = CronSchedule::Every { every_secs: 60 };
        assert!(job.validate(0).is_ok());
    }

    #[test]
    fn every_max_boundary_ok() {
        let mut job = valid_job();
        job.schedule = CronSchedule::Every { every_secs: 86_400 };
        assert!(job.validate(0).is_ok());
    }

    // -- Schedule: At --

    #[test]
    fn at_in_past_rejected() {
        let mut job = valid_job();
        job.schedule = CronSchedule::At {
            at: Utc::now() - Duration::seconds(10),
        };
        let err = job.validate(0).unwrap_err();
        assert!(err.contains("future"), "{err}");
    }

    #[test]
    fn at_too_far_future_rejected() {
        let mut job = valid_job();
        job.schedule = CronSchedule::At {
            at: Utc::now() + Duration::days(366),
        };
        let err = job.validate(0).unwrap_err();
        assert!(err.contains("too far"), "{err}");
    }

    #[test]
    fn at_near_future_ok() {
        let mut job = valid_job();
        job.schedule = CronSchedule::At {
            at: Utc::now() + Duration::hours(1),
        };
        assert!(job.validate(0).is_ok());
    }

    // -- Schedule: Cron --

    #[test]
    fn cron_valid_expr() {
        let mut job = valid_job();
        job.schedule = CronSchedule::Cron {
            expr: "0 9 * * 1-5".into(),
            tz: Some("America/New_York".into()),
        };
        assert!(job.validate(0).is_ok());
    }

    #[test]
    fn cron_empty_expr() {
        let mut job = valid_job();
        job.schedule = CronSchedule::Cron {
            expr: String::new(),
            tz: None,
        };
        let err = job.validate(0).unwrap_err();
        assert!(err.contains("empty"), "{err}");
    }

    #[test]
    fn cron_wrong_field_count() {
        let mut job = valid_job();
        job.schedule = CronSchedule::Cron {
            expr: "0 9 * *".into(),
            tz: None,
        };
        let err = job.validate(0).unwrap_err();
        assert!(err.contains("5 fields"), "{err}");
    }

    #[test]
    fn cron_invalid_chars() {
        let mut job = valid_job();
        job.schedule = CronSchedule::Cron {
            expr: "0 9 * * MON".into(),
            tz: None,
        };
        let err = job.validate(0).unwrap_err();
        assert!(err.contains("invalid characters"), "{err}");
    }

    // -- Action: SystemEvent --

    #[test]
    fn system_event_empty_text() {
        let mut job = valid_job();
        job.action = CronAction::SystemEvent {
            event: String::new(),
            payload: JsonValue::Null,
        };
        let err = job.validate(0).unwrap_err();
        assert!(err.contains("empty"), "{err}");
    }

    #[test]
    fn system_event_text_too_long() {
        let mut job = valid_job();
        job.action = CronAction::SystemEvent {
            event: "x".repeat(4097),
            payload: JsonValue::Null,
        };
        let err = job.validate(0).unwrap_err();
        assert!(err.contains("too long"), "{err}");
    }

    #[test]
    fn system_event_max_text_ok() {
        let mut job = valid_job();
        job.action = CronAction::SystemEvent {
            event: "x".repeat(4096),
            payload: JsonValue::Null,
        };
        assert!(job.validate(0).is_ok());
    }

    // -- Action: AgentTurn --

    #[test]
    fn agent_turn_empty_message() {
        let mut job = valid_job();
        job.action = CronAction::AgentTurn {
            message: Some(String::new()),
            input: CronTextInputPayload::default(),
            model_override: None,
            timeout_secs: None,
        };
        let err = job.validate(0).unwrap_err();
        assert!(err.contains("empty"), "{err}");
    }

    #[test]
    fn agent_turn_message_too_long() {
        let mut job = valid_job();
        job.action = CronAction::AgentTurn {
            message: Some("x".repeat(16_385)),
            input: CronTextInputPayload::default(),
            model_override: None,
            timeout_secs: None,
        };
        let err = job.validate(0).unwrap_err();
        assert!(err.contains("too long"), "{err}");
    }

    #[test]
    fn agent_turn_timeout_too_small() {
        let mut job = valid_job();
        job.action = CronAction::AgentTurn {
            message: None,
            input: text_input("hello"),
            model_override: None,
            timeout_secs: Some(9),
        };
        let err = job.validate(0).unwrap_err();
        assert!(err.contains("too small"), "{err}");
    }

    #[test]
    fn agent_turn_timeout_too_large() {
        let mut job = valid_job();
        job.action = CronAction::AgentTurn {
            message: None,
            input: text_input("hello"),
            model_override: None,
            timeout_secs: Some(601),
        };
        let err = job.validate(0).unwrap_err();
        assert!(err.contains("too large"), "{err}");
    }

    #[test]
    fn agent_turn_timeout_boundaries_ok() {
        let mut job = valid_job();
        job.action = CronAction::AgentTurn {
            message: None,
            input: text_input("hello"),
            model_override: Some("claude-haiku-4-5-20251001".into()),
            timeout_secs: Some(10),
        };
        assert!(job.validate(0).is_ok());

        job.action = CronAction::AgentTurn {
            message: None,
            input: text_input("hello"),
            model_override: None,
            timeout_secs: Some(600),
        };
        assert!(job.validate(0).is_ok());
    }

    #[test]
    fn agent_turn_no_timeout_ok() {
        let mut job = valid_job();
        job.action = CronAction::AgentTurn {
            message: None,
            input: text_input("hello"),
            model_override: None,
            timeout_secs: None,
        };
        assert!(job.validate(0).is_ok());
    }

    // -- Delivery: Channel --

    #[test]
    fn delivery_channel_empty_channel() {
        let mut job = valid_job();
        job.delivery = CronDelivery::Channel {
            channel: String::new(),
            to: "user123".into(),
        };
        let err = job.validate(0).unwrap_err();
        assert!(err.contains("channel must not be empty"), "{err}");
    }

    #[test]
    fn delivery_channel_empty_to() {
        let mut job = valid_job();
        job.delivery = CronDelivery::Channel {
            channel: "slack".into(),
            to: String::new(),
        };
        let err = job.validate(0).unwrap_err();
        assert!(err.contains("recipient must not be empty"), "{err}");
    }

    #[test]
    fn delivery_channel_ok() {
        let mut job = valid_job();
        job.delivery = CronDelivery::Channel {
            channel: "telegram".into(),
            to: "chat_12345".into(),
        };
        assert!(job.validate(0).is_ok());
    }

    // -- Delivery: Webhook --

    #[test]
    fn webhook_bad_scheme() {
        let mut job = valid_job();
        job.delivery = CronDelivery::Webhook {
            url: "ftp://example.com/hook".into(),
        };
        let err = job.validate(0).unwrap_err();
        assert!(err.contains("http://"), "{err}");
    }

    #[test]
    fn webhook_too_long() {
        let mut job = valid_job();
        job.delivery = CronDelivery::Webhook {
            url: format!("https://example.com/{}", "a".repeat(2048)),
        };
        let err = job.validate(0).unwrap_err();
        assert!(err.contains("too long"), "{err}");
    }

    #[test]
    fn webhook_http_ok() {
        let mut job = valid_job();
        job.delivery = CronDelivery::Webhook {
            url: "http://localhost:8080/hook".into(),
        };
        assert!(job.validate(0).is_ok());
    }

    #[test]
    fn webhook_https_ok() {
        let mut job = valid_job();
        job.delivery = CronDelivery::Webhook {
            url: "https://example.com/hook".into(),
        };
        assert!(job.validate(0).is_ok());
    }

    // -- Delivery: None / LastChannel --

    #[test]
    fn delivery_none_ok() {
        let mut job = valid_job();
        job.delivery = CronDelivery::None;
        assert!(job.validate(0).is_ok());
    }

    #[test]
    fn delivery_last_channel_ok() {
        let mut job = valid_job();
        job.delivery = CronDelivery::LastChannel;
        assert!(job.validate(0).is_ok());
    }

    // -- Serde roundtrip --

    #[test]
    fn serde_roundtrip_every() {
        let job = valid_job();
        let json = serde_json::to_string(&job).unwrap();
        let back: CronJob = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, job.name);
        assert_eq!(back.id, job.id);
    }

    #[test]
    fn serde_roundtrip_cron_schedule() {
        let schedule = CronSchedule::Cron {
            expr: "*/5 * * * *".into(),
            tz: Some("UTC".into()),
        };
        let json = serde_json::to_string(&schedule).unwrap();
        assert!(json.contains("\"kind\":\"cron\""));
        let back: CronSchedule = serde_json::from_str(&json).unwrap();
        if let CronSchedule::Cron { expr, tz } = back {
            assert_eq!(expr, "*/5 * * * *");
            assert_eq!(tz, Some("UTC".into()));
        } else {
            panic!("expected Cron variant");
        }
    }

    #[test]
    fn serde_action_tags() {
        let action = CronAction::AgentTurn {
            message: None,
            input: text_input("hi"),
            model_override: None,
            timeout_secs: Some(30),
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"kind\":\"agent_turn\""));
    }

    #[test]
    fn serde_delivery_tags() {
        let d = CronDelivery::LastChannel;
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("\"kind\":\"last_channel\""));

        let d2 = CronDelivery::Webhook {
            url: "https://x.com".into(),
        };
        let json2 = serde_json::to_string(&d2).unwrap();
        assert!(json2.contains("\"kind\":\"webhook\""));
    }

    // -- Cron expression edge cases --

    #[test]
    fn cron_extra_whitespace_ok() {
        let mut job = valid_job();
        job.schedule = CronSchedule::Cron {
            expr: "  0  9  *  *  *  ".into(),
            tz: None,
        };
        assert!(job.validate(0).is_ok());
    }

    #[test]
    fn cron_six_fields_rejected() {
        let mut job = valid_job();
        job.schedule = CronSchedule::Cron {
            expr: "0 0 9 * * 1".into(),
            tz: None,
        };
        let err = job.validate(0).unwrap_err();
        assert!(err.contains("5 fields"), "{err}");
    }

    #[test]
    fn cron_slash_and_comma_ok() {
        let mut job = valid_job();
        job.schedule = CronSchedule::Cron {
            expr: "*/15 0,12 1-15 * 1,3,5".into(),
            tz: None,
        };
        assert!(job.validate(0).is_ok());
    }

    // -- Action: WorkflowRun --

    #[test]
    fn workflow_run_valid() {
        let mut job = valid_job();
        job.action = CronAction::WorkflowRun {
            workflow_id: "my-report-pipeline".into(),
            input: Some(serde_json::json!({
                "prompt": "generate daily metrics",
            })),
            timeout_secs: Some(300),
        };
        assert!(job.validate(0).is_ok());
    }

    #[test]
    fn workflow_run_empty_id() {
        let mut job = valid_job();
        job.action = CronAction::WorkflowRun {
            workflow_id: String::new(),
            input: None,
            timeout_secs: None,
        };
        let err = job.validate(0).unwrap_err();
        assert!(err.contains("workflow_id"), "{err}");
    }

    #[test]
    fn workflow_run_input_too_long() {
        let mut job = valid_job();
        job.action = CronAction::WorkflowRun {
            workflow_id: "test".into(),
            input: Some(JsonValue::String("x".repeat(16_385))),
            timeout_secs: None,
        };
        let err = job.validate(0).unwrap_err();
        assert!(err.contains("too long"), "{err}");
    }

    #[test]
    fn workflow_run_timeout_too_small() {
        let mut job = valid_job();
        job.action = CronAction::WorkflowRun {
            workflow_id: "test".into(),
            input: None,
            timeout_secs: Some(9),
        };
        let err = job.validate(0).unwrap_err();
        assert!(err.contains("too small"), "{err}");
    }

    #[test]
    fn workflow_run_timeout_too_large() {
        let mut job = valid_job();
        job.action = CronAction::WorkflowRun {
            workflow_id: "test".into(),
            input: None,
            timeout_secs: Some(3601),
        };
        let err = job.validate(0).unwrap_err();
        assert!(err.contains("too large"), "{err}");
    }

    #[test]
    fn workflow_run_max_timeout_ok() {
        let mut job = valid_job();
        job.action = CronAction::WorkflowRun {
            workflow_id: "test".into(),
            input: None,
            timeout_secs: Some(3600),
        };
        assert!(job.validate(0).is_ok());
    }

    #[test]
    fn workflow_run_no_input_ok() {
        let mut job = valid_job();
        job.action = CronAction::WorkflowRun {
            workflow_id: "550e8400-e29b-41d4-a716-446655440000".into(),
            input: None,
            timeout_secs: None,
        };
        assert!(job.validate(0).is_ok());
    }

    #[test]
    fn serde_workflow_run_tag() {
        let action = CronAction::WorkflowRun {
            workflow_id: "my-wf".into(),
            input: Some(serde_json::json!({
                "text": "go",
            })),
            timeout_secs: Some(60),
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"kind\":\"workflow_run\""));
        let back: CronAction = serde_json::from_str(&json).unwrap();
        if let CronAction::WorkflowRun { workflow_id, .. } = back {
            assert_eq!(workflow_id, "my-wf");
        } else {
            panic!("expected WorkflowRun variant");
        }
    }
}
