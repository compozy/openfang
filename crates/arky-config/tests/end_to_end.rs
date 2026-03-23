//! Integration coverage for end-to-end config loading.

use std::fs;

use arky_config::ConfigLoader;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

#[test]
fn end_to_end_loading_should_merge_file_env_and_validation() {
    let directory = tempdir().expect("temp directory should be created");
    let path = directory.path().join("arky.toml");
    fs::write(
        &path,
        r#"
            [workspace]
            default_provider = "default"

            [providers.default]
            kind = "claude-code"
            model = "claude-sonnet-4"

            [providers.default.env]
            RUST_LOG = "info"

            [agents.writer]
            provider = "default"
            model = "claude-sonnet-4"
            max_turns = 4
        "#,
    )
    .expect("config file should be written");

    let config = ConfigLoader::from_path(&path)
        .with_env_overrides([
            (
                "ARKY_PROVIDERS__DEFAULT__MODEL".to_owned(),
                "claude-opus-4".to_owned(),
            ),
            ("ARKY_AGENTS__WRITER__MAX_TURNS".to_owned(), "6".to_owned()),
            (
                "ARKY_WORKSPACE__ENV__RUST_LOG".to_owned(),
                "debug".to_owned(),
            ),
        ])
        .load()
        .expect("config should load");

    let actual = (
        config.workspace().default_provider(),
        config.workspace().env().get("RUST_LOG").map(String::as_str),
        config
            .provider("default")
            .and_then(|provider| provider.model()),
        config
            .agent("writer")
            .and_then(arky_config::AgentConfig::max_turns),
    );

    let expected = (
        Some("default"),
        Some("debug"),
        Some("claude-opus-4"),
        Some(6),
    );

    assert_eq!(actual, expected);
}
