//! Integration coverage for provider layering, profile reuse, and agent boundaries.

use std::fs;

use arky_config::{ConfigError, ConfigLoader, ResolvedProviderBehaviorConfig};
use arky_protocol::ReasoningEffort;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::tempdir;

#[test]
fn layered_config_should_resolve_workspace_profile_and_agent_overrides() {
    let directory = tempdir().expect("temp directory should be created");
    let path = directory.path().join("layered.toml");
    fs::write(
        &path,
        r#"
            [workspace]
            default_provider = "default"

            [providers.default]
            driver = "codex"
            binary = "cargo"
            model = "gpt-5"
            shared_app_server_key = "shared"

            [profiles.fast]
            driver = "codex"
            model = "gpt-5-mini"

            [profiles.fast.defaults]
            max_tokens = 900
            reasoning_effort = "medium"

            [profiles.fast.config.codex]
            include_plan_tool = true
            web_search = true

            [agents.writer]
            provider = "default"
            profile = "fast"
            model = "gpt-5-high"

            [agents.writer.defaults]
            max_tokens = 1200

            [agents.writer.config.codex]
            resume_last = true
            model_verbosity = "high"

            [agents.writer.request_extra]
            tool_choice = "required"
        "#,
    )
    .expect("config file should be written");

    let config = ConfigLoader::from_path(&path)
        .load()
        .expect("layered config should load");
    let resolved = config
        .resolve_agent_provider("writer")
        .expect("writer provider should resolve");

    assert_eq!(resolved.driver, "codex");
    assert_eq!(resolved.profile.as_deref(), Some("fast"));
    assert_eq!(resolved.model.as_deref(), Some("gpt-5-high"));
    assert_eq!(resolved.defaults.max_tokens, Some(1_200));
    assert_eq!(
        resolved.defaults.reasoning_effort,
        Some(ReasoningEffort::Medium)
    );
    assert_eq!(resolved.install.shared_app_server_key(), Some("shared"));
    assert_eq!(
        resolved.request_extra.get("tool_choice"),
        Some(&json!("required"))
    );

    match resolved.config.expect("codex config should resolve") {
        ResolvedProviderBehaviorConfig::Codex(config) => {
            assert!(config.workspace.include_plan_tool);
            assert!(config.workspace.resume_last);
            assert!(config.web_search);
            assert_eq!(config.model_verbosity.as_deref(), Some("high"));
        }
        other => panic!("expected codex config, got {other:?}"),
    }
}

#[test]
fn unsupported_provider_layer_combinations_should_fail_clearly() {
    let directory = tempdir().expect("temp directory should be created");
    let path = directory.path().join("invalid-layering.toml");
    fs::write(
        &path,
        r#"
            [providers.default]
            driver = "codex"

            [profiles.fast]
            driver = "codex"

            [profiles.fast.config.claude_code]
            continue_conversation = true
        "#,
    )
    .expect("config file should be written");

    let error = ConfigLoader::from_path(&path)
        .load()
        .expect_err("mismatched typed config should fail");

    let actual = match error {
        ConfigError::ValidationFailed { issues, .. } => issues
            .iter()
            .map(|issue| (issue.field().to_owned(), issue.message().to_owned()))
            .collect::<Vec<_>>(),
        other => panic!("expected validation error, got {other:?}"),
    };

    let expected = vec![(
        "profiles.fast.config.claude_code".to_owned(),
        "is not supported for driver `codex`; use `profiles.fast.config.codex`".to_owned(),
    )];

    assert_eq!(actual, expected);
}

#[test]
fn install_level_provider_fields_should_be_rejected_in_agent_docs() {
    let directory = tempdir().expect("temp directory should be created");
    let path = directory.path().join("invalid-agent-field.toml");
    fs::write(
        &path,
        r#"
            [providers.default]
            driver = "codex"

            [agents.writer]
            provider = "default"
            shared_app_server_key = "leak"
        "#,
    )
    .expect("config file should be written");

    let error = ConfigLoader::from_path(&path)
        .load()
        .expect_err("agent-level install field should fail");

    let actual = match error {
        ConfigError::ParseFailed {
            message, format, ..
        } => (message, format),
        other => panic!("expected parse error, got {other:?}"),
    };

    assert!(actual.0.contains("shared_app_server_key"));
    assert_eq!(actual.1, Some("toml"));
}

#[test]
fn typed_provider_fields_should_not_flatten_into_agent_top_level() {
    let directory = tempdir().expect("temp directory should be created");
    let path = directory.path().join("invalid-agent-typed-field.toml");
    fs::write(
        &path,
        r#"
            [providers.default]
            driver = "codex"

            [agents.writer]
            provider = "default"
            resume_last = true
        "#,
    )
    .expect("config file should be written");

    let error = ConfigLoader::from_path(&path)
        .load()
        .expect_err("flattened typed field should fail");

    let actual = match error {
        ConfigError::ParseFailed {
            message, format, ..
        } => (message, format),
        other => panic!("expected parse error, got {other:?}"),
    };

    assert!(actual.0.contains("resume_last"));
    assert_eq!(actual.1, Some("toml"));
}

#[test]
fn legacy_provider_setups_should_still_map_into_the_layered_model() {
    let directory = tempdir().expect("temp directory should be created");
    let path = directory.path().join("legacy.toml");
    fs::write(
        &path,
        r#"
            [workspace]
            default_provider = "default"

            [providers.default]
            kind = "claude-code"
            model = "claude-sonnet-4"

            [agents.writer]
            provider = "default"
            model = "claude-opus-4"
            max_turns = 6
        "#,
    )
    .expect("config file should be written");

    let config = ConfigLoader::from_path(&path)
        .load()
        .expect("legacy config should still load");
    let resolved = config
        .resolve_agent_provider("writer")
        .expect("writer provider should resolve");

    assert_eq!(
        config
            .provider("default")
            .map(arky_config::ProviderConfig::driver),
        Some("claude-code")
    );
    assert_eq!(resolved.driver, "claude-code");
    assert_eq!(resolved.profile, None);
    assert_eq!(resolved.model.as_deref(), Some("claude-opus-4"));
}
