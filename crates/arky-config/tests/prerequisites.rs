//! Integration coverage for provider prerequisite checks.

use arky_config::{ArkyConfig, ProviderConfigBuilder, WorkspaceConfigBuilder};
use pretty_assertions::assert_eq;

#[test]
fn prerequisite_check_should_find_cargo_on_path() {
    let config = ArkyConfig::builder()
        .workspace(WorkspaceConfigBuilder::new().default_provider("default"))
        .provider(
            "default",
            ProviderConfigBuilder::new().kind("codex").binary("cargo"),
        )
        .build()
        .expect("config should build");

    let resolved = config
        .check_prerequisites()
        .expect("cargo should be available on PATH");

    let actual = resolved
        .get("default")
        .and_then(|path| path.file_name())
        .map(|value| value.to_string_lossy().into_owned());

    let expected = Some(String::from("cargo"));

    assert_eq!(actual, expected);
}
