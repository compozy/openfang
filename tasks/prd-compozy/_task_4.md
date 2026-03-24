## markdown

## status: completed

<task_context>
<domain>providers/arky/integration</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>none</dependencies>
</task_context>

# Task 4.0: Copy Arky Crates Into OpenFang Workspace

## Overview

Copy the required Arky SDK crates from the `~/Dev/compozy/arky/` workspace (at
`/Users/pedronauck/Dev/compozy/arky/crates/`) into the OpenFang workspace under
`crates/`. The crates to copy are: `arky-config`, `arky-protocol`,
`arky-provider`, `arky-codex`, `arky-claude-code`, `arky-error`. After copying,
register them as workspace members in `openfang/Cargo.toml` and ensure they
compile within the OpenFang workspace.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Per ADR-012, Arky crates must be first-class workspace members of the OpenFang fork, not path-external dependencies. This is the prerequisite for Claude Code and Codex as strategic providers.
- All six crates (`arky-config`, `arky-protocol`, `arky-provider`, `arky-codex`, `arky-claude-code`, `arky-error`) must be registered as `members` in `openfang/Cargo.toml` and declared under `[workspace.dependencies]` with `{ path = "crates/<name>" }`.
- The copied crates already use `edition.workspace = true` and inherit the OpenFang workspace edition (`2021`) and resolver (`2`) automatically. No edition downgrade is needed — verify this by confirming each Arky crate's `Cargo.toml` has `edition.workspace = true` and does not override it locally.
- Line-width for rustfmt is `100` in OpenFang (vs `90` in Arky). All copied source files must be re-formatted with `cargo +nightly-2026-03-15 fmt --all` after copy so they conform to OpenFang's `.rustfmt.toml`.
- Dependency conflicts between Arky crates and existing OpenFang crates must be resolved at the `[workspace.dependencies]` level. Do not pin different versions of the same third-party crate in individual `Cargo.toml` files — promote shared versions into the workspace table.
- Existing tests inside the copied crates must remain enabled and must pass. Do not disable or `#[ignore]` tests to paper over compilation failures.
- The `openfang-kernel` and `openfang-types` crates must remain compilable after the workspace expands. Check that no symbol collisions occur between Arky and OpenFang type definitions (e.g., both define message and tool types).
- Additional Arky crates that are transitive dependencies of the six primary crates but not yet in the OpenFang workspace (`arky-hooks`, `arky-session`, `arky-tools`, `arky-mcp`) must also be included in the copy if required for compilation. Verify the full transitive dependency graph before starting.
</requirements>

## Subtasks

- [x] 4.1 Audit the full transitive dependency graph of the six primary crates in `~/Dev/compozy/arky/crates/` to determine the complete set of Arky crates that must be copied (including `arky-hooks`, `arky-session`, `arky-tools`, `arky-mcp` if required). The `arky/CLAUDE.md` documents the dependency hierarchy bottom-up: `arky-error` and `arky-protocol` are leaf crates; `arky-tools`, `arky-hooks`, `arky-session`, `arky-provider` are foundation; `arky-mcp` is integration; `arky-codex` and `arky-claude-code` are providers.
- [x] 4.2 Copy all required crate directories from `~/Dev/compozy/arky/crates/` into `openfang/crates/`. Preserve internal module structure exactly. Update each crate's `Cargo.toml` to use `workspace = true` for shared fields (`version`, `edition`, `license`, `repository`, `rust-version`) and to reference sibling Arky crates via `workspace = true` rather than relative paths pointing outside the workspace.
- [x] 4.3 Register every copied crate in `openfang/Cargo.toml` under both `workspace.members` and `workspace.dependencies`. Add all third-party dependencies introduced by the Arky crates (e.g., `serde_norway`, `async-trait`, `tokio-util`, `pretty_assertions`, `tempfile`) to the workspace dependency table if not already present, resolving version conflicts with existing OpenFang entries.
- [x] 4.4 Adapt workspace-level lint configuration. The Arky workspace uses `deny(warnings)` with pedantic and nursery groups; OpenFang's `.clippy.toml` has additional disallowed macros and methods. Run `cargo clippy --workspace --all-targets -- -D warnings` and fix all new warnings introduced by the copied crates, without suppressing them with blanket `allow` attributes.
- [x] 4.5 Re-format all copied source files with `cargo +nightly-2026-03-15 fmt --all` to apply OpenFang's `max_width = 100` and vertical import layout from `.rustfmt.toml`. Arky uses `max_width = 90`, so expect lines to reflow.
- [x] 4.6 Verify that `openfang-runtime` and `openfang-kernel` still compile and their tests still pass after the workspace expansion. Pay special attention to the existing `drivers/claude_code.rs` and `drivers/mod.rs` in `crates/openfang-runtime/src/drivers/` — these already reference `claude-code` and `codex` providers via the legacy `LlmDriver` interface and must not collide with the new Arky `Provider` trait.
- [x] 4.7 Run `./scripts/check-deps.sh` to validate the internal crate dependency graph. Confirm that no circular dependencies exist between the copied Arky crates and the existing OpenFang crates.

