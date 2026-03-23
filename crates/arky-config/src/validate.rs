//! Validation and prerequisite checks for finalized configuration.

use std::{
    collections::BTreeMap,
    env,
    path::{Path, PathBuf},
};

use crate::{
    error::{ConfigError, ValidationIssue},
    layered::{
        validate_defaults, validate_driver, validate_request_extra, PartialProviderProfileConfig,
        ProviderProfileConfig,
    },
    loader::{
        AgentConfig, ArkyConfig, PartialAgentConfig, PartialArkyConfig, PartialProviderConfig,
        PartialWorkspaceConfig, ProviderConfig, WorkspaceConfig,
    },
    merge::merge_profile,
};

pub fn validate_config(mut config: PartialArkyConfig) -> Result<ArkyConfig, ConfigError> {
    let mut issues = Vec::new();
    merge_workspace_profiles(&mut config);

    let workspace = finalize_workspace(config.workspace, &mut issues);
    let providers = finalize_providers(config.providers, &mut issues);
    let profiles = finalize_profiles(config.profiles, &mut issues);
    let agents = finalize_agents(config.agents, &providers, &profiles, &mut issues);

    if let Some(default_provider) = workspace.default_provider() {
        if !providers.contains_key(default_provider) {
            issues.push(ValidationIssue::new(
                "workspace.default_provider",
                format!("references unknown provider `{default_provider}`"),
            ));
        }
    }

    if issues.is_empty() {
        Ok(ArkyConfig::new(workspace, providers, profiles, agents))
    } else {
        Err(ConfigError::validation(issues))
    }
}

pub fn check_provider_prerequisites(
    config: &ArkyConfig,
) -> Result<BTreeMap<String, PathBuf>, ConfigError> {
    let mut resolved = BTreeMap::new();

    for (name, provider) in config.providers() {
        let binary = provider.prerequisite_binary();
        let path =
            find_binary_on_path(binary.as_str()).ok_or_else(|| ConfigError::MissingBinary {
                provider: name.clone(),
                binary: binary.clone(),
            })?;
        resolved.insert(name.clone(), path);
    }

    Ok(resolved)
}

/// Finds an executable on `PATH`.
#[must_use]
pub fn find_binary_on_path(binary: &str) -> Option<PathBuf> {
    let candidate = Path::new(binary);
    if candidate.components().count() > 1 || candidate.is_absolute() {
        return candidate
            .is_file()
            .then(|| candidate.to_path_buf())
            .filter(|path| is_executable(path));
    }

    let path_value = env::var_os("PATH")?;
    for directory in env::split_paths(&path_value) {
        for candidate_name in candidate_names(binary) {
            let candidate_path = directory.join(candidate_name);
            if candidate_path.is_file() && is_executable(&candidate_path) {
                return Some(candidate_path);
            }
        }
    }

    None
}

fn merge_workspace_profiles(config: &mut PartialArkyConfig) {
    let workspace_profiles = std::mem::take(&mut config.workspace.profiles);

    for (name, profile) in workspace_profiles {
        let merged = match config.profiles.remove(&name) {
            Some(existing) => merge_profile(profile, existing),
            None => profile,
        };
        config.profiles.insert(name, merged);
    }
}

fn finalize_workspace(
    partial: PartialWorkspaceConfig,
    issues: &mut Vec<ValidationIssue>,
) -> WorkspaceConfig {
    let name = validate_optional_string(partial.name, "workspace.name", issues);
    let default_provider = validate_optional_string(
        partial.default_provider,
        "workspace.default_provider",
        issues,
    );

    WorkspaceConfig::new(
        name,
        default_provider,
        partial.data_dir,
        partial.env.unwrap_or_default(),
    )
}

