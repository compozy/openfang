//! Integration coverage for shell-backed hook execution.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use arky_hooks::{
    BeforeToolCallContext, FailureMode, HookChain, HookError, HookEvent, Hooks,
    PromptSubmitContext, SessionStartContext, SessionStartSource, ShellCommandHook, Verdict,
};
use arky_protocol::{Message, SessionRef, ToolCall};
use pretty_assertions::assert_eq;
use serde_json::Value;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

fn write_script(dir: &TempDir, name: &str, body: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, body).expect("script should be written");
    path
}

fn before_context() -> BeforeToolCallContext {
    BeforeToolCallContext::new(
        SessionRef::default(),
        ToolCall::new(
            "call-1",
            "mcp/local/read_file",
            serde_json::json!({ "path": "Cargo.toml" }),
        ),
    )
}

fn process_is_running(pid: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("kill -0 {pid} 2>/dev/null")])
        .status()
        .expect("kill -0 should run")
        .success()
}

fn read_to_string(path: &Path) -> String {
    fs::read_to_string(path).expect("file should be readable")
}

#[tokio::test]
async fn shell_hook_should_round_trip_json_payload_and_response() {
    let temp_dir = TempDir::new().expect("temp dir should be created");
    let capture_path = temp_dir.path().join("payload.json");
    let script_path = write_script(
        &temp_dir,
        "roundtrip.sh",
        r#"#!/bin/sh
capture_path="$1"
payload="$(cat)"
printf '%s' "$payload" > "$capture_path"
printf '%s\n' '{"env":{"HOOK_ENV":"1"},"settings":{"mode":"review"},"messages":[{"role":"system","content":[{"type":"text","text":"hello from shell"}]}]}'
"#,
    );

    let hook = ShellCommandHook::new(HookEvent::SessionStart, "sh").with_args([
        script_path.to_string_lossy().into_owned(),
        capture_path.to_string_lossy().into_owned(),
    ]);

    let actual = hook
        .session_start(
            &SessionStartContext::new(SessionRef::default(), SessionStartSource::Startup),
            CancellationToken::new(),
        )
        .await
        .expect("session start should succeed")
        .expect("shell hook should return an update");

    assert_eq!(actual.env.get("HOOK_ENV"), Some(&"1".to_owned()));
    assert_eq!(actual.messages, vec![Message::system("hello from shell")],);

    let captured: Value = serde_json::from_str(&read_to_string(&capture_path))
        .expect("captured payload should be valid json");
    assert_eq!(captured["event"], "session_start");
    assert_eq!(captured["input"]["source"], "startup");
}

#[tokio::test]
async fn shell_hook_should_cleanup_process_on_timeout() {
    let temp_dir = TempDir::new().expect("temp dir should be created");
    let pid_path = temp_dir.path().join("timeout.pid");
    let script_path = write_script(
        &temp_dir,
        "timeout.sh",
        r#"#!/bin/sh
pid_path="$1"
echo $$ > "$pid_path"
cat >/dev/null
sleep 30
"#,
    );

    let hook = ShellCommandHook::new(HookEvent::BeforeToolCall, "sh")
        .with_args([
            script_path.to_string_lossy().into_owned(),
            pid_path.to_string_lossy().into_owned(),
        ])
        .with_timeout(Duration::from_millis(100))
        .with_failure_mode(FailureMode::FailClosed);

    let error = hook
        .before_tool_call(&before_context(), CancellationToken::new())
        .await
        .expect_err("timeout should fail");

    assert!(matches!(error, HookError::Timeout { .. }));
    tokio::time::sleep(Duration::from_millis(100)).await;

    let pid = read_to_string(&pid_path);
    assert!(!process_is_running(pid.trim()));
}

#[tokio::test]
async fn shell_hook_should_cleanup_process_on_cancellation() {
    let temp_dir = TempDir::new().expect("temp dir should be created");
    let pid_path = temp_dir.path().join("cancel.pid");
    let script_path = write_script(
        &temp_dir,
        "cancel.sh",
        r#"#!/bin/sh
pid_path="$1"
echo $$ > "$pid_path"
cat >/dev/null
sleep 30
"#,
    );

    let hook = ShellCommandHook::new(HookEvent::BeforeToolCall, "sh")
        .with_args([
            script_path.to_string_lossy().into_owned(),
            pid_path.to_string_lossy().into_owned(),
        ])
        .with_timeout(Duration::from_secs(5))
        .with_failure_mode(FailureMode::FailClosed);

    let token = CancellationToken::new();
    let cancel_token = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        cancel_token.cancel();
    });

    let error = hook
        .before_tool_call(&before_context(), token)
        .await
        .expect_err("cancellation should fail");

    assert!(matches!(error, HookError::ExecutionFailed { .. }));
    tokio::time::sleep(Duration::from_millis(100)).await;

    let pid = read_to_string(&pid_path);
    assert!(!process_is_running(pid.trim()));
}

#[tokio::test]
async fn shell_hook_should_default_to_fail_open_inside_chain() {
    let temp_dir = TempDir::new().expect("temp dir should be created");
    let script_path = write_script(
        &temp_dir,
        "fail.sh",
        r#"#!/bin/sh
echo "hook failed" >&2
exit 1
"#,
    );

    let chain = HookChain::new()
        .with_failure_mode(FailureMode::FailClosed)
        .with_hook(
            ShellCommandHook::new(HookEvent::BeforeToolCall, "sh")
                .with_args([script_path.to_string_lossy().into_owned()]),
        );

    let verdict = chain
        .before_tool_call(&before_context(), CancellationToken::new())
        .await
        .expect("shell hook should fail open by default");

    assert_eq!(verdict, Verdict::Allow);
    assert_eq!(chain.diagnostics().len(), 1);
}

#[tokio::test]
async fn shell_hook_should_map_plain_text_prompt_output_to_message_injection() {
    let temp_dir = TempDir::new().expect("temp dir should be created");
    let script_path = write_script(
        &temp_dir,
        "plain.sh",
        r"#!/bin/sh
cat >/dev/null
printf '%s\n' 'plain shell note'
",
    );

    let hook = ShellCommandHook::new(HookEvent::UserPromptSubmit, "sh")
        .with_args([script_path.to_string_lossy().into_owned()]);

    let update = hook
        .user_prompt_submit(
            &PromptSubmitContext::new(SessionRef::default(), "hello"),
            CancellationToken::new(),
        )
        .await
        .expect("prompt hook should succeed")
        .expect("plain text should become an update");

    assert_eq!(update.messages, vec![Message::system("plain shell note")],);
}