## Implementation Details

### Current State of the Arky Crates

Several Arky crates are already present in `openfang/crates/` (the copy was partially started):
`arky-claude-code`, `arky-codex`, `arky-config`, `arky-error`, `arky-hooks`, `arky-mcp`,
`arky-protocol`, `arky-provider`, `arky-session`, `arky-tools`. However, none are yet registered
as workspace members in `openfang/Cargo.toml` — that table still lists only the 13 original
OpenFang crates plus `xtask`.

The openfang copy of `arky-config` already contains a `layered.rs` module not present in the
upstream Arky source. This means the copy is ahead of upstream in that crate and must not be
overwritten blindly. Treat the openfang copy as the canonical version for crates that have
diverged.

### Key Crate Structures to Understand

- `arky-error/src/lib.rs` — defines the `ClassifiedError` trait and `RuntimeError` union type.
  Every other Arky crate's error type implements `ClassifiedError`.
- `arky-protocol/src/` — defines shared protocol types: `AgentEvent`, `Message`, `SessionRef`,
  `TurnContext`, `ProviderSettings`, `ReasoningEffort`, `ProviderId`, `ModelRef`. These are the
  wire types used across the entire provider stack.
- `arky-provider/src/traits.rs` — defines the core `Provider` trait with `descriptor()` and
  `stream()`. The `ProviderEventStream` type alias is `Pin<Box<dyn Stream<Item = Result<AgentEvent, ProviderError>> + Send>>`.
- `arky-provider/src/descriptor.rs` — defines `ProviderDescriptor`, `ProviderFamily`,
  `ProviderCapabilities`, and `validate_capabilities()`.
- `arky-provider/src/registry.rs` — defines `ProviderRegistry` (thread-safe, `Arc<RwLock<BTreeMap<ProviderId, Arc<dyn Provider>>>>`).
- `arky-config/src/loader.rs` — defines `ArkyConfig`, `WorkspaceConfig`, `ProviderConfig`,
  `AgentConfig`, and the `ConfigLoader` struct with file + env + builder merge.
- `arky-config/src/layered.rs` (openfang copy only) — already defines `ProviderBehaviorLayer`,
  `ResolvedAgentProviderConfig<TInstall>`, `ProviderRequestDefaults`, `CodexBehaviorLayer`,
  `ClaudeCodeBehaviorLayer`, `ClaudeCompatibleBehaviorLayer`, and `validate_request_extra()`. This
  is the new provider layering layer added for the Compozy fork.
- `arky-codex/src/config.rs` — defines `CodexProviderConfig` with sub-structs
  `CodexProcessConfig`, `CodexSandboxConfig`, `CodexWorkspaceConfig`, `CodexCapabilityConfig`.
