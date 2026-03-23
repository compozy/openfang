//! Deep merge helpers for configuration sources.

use std::collections::BTreeMap;

use crate::{
    layered::PartialProviderProfileConfig,
    loader::{
        PartialAgentConfig, PartialArkyConfig, PartialProviderConfig, PartialWorkspaceConfig,
    },
};

pub fn merge_config(base: PartialArkyConfig, overlay: PartialArkyConfig) -> PartialArkyConfig {
    PartialArkyConfig {
        workspace: merge_workspace(base.workspace, overlay.workspace),
        providers: merge_named_map(base.providers, overlay.providers, merge_provider),
        profiles: merge_named_map(base.profiles, overlay.profiles, merge_profile),
        agents: merge_named_map(base.agents, overlay.agents, merge_agent),
    }
}

pub fn merge_workspace(
    base: PartialWorkspaceConfig,
    overlay: PartialWorkspaceConfig,
) -> PartialWorkspaceConfig {
    PartialWorkspaceConfig {
        name: overlay.name.or(base.name),
        default_provider: overlay.default_provider.or(base.default_provider),
        data_dir: overlay.data_dir.or(base.data_dir),
        env: merge_string_map(base.env, overlay.env),
        profiles: merge_named_map(base.profiles, overlay.profiles, merge_profile),
    }
}

pub fn merge_provider(
    base: PartialProviderConfig,
    overlay: PartialProviderConfig,
) -> PartialProviderConfig {
    PartialProviderConfig {
        driver: overlay.driver.or(base.driver),
        binary: overlay.binary.or(base.binary),
        model: overlay.model.or(base.model),
        args: overlay.args.or(base.args),
        env: merge_string_map(base.env, overlay.env),
        cwd: overlay.cwd.or(base.cwd),
        shared_app_server_key: overlay.shared_app_server_key.or(base.shared_app_server_key),
        request_timeout_ms: overlay.request_timeout_ms.or(base.request_timeout_ms),
        startup_timeout_ms: overlay.startup_timeout_ms.or(base.startup_timeout_ms),
        cache_dir: overlay.cache_dir.or(base.cache_dir),
        runtime_dir: overlay.runtime_dir.or(base.runtime_dir),
        client_name: overlay.client_name.or(base.client_name),
        client_version: overlay.client_version.or(base.client_version),
    }
}

pub fn merge_profile(
    base: PartialProviderProfileConfig,
    overlay: PartialProviderProfileConfig,
) -> PartialProviderProfileConfig {
    let defaults = base.defaults.merge(&overlay.defaults);
    let config = base.config.merge(overlay.config);

    PartialProviderProfileConfig {
        driver: overlay.driver.or(base.driver),
        model: overlay.model.or(base.model),
        defaults,
        config,
    }
}

pub fn merge_agent(base: PartialAgentConfig, overlay: PartialAgentConfig) -> PartialAgentConfig {
    let defaults = base.defaults.merge(&overlay.defaults);
    let config = base.config.merge(overlay.config);

    PartialAgentConfig {
        provider: overlay.provider.or(base.provider),
        driver: overlay.driver.or(base.driver),
        profile: overlay.profile.or(base.profile),
        model: overlay.model.or(base.model),
        defaults,
        config,
        request_extra: merge_json_map(base.request_extra, overlay.request_extra),
        instructions: overlay.instructions.or(base.instructions),
        max_turns: overlay.max_turns.or(base.max_turns),
        tools: overlay.tools.or(base.tools),
    }
}

fn merge_named_map<T>(
    mut base: BTreeMap<String, T>,
    overlay: BTreeMap<String, T>,
    merge_entry: fn(T, T) -> T,
) -> BTreeMap<String, T> {
    for (key, incoming) in overlay {
        let merged = match base.remove(&key) {
            Some(existing) => merge_entry(existing, incoming),
            None => incoming,
        };
        base.insert(key, merged);
    }

    base
}

fn merge_string_map(
    base: Option<BTreeMap<String, String>>,
    overlay: Option<BTreeMap<String, String>>,
) -> Option<BTreeMap<String, String>> {
    match (base, overlay) {
        (Some(mut base), Some(overlay)) => {
            base.extend(overlay);
            Some(base)
        }
        (Some(base), None) => Some(base),
        (None, Some(overlay)) => Some(overlay),
        (None, None) => None,
    }
}