fn finalize_providers(
    partials: BTreeMap<String, PartialProviderConfig>,
    issues: &mut Vec<ValidationIssue>,
) -> BTreeMap<String, ProviderConfig> {
    let mut providers = BTreeMap::new();

    for (name, partial) in partials {
        let field_prefix = format!("providers.{name}");
        let driver_field = format!("{field_prefix}.driver");
        let Some(driver) = require_string(
            validate_driver(partial.driver, driver_field.as_str(), issues),
            driver_field,
            issues,
        ) else {
            continue;
        };

        let model =
            validate_optional_string(partial.model, format!("{field_prefix}.model"), issues);

        let binary = partial.binary.and_then(|value| {
            let rendered = value.to_string_lossy().trim().to_owned();
            if rendered.is_empty() {
                issues.push(ValidationIssue::new(
                    format!("{field_prefix}.binary"),
                    "must not be empty",
                ));
                None
            } else {
                Some(value)
            }
        });

        providers.insert(
            name,
            ProviderConfig {
                driver,
                binary,
                model,
                args: partial.args.unwrap_or_default(),
                env: partial.env.unwrap_or_default(),
                cwd: partial.cwd,
                shared_app_server_key: validate_optional_string(
                    partial.shared_app_server_key,
                    format!("{field_prefix}.shared_app_server_key"),
                    issues,
                ),
                request_timeout_ms: partial.request_timeout_ms,
                startup_timeout_ms: partial.startup_timeout_ms,
                cache_dir: partial.cache_dir,
                runtime_dir: partial.runtime_dir,
                client_name: validate_optional_string(
                    partial.client_name,
                    format!("{field_prefix}.client_name"),
                    issues,
                ),
                client_version: validate_optional_string(
                    partial.client_version,
                    format!("{field_prefix}.client_version"),
                    issues,
                ),
            },
        );
    }

    providers
}

fn finalize_profiles(
    partials: BTreeMap<String, PartialProviderProfileConfig>,
    issues: &mut Vec<ValidationIssue>,
) -> BTreeMap<String, ProviderProfileConfig> {
    let mut profiles = BTreeMap::new();

    for (name, partial) in partials {
        let field_prefix = format!("profiles.{name}");
        let driver_field = format!("{field_prefix}.driver");
        let Some(driver) = require_string(
            validate_driver(partial.driver, driver_field.as_str(), issues),
            driver_field,
            issues,
        ) else {
            continue;
        };

        let model =
            validate_optional_string(partial.model, format!("{field_prefix}.model"), issues);
        let defaults = validate_defaults(
            partial.defaults,
            format!("{field_prefix}.defaults").as_str(),
            issues,
        );
        let config = partial.config.finalize_for_driver(
            driver.as_str(),
            format!("{field_prefix}.config").as_str(),
            issues,
        );

        profiles.insert(
            name,
            ProviderProfileConfig::new(driver, model, defaults, config),
        );
    }

    profiles
}