- `arky-claude-code/src/config.rs` — defines `ClaudeCodeProviderConfig` with sub-structs
  `ClaudeCliBehaviorConfig`, `ClaudePermissionConfig`, `ClaudeSessionConfig`,
  `ClaudeFilesystemConfig`.
- `arky-claude-code/src/profile.rs` — defines `ClaudeProviderProfile` enum and all wrapper
  provider types (`BedrockProviderConfig`, `ZaiProviderConfig`, etc.) plus
  `ClaudeCompatibleProviderKind`.

### Integration Point with OpenFang Runtime

`openfang-runtime/src/drivers/mod.rs` already has a `create_driver()` function that handles
`"claude-code"` and `"codex"` provider strings via the legacy `LlmDriver` trait (defined in
`openfang-runtime/src/llm_driver.rs`). This is the old OpenFang provider path. The new Arky
`Provider` trait is a separate abstraction. Task 4 must not break the legacy driver path; tasks
10-12 will wire the new Arky path on top.

### Workspace Configuration Differences

| Setting               | Arky workspace (upstream) | OpenFang workspace         |
| --------------------- | ------------------------- | -------------------------- |
| `resolver`            | `3`                       | `2`                        |
| `edition`             | `2024`                    | `2021`                     |
| `max_width` (rustfmt) | `90`                      | `100`                      |
| `anyhow`              | allowed broadly           | restricted to WASM sandbox |
| Error model           | `ClassifiedError` trait   | `thiserror` per-crate      |

The upstream Arky workspace uses edition `2024` and resolver `3`, but the OpenFang workspace uses
`edition = "2021"` and `resolver = "2"`. The copied Arky crates already use `edition.workspace = true`,
so they inherit OpenFang's edition `2021` automatically. **Do NOT change the workspace edition or resolver.**
No edition downgrade work is expected — the crates are already compatible since they inherit
the workspace edition via `edition.workspace = true`.

### Relevant Files

- `/Users/pedronauck/Dev/compozy/openfang/Cargo.toml` — workspace root, needs `members` and `workspace.dependencies` updates
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-config/` — already copied, has diverged with `layered.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-protocol/` — already copied
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-provider/` — already copied
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-codex/` — already copied
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-claude-code/` — already copied
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-error/` — already copied
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-hooks/` — already copied (transitive dep)
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-session/` — already copied (transitive dep)
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-tools/` — already copied (transitive dep)
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-mcp/` — already copied (transitive dep)
- `/Users/pedronauck/Dev/compozy/arky/crates/` — upstream source for any crates that need re-sync
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-runtime/src/drivers/mod.rs` — legacy driver path, must remain unbroken
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/Cargo.toml` — downstream consumer
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-types/Cargo.toml` — downstream consumer
- `/Users/pedronauck/Dev/compozy/openfang/.rustfmt.toml` — enforces `max_width = 100`
- `/Users/pedronauck/Dev/compozy/openfang/.clippy.toml` — enforces disallowed macros/methods
- `/Users/pedronauck/Dev/compozy/openfang/scripts/check-deps.sh` — dependency graph validator

### Dependent Files

- `crates/openfang-kernel/Cargo.toml` — will need to declare `arky-provider` and `arky-config` as dependencies in tasks 10-12
- `crates/openfang-types/Cargo.toml` — may need to reference `arky-protocol` types once the provider layer is wired
- All tasks 10, 11, 12 depend on this task completing cleanly

## Deliverables

- All Arky crates compilable as workspace members in `openfang/Cargo.toml`
- Workspace `Cargo.toml` `members` list and `[workspace.dependencies]` updated
- All existing OpenFang crate tests still pass
- All existing Arky crate tests still pass within the new workspace context
- `./scripts/check-deps.sh` passes without circular dependency errors

## Tests

### Unit Tests (Required)