fn merge_json_map(
    mut base: BTreeMap<String, serde_json::Value>,
    overlay: BTreeMap<String, serde_json::Value>,
) -> BTreeMap<String, serde_json::Value> {
    base.extend(overlay);
    base
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::PathBuf};

    use pretty_assertions::assert_eq;

    use super::merge_config;
    use crate::{
        layered::PartialProviderProfileConfig,
        loader::{
            PartialAgentConfig, PartialArkyConfig, PartialProviderConfig, PartialWorkspaceConfig,
        },
    };

    #[test]
    fn merge_precedence_should_be_file_then_env_then_builder() {
        let file = file_config();
        let env = env_config();
        let builder = builder_config();

        let merged = merge_config(merge_config(file, env), builder);
        let provider = merged.providers.get("default");
        let agent = merged.agents.get("writer");

        let actual = (
            merged.workspace.name.as_deref(),
            merged
                .workspace
                .env
                .as_ref()
                .and_then(|value| value.get("RUST_LOG"))
                .map(String::as_str),
            provider.and_then(|value| value.model.as_deref()),
            provider
                .and_then(|value| value.env.as_ref())
                .and_then(|value| value.get("API_KEY"))
                .map(String::as_str),
            agent.and_then(|value| value.instructions.as_deref()),
            agent.and_then(|value| value.max_turns),
            agent.and_then(|value| value.tools.as_ref()).cloned(),
        );

        let expected = (
            Some("builder"),
            Some("debug"),
            Some("builder-model"),
            Some("env"),
            Some("builder instructions"),
            Some(8),
            Some(vec!["edit".to_owned()]),
        );

        assert_eq!(actual, expected);
    }

    fn file_config() -> PartialArkyConfig {
        PartialArkyConfig {
            workspace: PartialWorkspaceConfig {
                name: Some("file".to_owned()),
                default_provider: Some("default".to_owned()),
                data_dir: Some(PathBuf::from("file-dir")),
                env: Some(BTreeMap::from([("RUST_LOG".to_owned(), "info".to_owned())])),
                profiles: BTreeMap::new(),
            },
            providers: BTreeMap::from([(
                "default".to_owned(),
                PartialProviderConfig {
                    driver: Some("claude-code".to_owned()),
                    binary: Some(PathBuf::from("claude")),
                    model: Some("file-model".to_owned()),
                    args: Some(vec!["--json".to_owned()]),
                    env: Some(BTreeMap::from([("API_KEY".to_owned(), "file".to_owned())])),
                    cwd: None,
                    shared_app_server_key: None,
                    request_timeout_ms: None,
                    startup_timeout_ms: None,
                    cache_dir: None,
                    runtime_dir: None,
                    client_name: None,
                    client_version: None,
                },
            )]),
            profiles: BTreeMap::from([(
                "fast".to_owned(),
                PartialProviderProfileConfig {
                    driver: Some("claude-code".to_owned()),
                    model: Some("profile-model".to_owned()),
                    ..PartialProviderProfileConfig::default()
                },
            )]),
            agents: BTreeMap::from([(
                "writer".to_owned(),
                PartialAgentConfig {
                    provider: Some("default".to_owned()),
                    profile: Some("fast".to_owned()),
                    model: Some("file-model".to_owned()),
                    instructions: Some("file instructions".to_owned()),
                    max_turns: Some(4),
                    tools: Some(vec!["search".to_owned()]),
                    ..PartialAgentConfig::default()
                },
            )]),
        }
    }

    fn env_config() -> PartialArkyConfig {
        PartialArkyConfig {
            workspace: PartialWorkspaceConfig {
                name: Some("env".to_owned()),
                default_provider: None,
                data_dir: None,
                env: Some(BTreeMap::from([(
                    "RUST_LOG".to_owned(),
                    "debug".to_owned(),
                )])),
                profiles: BTreeMap::new(),
            },
            providers: BTreeMap::from([(
                "default".to_owned(),
                PartialProviderConfig {
                    driver: None,
                    binary: None,
                    model: Some("env-model".to_owned()),
                    args: None,
                    env: Some(BTreeMap::from([("API_KEY".to_owned(), "env".to_owned())])),
                    cwd: None,
                    shared_app_server_key: None,
                    request_timeout_ms: None,
                    startup_timeout_ms: None,
                    cache_dir: None,
                    runtime_dir: None,
                    client_name: None,
                    client_version: None,
                },
            )]),
            profiles: BTreeMap::from([(
                "fast".to_owned(),
                PartialProviderProfileConfig {
                    driver: None,
                    model: Some("env-profile-model".to_owned()),
                    ..PartialProviderProfileConfig::default()
                },
            )]),
            agents: BTreeMap::from([(
                "writer".to_owned(),
                PartialAgentConfig {
                    provider: None,
                    profile: None,
                    model: Some("env-model".to_owned()),
                    instructions: None,
                    max_turns: Some(8),
                    tools: None,
                    ..PartialAgentConfig::default()
                },
            )]),
        }
    }

    fn builder_config() -> PartialArkyConfig {
        PartialArkyConfig {
            workspace: PartialWorkspaceConfig {
                name: Some("builder".to_owned()),
                default_provider: None,
                data_dir: None,
                env: None,
                profiles: BTreeMap::new(),
            },
            providers: BTreeMap::from([(
                "default".to_owned(),
                PartialProviderConfig {
                    driver: None,
                    binary: None,
                    model: Some("builder-model".to_owned()),
                    args: None,
                    env: None,
                    cwd: None,
                    shared_app_server_key: None,
                    request_timeout_ms: None,
                    startup_timeout_ms: None,
                    cache_dir: None,
                    runtime_dir: None,
                    client_name: None,
                    client_version: None,
                },
            )]),
            profiles: BTreeMap::new(),
            agents: BTreeMap::from([(
                "writer".to_owned(),
                PartialAgentConfig {
                    provider: None,
                    profile: None,
                    model: Some("builder-model".to_owned()),
                    instructions: Some("builder instructions".to_owned()),
                    max_turns: None,
                    tools: Some(vec!["edit".to_owned()]),
                    ..PartialAgentConfig::default()
                },
            )]),
        }
    }
}