fn finalize_agents(
    partials: BTreeMap<String, PartialAgentConfig>,
    providers: &BTreeMap<String, ProviderConfig>,
    profiles: &BTreeMap<String, ProviderProfileConfig>,
    issues: &mut Vec<ValidationIssue>,
) -> BTreeMap<String, AgentConfig> {
    let mut agents = BTreeMap::new();

    for (name, partial) in partials {
        let field_prefix = format!("agents.{name}");
        let Some(provider) =
            require_string(partial.provider, format!("{field_prefix}.provider"), issues)
        else {
            continue;
        };

        if !providers.contains_key(provider.as_str()) {
            issues.push(ValidationIssue::new(
                format!("{field_prefix}.provider"),
                format!("references unknown provider `{provider}`"),
            ));
            continue;
        }

        let install_driver = providers
            .get(provider.as_str())
            .map(ProviderConfig::driver)
            .unwrap_or_default();
        let driver = validate_driver(
            partial.driver,
            format!("{field_prefix}.driver").as_str(),
            issues,
        );
        let profile =
            validate_optional_string(partial.profile, format!("{field_prefix}.profile"), issues);
        let profile_config = profile
            .as_deref()
            .and_then(|profile_name| profiles.get(profile_name));
        if let Some(profile_name) = profile.as_deref() {
            if profile_config.is_none() {
                issues.push(ValidationIssue::new(
                    format!("{field_prefix}.profile"),
                    format!("references unknown profile `{profile_name}`"),
                ));
                continue;
            }
        }

        if let Some(profile_config) = profile_config {
            if profile_config.driver() != install_driver {
                issues.push(ValidationIssue::new(
                    format!("{field_prefix}.profile"),
                    format!(
                        "targets driver `{}` but provider `{provider}` uses `{install_driver}`",
                        profile_config.driver()
                    ),
                ));
                continue;
            }
        }

        if let Some(driver) = driver.as_deref() {
            if driver != install_driver {
                issues.push(ValidationIssue::new(
                    format!("{field_prefix}.driver"),
                    format!(
                        "targets driver `{driver}` but provider `{provider}` uses `{install_driver}`"
                    ),
                ));
                continue;
            }
        }

        let effective_driver = driver
            .clone()
            .or_else(|| profile_config.map(|config| config.driver().to_owned()))
            .unwrap_or_else(|| install_driver.to_owned());
        let model =
            validate_optional_string(partial.model, format!("{field_prefix}.model"), issues);
        let defaults = validate_defaults(
            partial.defaults,
            format!("{field_prefix}.defaults").as_str(),
            issues,
        );
        let config = partial.config.finalize_for_driver(
            effective_driver.as_str(),
            format!("{field_prefix}.config").as_str(),
            issues,
        );
        validate_request_extra(
            &partial.request_extra,
            format!("{field_prefix}.request_extra").as_str(),
            issues,
        );
        let instructions = validate_optional_string(
            partial.instructions,
            format!("{field_prefix}.instructions"),
            issues,
        );
        let max_turns = partial.max_turns.and_then(|value| {
            if value == 0 {
                issues.push(ValidationIssue::new(
                    format!("{field_prefix}.max_turns"),
                    "must be greater than zero",
                ));
                None
            } else {
                Some(value)
            }
        });

        agents.insert(
            name,
            AgentConfig {
                provider,
                driver,
                profile,
                model,
                defaults,
                config,
                request_extra: partial.request_extra,
                instructions,
                max_turns,
                tools: partial.tools.unwrap_or_default(),
            },
        );
    }

    agents
}

fn require_string(
    value: Option<String>,
    field: impl Into<String>,
    issues: &mut Vec<ValidationIssue>,
) -> Option<String> {
    let field = field.into();

    let Some(value) = validate_optional_string(value, field.clone(), issues) else {
        issues.push(ValidationIssue::new(field, "is required"));
        return None;
    };

    Some(value)
}

fn validate_optional_string(
    value: Option<String>,
    field: impl Into<String>,
    issues: &mut Vec<ValidationIssue>,
) -> Option<String> {
    let field = field.into();

    value.and_then(|value| {
        let trimmed = value.trim().to_owned();
        if trimmed.is_empty() {
            issues.push(ValidationIssue::new(field, "must not be empty"));
            None
        } else {
            Some(trimmed)
        }
    })
}