- [x] `arky_error` — `classified_error_defaults_should_match_the_techspec` and `http_error_mapping_should_capture_http_projection` pass after copy
- [x] `arky_provider` — `provider_descriptor_should_preserve_construction_inputs`, `provider_registry_should_register_lookup_and_list_providers`, `infer_provider_id_should_map_known_model_prefixes` pass after copy
- [x] `arky_provider` — `generate_response_from_stream_should_use_terminal_message_event` and `generate_response_from_stream_should_reject_missing_terminal_message` pass
- [x] `arky_config` — `file_loading_should_parse_valid_toml`, `env_overrides_should_override_file_values`, `builder_should_override_environment_values` pass after copy
- [x] `arky_codex` — `config_should_round_trip_through_serde` and `config_registry_key_should_ignore_cwd_when_shared_key_is_set` pass
- [x] `arky_claude_code` — `config_should_serialize_key_runtime_fields_to_cli_args` and `validators_should_cover_model_prompt_and_session_warnings` pass
- [x] `arky_claude_code` — `supported_provider_kinds_should_round_trip` and `selected_model_should_prefer_provider_model_id` pass in `profile.rs` tests
- [x] No test in any copied crate is annotated with `#[ignore]` that was not already ignored in the upstream Arky source

### Integration Tests (Required)

- [x] `cargo build --workspace` succeeds with zero errors after adding all Arky crates to `workspace.members`
- [x] `cargo test --workspace` passes across all 23+ crates (13 original OpenFang + 10 Arky) with zero failures
- [x] `cargo clippy --workspace --all-targets -- -D warnings` produces zero warnings
- [x] The legacy `create_driver("claude-code", ...)` path in `openfang-runtime/src/drivers/mod.rs` still compiles and its existing tests pass unchanged
- [x] `cargo test -p openfang-kernel` and `cargo test -p openfang-types` pass with no regressions

### Regression and Anti-Pattern Guards

- [x] Do not introduce path dependencies that point outside the workspace (e.g., `arky-error = { path = "../../arky/crates/arky-error" }` is forbidden)
- [x] Do not silently disable Arky crate tests with `#[ignore]` or `cfg(test)` gating to make the build pass
- [x] Do not add `anyhow` as a dependency to non-WASM crates — OpenFang restricts `anyhow` to the WASM sandbox only (see key decisions in project memory)
- [x] Do not change the workspace `resolver = "2"` or `edition = "2021"` — all copied Arky crates inherit edition 2021 via `edition.workspace = true` and no downgrade is expected
- [x] Do not manually add version pins in individual `Cargo.toml` files for dependencies already in `[workspace.dependencies]` — use `dep.workspace = true`

### Verification Commands

- [x] `cargo fmt --all`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`

## Success Criteria

- All ten Arky crates (`arky-config`, `arky-protocol`, `arky-provider`, `arky-codex`, `arky-claude-code`, `arky-error`, `arky-hooks`, `arky-session`, `arky-tools`, `arky-mcp`) are workspace members and compile cleanly.
- The openfang copy of `arky-config` with its additional `layered.rs` module is preserved and compiles.
- Zero existing OpenFang tests are broken by the workspace expansion.
- Zero existing Arky crate tests are disabled or skipped as a result of this task.
- `./scripts/check-deps.sh` passes — no circular dependencies.
- Provider tasks 10, 11, and 12 can declare `arky-config`, `arky-provider`, `arky-codex`, and `arky-claude-code` as workspace dependencies immediately.
- `cargo clippy --workspace --all-targets -- -D warnings` passes with zero warnings.

---

## Notes

- This task is a prerequisite for all Arky-based provider integration work (tasks 10, 11, 12).
- The upstream Arky workspace is at `/Users/pedronauck/Dev/compozy/arky/`. Do not modify it. All changes happen inside `openfang/`.
- The openfang copy of `arky-config` already diverges from upstream with `layered.rs` — treat the openfang copy as canonical for that crate.
- Keep the copied crates as close to their upstream form as practical to simplify future syncs, except for workspace-level wiring and formatting changes mandated by OpenFang conventions.
