//! Canonical Codex tool payload builders.

use serde_json::{Value, json};

/// Maps provider-specific tool categories onto canonical Arky tool names.
#[must_use]
pub fn canonical_tool_name(tool_name: &str) -> String {
    match tool_name {
        "command_execution" | "commandexecution" => "shell".to_owned(),
        "file_change" | "filechange" => "apply_patch".to_owned(),
        _ => tool_name.to_owned(),
    }
}

/// Builds the canonical input payload for a tool call.
#[must_use]
pub fn build_tool_input_payload(tool_name: &str, payload: &Value) -> Value {
    match canonical_tool_name(tool_name).as_str() {
        "shell" => json!({
            "command": payload
                .get("command")
                .or_else(|| payload.get("cmd"))
                .cloned()
                .unwrap_or(Value::Null),
            "cwd": payload.get("cwd").cloned().unwrap_or(Value::Null),
            "timeout_ms": payload
                .get("timeout_ms")
                .or_else(|| payload.get("timeoutMs"))
                .cloned()
                .unwrap_or(Value::Null),
        }),
        "apply_patch" => json!({
            "path": payload
                .get("path")
                .or_else(|| payload.get("file"))
                .cloned()
                .unwrap_or(Value::Null),
            "patch": payload
                .get("patch")
                .or_else(|| payload.get("diff"))
                .cloned()
                .unwrap_or(Value::Null),
        }),
        _ => payload.clone(),
    }
}

/// Builds the canonical result payload for a completed tool call.
#[must_use]
pub fn build_tool_result_payload(tool_name: &str, payload: &Value) -> Value {
    match canonical_tool_name(tool_name).as_str() {
        "shell" => json!({
            "stdout": payload.get("stdout").cloned().unwrap_or(Value::Null),
            "stderr": payload.get("stderr").cloned().unwrap_or(Value::Null),
            "exitCode": payload
                .get("exitCode")
                .or_else(|| payload.get("exit_code"))
                .cloned()
                .unwrap_or(Value::Null),
            "status": payload.get("status").cloned().unwrap_or(Value::Null),
        }),
        "apply_patch" => json!({
            "status": payload.get("status").cloned().unwrap_or(Value::Null),
            "applied": payload
                .get("applied")
                .or_else(|| payload.get("success"))
                .cloned()
                .unwrap_or(Value::Null),
            "files": payload
                .get("files")
                .or_else(|| payload.get("paths"))
                .cloned()
                .unwrap_or(Value::Null),
            "error": payload.get("error").cloned().unwrap_or(Value::Null),
        }),
        _ => payload.clone(),
    }
}

/// Returns whether a tool payload represents a failed tool execution.
#[must_use]
pub fn payload_has_error(payload: &Value) -> bool {
    payload
        .get("exitCode")
        .and_then(Value::as_i64)
        .is_some_and(|exit_code| exit_code != 0)
        || payload
            .get("exit_code")
            .and_then(Value::as_i64)
            .is_some_and(|exit_code| exit_code != 0)
        || payload
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|status| matches!(status, "error" | "failed" | "denied" | "cancelled"))
        || payload.get("error").is_some()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::{
        build_tool_input_payload, build_tool_result_payload, canonical_tool_name, payload_has_error,
    };

    #[test]
    fn canonical_tool_name_should_map_codex_categories() {
        assert_eq!(canonical_tool_name("command_execution"), "shell");
        assert_eq!(canonical_tool_name("file_change"), "apply_patch");
    }

    #[test]
    fn build_tool_input_payload_should_build_shell_payloads() {
        let payload = build_tool_input_payload(
            "command_execution",
            &json!({
                "command": "pwd",
                "cwd": "/workspace",
            }),
        );

        assert_eq!(payload["command"], "pwd");
        assert_eq!(payload["cwd"], "/workspace");
    }

    #[test]
    fn build_tool_result_payload_should_build_apply_patch_payloads() {
        let payload = build_tool_result_payload(
            "file_change",
            &json!({
                "status": "ok",
                "success": true,
                "paths": ["lib.rs"],
            }),
        );

        assert_eq!(payload["status"], "ok");
        assert_eq!(payload["applied"], true);
        assert_eq!(payload["files"], json!(["lib.rs"]));
    }

    #[test]
    fn payload_has_error_should_detect_common_error_shapes() {
        assert_eq!(payload_has_error(&json!({ "exitCode": 1 })), true);
        assert_eq!(payload_has_error(&json!({ "status": "failed" })), true);
        assert_eq!(payload_has_error(&json!({ "status": "ok" })), false);
    }
}