fn candidate_names(binary: &str) -> Vec<String> {
    #[cfg(windows)]
    {
        let path = Path::new(binary);
        if path.extension().is_some() {
            return vec![binary.to_owned()];
        }

        if let Some(extensions) = env::var_os("PATHEXT") {
            let mut names = vec![binary.to_owned()];
            for extension in env::split_paths(&extensions) {
                names.push(format!("{binary}{}", extension.to_string_lossy()));
            }
            return names;
        }

        return vec![
            binary.to_owned(),
            format!("{binary}.exe"),
            format!("{binary}.cmd"),
            format!("{binary}.bat"),
        ];
    }

    #[cfg(not(windows))]
    {
        vec![binary.to_owned()]
    }
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.metadata()
        .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use arky_protocol::ReasoningEffort;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::{check_provider_prerequisites, validate_config};
    use crate::{
        layered::PartialProviderProfileConfig,
        loader::{
            PartialAgentConfig, PartialArkyConfig, PartialProviderConfig, PartialWorkspaceConfig,
        },
        ConfigError, ResolvedProviderBehaviorConfig,
    };

    #[test]
    fn validation_should_fail_when_required_fields_are_missing() {
        let config = PartialArkyConfig {
            workspace: PartialWorkspaceConfig::default(),
            providers: BTreeMap::from([("default".to_owned(), PartialProviderConfig::default())]),
            profiles: BTreeMap::new(),
            agents: BTreeMap::from([(
                "writer".to_owned(),
                PartialAgentConfig {
                    provider: Some("missing".to_owned()),
                    ..PartialAgentConfig::default()
                },
            )]),
        };

        let error = validate_config(config).expect_err("validation should fail");

        let actual = match error {
            ConfigError::ValidationFailed { issues, .. } => issues
                .iter()
                .map(|issue| (issue.field().to_owned(), issue.message().to_owned()))
                .collect::<Vec<_>>(),
            other => panic!("expected validation error, got {other:?}"),
        };

        let expected = vec![
            (
                "providers.default.driver".to_owned(),
                "is required".to_owned(),
            ),
            (
                "agents.writer.provider".to_owned(),
                "references unknown provider `missing`".to_owned(),
            ),
        ];

        assert_eq!(actual, expected);
    }

    #[test]
    fn prerequisite_check_should_return_missing_binary_when_not_available() {
        let config = validate_config(PartialArkyConfig {
            workspace: PartialWorkspaceConfig::default(),
            providers: BTreeMap::from([(
                "default".to_owned(),
                PartialProviderConfig {
                    driver: Some("custom-provider".to_owned()),
                    binary: Some("definitely-not-a-real-binary-for-arky-tests".into()),
                    model: None,
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
            agents: BTreeMap::new(),
        })
        .expect("config should validate");

        let error = check_provider_prerequisites(&config).expect_err("binary lookup should fail");

        let actual = match error {
            ConfigError::MissingBinary { provider, binary } => (provider, binary),
            other => panic!("expected missing binary error, got {other:?}"),
        };

        let expected = (
            "default".to_owned(),
            "definitely-not-a-real-binary-for-arky-tests".to_owned(),
        );

        assert_eq!(actual, expected);
    }

    #[test]
    fn resolved_agent_provider_should_merge_workspace_profile_and_agent_layers() {
        let config = validate_config(PartialArkyConfig {
            workspace: PartialWorkspaceConfig {
                default_provider: Some("default".to_owned()),
                ..PartialWorkspaceConfig::default()
            },
            providers: BTreeMap::from([(
                "default".to_owned(),
                PartialProviderConfig {
                    driver: Some("codex".to_owned()),
                    binary: Some("cargo".into()),
                    model: Some("gpt-5".to_owned()),
                    shared_app_server_key: Some("shared".to_owned()),
                    ..PartialProviderConfig::default()
                },
            )]),
            profiles: BTreeMap::from([(
                "fast".to_owned(),
                PartialProviderProfileConfig {
                    driver: Some("codex".to_owned()),
                    model: Some("gpt-5-mini".to_owned()),
                    defaults: crate::ProviderRequestDefaults {
                        max_tokens: Some(900),
                        reasoning_effort: Some(ReasoningEffort::Medium),
                    },
                    config: crate::PartialProviderBehaviorConfig {
                        codex: Some(crate::CodexBehaviorLayer {
                            include_plan_tool: Some(true),
                            web_search: Some(true),
                            ..crate::CodexBehaviorLayer::default()
                        }),
                        ..crate::PartialProviderBehaviorConfig::default()
                    },
                },
            )]),
            agents: BTreeMap::from([(
                "writer".to_owned(),
                PartialAgentConfig {
                    provider: Some("default".to_owned()),
                    profile: Some("fast".to_owned()),
                    model: Some("gpt-5-high".to_owned()),
                    defaults: crate::ProviderRequestDefaults {
                        max_tokens: Some(1_200),
                        reasoning_effort: None,
                    },
                    config: crate::PartialProviderBehaviorConfig {
                        codex: Some(crate::CodexBehaviorLayer {
                            resume_last: Some(true),
                            model_verbosity: Some("high".to_owned()),
                            ..crate::CodexBehaviorLayer::default()
                        }),
                        ..crate::PartialProviderBehaviorConfig::default()
                    },
                    request_extra: BTreeMap::from([("tool_choice".to_owned(), json!("required"))]),
                    ..PartialAgentConfig::default()
                },
            )]),
        })
        .expect("config should validate");

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
    fn profile_driver_mismatch_should_fail_clearly() {
        let error = validate_config(PartialArkyConfig {
            workspace: PartialWorkspaceConfig::default(),
            providers: BTreeMap::from([(
                "default".to_owned(),
                PartialProviderConfig {
                    driver: Some("codex".to_owned()),
                    ..PartialProviderConfig::default()
                },
            )]),
            profiles: BTreeMap::from([(
                "claude".to_owned(),
                PartialProviderProfileConfig {
                    driver: Some("claude_code".to_owned()),
                    ..PartialProviderProfileConfig::default()
                },
            )]),
            agents: BTreeMap::from([(
                "writer".to_owned(),
                PartialAgentConfig {
                    provider: Some("default".to_owned()),
                    profile: Some("claude".to_owned()),
                    ..PartialAgentConfig::default()
                },
            )]),
        })
        .expect_err("mismatched profile driver should fail");

        let actual = match error {
            ConfigError::ValidationFailed { issues, .. } => issues
                .iter()
                .map(|issue| (issue.field().to_owned(), issue.message().to_owned()))
                .collect::<Vec<_>>(),
            other => panic!("expected validation error, got {other:?}"),
        };

        let expected = vec![(
            "agents.writer.profile".to_owned(),
            "targets driver `claude-code` but provider `default` uses `codex`".to_owned(),
        )];

        assert_eq!(actual, expected);
    }

    #[test]
    fn request_extra_should_reject_install_level_keys_and_excessive_depth() {
        let error = validate_config(PartialArkyConfig {
            workspace: PartialWorkspaceConfig::default(),
            providers: BTreeMap::from([(
                "default".to_owned(),
                PartialProviderConfig {
                    driver: Some("codex".to_owned()),
                    ..PartialProviderConfig::default()
                },
            )]),
            profiles: BTreeMap::new(),
            agents: BTreeMap::from([(
                "writer".to_owned(),
                PartialAgentConfig {
                    provider: Some("default".to_owned()),
                    request_extra: BTreeMap::from([
                        ("api_key".to_owned(), json!("secret")),
                        (
                            "nested".to_owned(),
                            json!({
                                "level1": {
                                    "level2": {
                                        "level3": {
                                            "level4": "too deep"
                                        }
                                    }
                                }
                            }),
                        ),
                    ]),
                    ..PartialAgentConfig::default()
                },
            )]),
        })
        .expect_err("request_extra boundary violations should fail");

        let actual = match error {
            ConfigError::ValidationFailed { issues, .. } => issues
                .iter()
                .map(|issue| issue.field().to_owned())
                .collect::<Vec<_>>(),
            other => panic!("expected validation error, got {other:?}"),
        };

        let expected = vec![
            "agents.writer.request_extra.api_key".to_owned(),
            "agents.writer.request_extra.nested.level1.level2.level3.level4".to_owned(),
        ];

        assert_eq!(actual, expected);
    }

    #[test]
    fn request_extra_should_reject_excessive_entry_counts() {
        let request_extra = (0..33)
            .map(|index| (format!("key_{index}"), json!(index)))
            .collect::<BTreeMap<_, _>>();
        let error = validate_config(PartialArkyConfig {
            workspace: PartialWorkspaceConfig::default(),
            providers: BTreeMap::from([(
                "default".to_owned(),
                PartialProviderConfig {
                    driver: Some("codex".to_owned()),
                    ..PartialProviderConfig::default()
                },
            )]),
            profiles: BTreeMap::new(),
            agents: BTreeMap::from([(
                "writer".to_owned(),
                PartialAgentConfig {
                    provider: Some("default".to_owned()),
                    request_extra,
                    ..PartialAgentConfig::default()
                },
            )]),
        })
        .expect_err("too many request_extra entries should fail");

        let actual = match error {
            ConfigError::ValidationFailed { issues, .. } => issues
                .iter()
                .map(|issue| (issue.field().to_owned(), issue.message().to_owned()))
                .collect::<Vec<_>>(),
            other => panic!("expected validation error, got {other:?}"),
        };

        assert_eq!(
            actual,
            vec![(
                "agents.writer.request_extra".to_owned(),
                "must not contain more than 32 nested request_extra entries".to_owned(),
            )]
        );
    }
}
