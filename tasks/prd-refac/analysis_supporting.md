# Refactoring Analysis: Supporting Crates

**Date**: 2026-03-27
**Scope**: 19 crates (arky-*, openfang-types, openfang-channels, openfang-extensions, openfang-hands, openfang-migrate, openfang-skills, openfang-wire, openfang-agent-definition, openfang-provider-binding)
**Total lines analyzed**: ~104,659

---

## Executive Summary

The supporting crate layer suffers from three dominant problems:

1. **openfang-types/config.rs** is a 4,321-line monolith that mixes 40+ channel config structs, kernel config, validation logic, and helper functions into one file.
2. **openfang-channels** contains 42 adapter implementations (29,074 lines total) with massive structural duplication -- every adapter hand-rolls shutdown channels, reqwest clients, backoff constants, and WebSocket reconnection loops.
3. **openfang-migrate/openclaw.rs** is a 4,608-line file with interleaved parsing types, conversion logic, migration orchestration, and 1,600+ lines of tests -- all in a single module.

These three files alone account for 13,537 lines (13% of the analyzed codebase) and represent the highest-leverage refactoring targets.

---

## Crate-by-Crate Analysis

### 1. openfang-types (17,814 lines) -- CRITICAL

| File | Lines | Issue |
|------|-------|-------|
| config.rs | 4,321 | **Monolith**: 40+ channel configs, KernelConfig (45 fields), validation, helpers, tests |
| agent.rs | 1,325 | Large but well-scoped |
| scheduler.rs | 1,183 | Large but well-scoped |
| workflow.rs | 1,148 | Large but well-scoped |
| task.rs | 1,008 | Large but well-scoped |
| contract.rs | 948 | Large but well-scoped |

**Problems found:**

- **config.rs -- God file (CRITICAL)**: This single file contains:
  - 40+ channel configuration structs (TelegramConfig, DiscordConfig, SlackConfig, ..., WeComConfig) with their Default impls and serde helpers -- approximately 2,600 lines
  - KernelConfig struct with 45 fields and a 70-line Default impl
  - 80-line custom Debug impl for KernelConfig
  - The `validate()` method -- a 460-line chain of identical `if let Some(ref x) = self.channels.X { check env var }` blocks
  - The `clamp_bounds()` method
  - Database path validation logic (60 lines)
  - 18 standalone helper functions (`default_true()`, `default_thread_ttl()`, etc.)
  - 550+ lines of tests

- **validate() is pure boilerplate**: The method at line 3197 repeats the exact same env-var-check pattern 30+ times. Each block is:
  ```rust
  if let Some(ref x) = self.channels.channel_name {
      if std::env::var(&x.token_env).unwrap_or_default().is_empty() {
          warnings.push(format!("ChannelName configured but {} is not set", x.token_env));
      }
  }
  ```

- **ChannelsConfig struct** (lines 1737-1827) has 40+ `Option<XxxConfig>` fields -- a sign that channel configs should be registry-driven (HashMap) rather than hard-coded struct fields.

**Recommended refactoring:**

1. Split config.rs into a `config/` module directory:
   - `config/mod.rs` -- KernelConfig, DefaultModelConfig, PersistenceConfig, MemoryConfig
   - `config/channels.rs` -- ChannelsConfig + all 40 channel config structs
   - `config/security.rs` -- ExecPolicy, DockerSandboxConfig, AuthConfig, VaultConfig
   - `config/web.rs` -- WebConfig, BraveSearchConfig, TavilySearchConfig, etc.
   - `config/integrations.rs` -- A2aConfig, McpServerConfigEntry, ExtensionsConfig, OAuthConfig
   - `config/media_tts.rs` -- TtsConfig, BrowserConfig, CanvasConfig
   - `config/routing.rs` -- AgentBinding, BroadcastConfig, AutoReplyConfig
   - `config/validate.rs` -- validate() and clamp_bounds()
2. Introduce a `ChannelConfigEntry` trait with `fn required_env_vars() -> Vec<&str>` to eliminate the repetitive validation boilerplate.
3. Consider making ChannelsConfig use `HashMap<String, Box<dyn ChannelConfig>>` instead of 40 named fields. This would eliminate the need to modify the struct every time a channel is added.

**Priority**: CRITICAL -- this file is a bottleneck for every contributor, causes merge conflicts, and grows linearly with every new feature.

---

### 2. openfang-channels (29,074 lines) -- CRITICAL

| File | Lines | Role |
|------|-------|------|
| bridge.rs | 1,981 | Bridge manager (well-structured) |
| telegram.rs | 1,862 | Adapter |
| feishu.rs | 1,295 | Adapter |
| discord.rs | 904 | Adapter |
| + 38 more adapters | 500-750 each | All adapters |

**Problems found:**

- **Massive structural duplication across 42 adapters**: Every adapter repeats:
  - `shutdown_tx: Arc<watch::Sender<bool>>` + `shutdown_rx: watch::Receiver<bool>` fields (85 occurrences across 43 files)
  - `let (shutdown_tx, shutdown_rx) = watch::channel(false)` in constructors (43 occurrences)
  - `client: reqwest::Client` field + `reqwest::Client::new()` (72 occurrences across 36 files)
  - `MAX_BACKOFF` / `INITIAL_BACKOFF` constants
  - Identical WebSocket reconnection loops (discord, slack, revolt, feishu, etc.)
  - Identical message-splitting logic (`split_message()` is called from every adapter)
  - Identical `shutdown()` trait implementations

- **Dead code**: 47 `#[allow(dead_code)]` annotations across channel adapter files indicate unused fields that were never cleaned up.

- **No shared HTTP/webhook adapter base**: 20+ adapters are webhook-based (LINE, Viber, Messenger, Teams, Google Chat, Threema, etc.) and share the same pattern: listen on a port, verify signature, parse JSON body, route to agent. This is reimplemented 20 times.

**Recommended refactoring:**

1. **Extract `AdapterBase` struct**:
   ```rust
   pub struct AdapterBase {
       client: reqwest::Client,
       shutdown_tx: Arc<watch::Sender<bool>>,
       shutdown_rx: watch::Receiver<bool>,
   }
   ```
   Every adapter embeds this via composition instead of duplicating the fields.

2. **Extract `WebhookAdapterBase`** for webhook-mode adapters:
   ```rust
   pub struct WebhookAdapterBase {
       base: AdapterBase,
       port: u16,
       path: String,
   }
   impl WebhookAdapterBase {
       async fn listen(&self, handler: impl WebhookHandler) -> ...
   }
   ```

3. **Extract `WebSocketAdapterBase`** for WS-mode adapters (Discord, Slack, Revolt, Feishu):
   ```rust
   pub struct WebSocketAdapterBase {
       base: AdapterBase,
       reconnect_policy: ReconnectPolicy,
   }
   ```

4. **Move constants to shared module**: `MAX_BACKOFF`, `INITIAL_BACKOFF`, message limits.

5. **Clean up dead code**: Remove all `#[allow(dead_code)]` fields or implement them.

**Priority**: CRITICAL -- 29K lines with rampant duplication. Each new channel adds ~600 lines of boilerplate that could be ~100 lines if base traits existed.

---

### 3. openfang-migrate (5,500 lines) -- HIGH

| File | Lines | Issue |
|------|-------|-------|
| openclaw.rs | 4,608 | **Monolith**: types + parsing + conversion + migration + tests all in one file |
| report.rs | 211 | Clean |
| lib.rs | 77 | Clean |

**Problems found:**

- **openclaw.rs -- Single-file monolith (4,608 lines)**: Contains:
  - 20+ OpenClaw input type structs (lines 34-317) -- 283 lines
  - Legacy YAML input types (lines 322-407) -- 85 lines
  - 30+ conversion/mapping helper functions (lines 455-929) -- 474 lines
  - Public API functions: `detect_openclaw_home`, `scan_openclaw_workspace`, `migrate` (lines 930-1450)
  - Migration orchestration: `migrate_from_json5`, `migrate_config_from_json`, `migrate_channels_from_json`, `migrate_agents`, `migrate_skills`, `migrate_sessions`, `migrate_memory` (lines 1312-2850) -- ~1,538 lines
  - 1,600+ lines of tests (lines 3000-4608)

- **10 `#[allow(dead_code)]` annotations** indicate unused fields in input types.

- **86 functions** in a single file -- well beyond reasonable module scope.

**Recommended refactoring:**

1. Split into a module directory:
   - `openclaw/mod.rs` -- public API (`detect_openclaw_home`, `scan_openclaw_workspace`, `migrate`)
   - `openclaw/types.rs` -- OpenClaw input structs and Legacy YAML structs
   - `openclaw/convert.rs` -- Mapping/conversion helpers (`map_dm_policy`, `split_model_ref`, `tools_for_profile`, etc.)
   - `openclaw/channels.rs` -- Channel migration logic
   - `openclaw/agents.rs` -- Agent migration logic
   - `openclaw/sessions.rs` -- Session/memory migration
   - Tests stay in `tests/` directory (already partially there)

**Priority**: HIGH -- 4,608 lines in one file makes navigation and maintenance painful, but this is migration code that changes less frequently.

---

### 4. arky-claude-code (7,362 lines) -- MEDIUM

| File | Lines | Issue |
|------|-------|-------|
| provider.rs | 1,541 | Complex orchestration |
| parser.rs | 1,196 | Event parsing |
| profile.rs | 871 | Profile handling |
| config.rs | 837 | Configuration |
| conversion.rs | 725 | Type conversion |

**Problems found:**

- **provider.rs** is large (1,541 lines) but already has good internal structure with helper structs (`StreamState`, `ClaudeProcessPlan`). Could benefit from extracting the process building logic.

- **Dedup module** (`dedup.rs`, 84 lines) is simple text dedup. The codex crate has its own `FingerprintDeduper` (119 lines). These serve different purposes (text-level vs. notification-level) so are not strict duplicates, but the pattern of each provider crate having its own dedup is worth noting.

- The crate is well-decomposed overall (12 source files). No critical issues.

**Recommended refactoring:**

- Consider extracting process-building logic from `provider.rs` (the `build_process_config` + `generate` flow) into a separate `orchestration.rs` to keep the Provider trait impl focused.
- `config.rs` (837 lines) contains both config types and validation -- could split validation into its own module.

**Priority**: MEDIUM -- functional but slightly large provider.rs.

---

### 5. arky-codex (7,335 lines) -- MEDIUM

| File | Lines | Issue |
|------|-------|-------|
| rpc.rs | 1,038 | RPC handling |
| provider.rs | 1,033 | Provider impl |
| dispatcher.rs | 907 | Event dispatch |
| thread.rs | 582 | Thread management |
| app_server.rs | 579 | Server management |

**Problems found:**

- Well-decomposed into 14 source files. No single file exceeds 1,100 lines.
- `rpc.rs` and `provider.rs` are both around 1,000 lines -- manageable.
- Same dedup pattern as arky-claude-code (see note above).

**Recommended refactoring:**

- Minor: `rpc.rs` mixes JSON-RPC message types with transport logic. Could split types into `rpc_types.rs`.

**Priority**: LOW -- well-structured.

---

### 6. arky-config (4,694 lines) -- MEDIUM

| File | Lines | Issue |
|------|-------|-------|
| loader.rs | 1,794 | Config loading + types |
| layered.rs | 1,002 | Layered config system |
| validate.rs | 763 | Validation |
| merge.rs | 327 | Config merging |

**Problems found:**

- **loader.rs (1,794 lines)** mixes config types (`ArkyConfig`, `ProviderConfig`, `AgentConfig`, `WorkspaceConfig`, their builders, and their impls) with loading logic (`ConfigLoader`, `from_path`, `from_toml`, `from_yaml`). This is a structural issue but less severe than config.rs in openfang-types because the types here are fewer and more cohesive.

- **Parallel validation systems**: This crate has `validate.rs` (763 lines) AND `validation.rs` (219 lines). Two validation modules in one crate is confusing.

**Recommended refactoring:**

1. Merge `validation.rs` into `validate.rs` or rename for clarity.
2. Consider splitting `loader.rs` into `types.rs` (config structs + builders) and `loader.rs` (file loading + parsing).

**Priority**: MEDIUM -- the dual validation files are confusing.

---

### 7. arky-provider (2,941 lines) -- LOW

Well-structured with 16 files, none exceeding 416 lines. Clean trait hierarchy (`Provider`, `ProviderDescriptor`, `ProcessManager`). No significant issues.

---

### 8. arky-protocol (2,454 lines) -- LOW

Clean separation of concerns: `request.rs` (682), `event.rs` (448), `message.rs` (348), `utils.rs` (235), `id.rs` (229), `session.rs` (208), `tool.rs` (146). Well-modularized.

---

### 9. arky-hooks (2,961 lines) -- LOW

Reasonable structure. `chain.rs` (1,144 lines) is the largest but handles a genuinely complex hook-chain system. Tests are properly in `tests/` directory.

---

### 10. arky-mcp (2,685 lines) -- LOW

Well-structured: `client.rs` (799), `bridge.rs` (504), `server.rs` (430), `error.rs` (243), `auth.rs` (162), `naming.rs` (156). No issues.

---

### 11. arky-session (2,707 lines) -- LOW

Clean split: `sqlite.rs` (1,022), `memory.rs` (798). The SQLite impl is large but coherent. No issues.

---

### 12. arky-tools (1,698 lines) -- LOW

Small and well-scoped. No issues.

---

### 13. arky-error (974 lines) -- LOW

Clean error classification infrastructure. `ClassifiedError` trait is well-designed and used across all arky crates.

---

### 14. openfang-provider-binding (4,154 lines) -- MEDIUM

| File | Lines | Issue |
|------|-------|-------|
| bridge.rs | 1,209 | Driver bridge |
| adapter.rs | 1,126 | Provider adapter |
| lib.rs | 907 | Types + compile pipeline |
| convert.rs | 747 | Type conversions |

**Problems found:**

- **lib.rs at 907 lines** contains both the `ProviderBinding` type and the `compile_provider_binding` compilation pipeline. The compile pipeline is complex enough to warrant its own module.
- **convert.rs (747 lines)** has 16 public conversion functions re-exported from `lib.rs`. This is borderline but manageable.
- **Duplicate arky-codex dependency** in Cargo.toml (appears twice).

**Recommended refactoring:**

- Extract `compile_provider_binding` and `CompileError` from `lib.rs` into `compile.rs`.
- Fix duplicate `arky-codex` dependency in Cargo.toml.

**Priority**: MEDIUM -- the duplicate dependency needs fixing regardless.

---

### 15. openfang-agent-definition (1,692 lines) -- MEDIUM

Single `lib.rs` file with 1,692 lines. Contains:
- A `string_enum_with_unknown!` macro (lines 35-93)
- Agent definition types
- Validation/normalization pipeline
- Compilation from definition to provider binding

**Problems found:**

- **Single-file crate**: 1,692 lines in `lib.rs` with no module decomposition.
- The macro at the top is reusable but buried in this crate.

**Recommended refactoring:**

- Split into: `types.rs` (agent definition types), `validate.rs` (validation pipeline), `compile.rs` (compilation to ProviderBinding).
- Consider moving `string_enum_with_unknown!` macro to `openfang-types` or a shared macros crate if it's useful elsewhere.

**Priority**: MEDIUM -- not critical but could benefit from module decomposition.

---

### 16. openfang-skills (3,707 lines) -- LOW

| File | Lines | Issue |
|------|-------|-------|
| clawhub.rs | 910 | ClawHub marketplace client |
| openclaw_compat.rs | 707 | OpenClaw format compatibility |
| registry.rs | 578 | Skill registry |

Well-structured with 8 files. No issues found.

---

### 17. openfang-hands (2,083 lines) -- LOW

Three files: `registry.rs` (885), `lib.rs` (866), `bundled.rs` (332). `lib.rs` is heavy for a lib.rs (types + error enum + helpers) but not problematic at 866 lines.

---

### 18. openfang-extensions (2,878 lines) -- LOW

Well-decomposed into 7 files, none exceeding 658 lines. Clean module boundaries.

---

### 19. openfang-wire (1,946 lines) -- LOW

`peer.rs` (1,284 lines) handles TCP server, client, handshake, HMAC auth, and nonce tracking -- all cohesive OFP wire protocol concerns. The file is large but well-focused. No issues.

---

## Cross-Cutting Concerns

### A. Channel Config Duplication Between openfang-types and openfang-channels

The config structs live in `openfang-types/src/config.rs` but the adapter implementations live in `openfang-channels/src/*.rs`. When a channel is added, developers must modify:
1. `openfang-types/src/config.rs` -- add XxxConfig struct + Default + add field to ChannelsConfig + add validation in validate()
2. `openfang-channels/src/xxx.rs` -- add adapter implementation
3. `openfang-channels/src/lib.rs` -- re-export module
4. `openfang-channels/src/bridge.rs` -- wire up adapter creation

**Recommendation**: Move channel config structs closer to their adapter implementations. Either:
- (a) Move all channel configs to `openfang-channels` and re-export from `openfang-types` (preferred -- channels own their configs)
- (b) Use a trait-based registry where each adapter registers its config schema

### B. Error Type Proliferation

Each crate defines its own error enum:
- `arky-error`: `ClassifiedError` trait, `RuntimeError`
- `arky-provider`: `ProviderError`
- `arky-session`: `SessionError`
- `arky-hooks`: `HookError`
- `arky-config`: `ConfigError`
- `arky-mcp`: `McpError`
- `arky-tools`: `ToolError`
- `openfang-types`: `OpenFangError`
- `openfang-channels`: uses `Box<dyn Error>` (no typed errors)
- `openfang-extensions`: `ExtensionError`
- `openfang-hands`: `HandError`
- `openfang-skills`: `SkillError`
- `openfang-provider-binding`: `CompileError`, `BridgeError`, `AdapterError`, `ConvertError`, `InstantiateError`

The arky crates properly implement `ClassifiedError`. The openfang crates do not -- they use ad-hoc error enums without classification metadata (retryable, http_status, etc.).

**Recommendation**: Ensure all openfang error types implement the `ClassifiedError` trait from `arky-error` for consistent error handling.

### C. Crate Granularity Assessment

The arky-* layer has good granularity -- each crate has a clear, bounded responsibility:
- `arky-error` (bottom) -> `arky-protocol` -> `arky-tools`, `arky-session` -> `arky-provider` -> `arky-claude-code`, `arky-codex` -> `arky-config`

The openfang-* layer has some unnecessary granularity:
- **openfang-hands** (2,083 lines) and **openfang-skills** (3,707 lines) are similar in scope (pluggable capability packages). Could potentially merge into `openfang-plugins` or similar.
- **openfang-wire** (1,946 lines) is small enough that it could live inside `openfang-kernel` as a module, but keeping it separate avoids bloating the kernel crate. The current separation is reasonable.

**Recommendation**: No crate merges needed. The crate boundary decisions are sound. Focus on intra-crate file organization.

---

## Priority Matrix

| Priority | Target | LOC Affected | Effort | Impact |
|----------|--------|-------------|--------|--------|
| **CRITICAL** | openfang-types/config.rs split | 4,321 | Medium | Eliminates merge-conflict hotspot, reduces contributor friction |
| **CRITICAL** | openfang-channels adapter dedup | 29,074 | High | Eliminates ~15K lines of boilerplate, makes new channels trivial |
| **HIGH** | openfang-migrate/openclaw.rs split | 4,608 | Medium | Improves navigability of complex migration logic |
| **MEDIUM** | arky-config validate.rs/validation.rs merge | 982 | Low | Eliminates confusing dual-file pattern |
| **MEDIUM** | openfang-provider-binding lib.rs split | 907 | Low | Cleaner module boundaries |
| **MEDIUM** | openfang-agent-definition module decomposition | 1,692 | Low | Better separation of types/validation/compile |
| **LOW** | arky-claude-code provider.rs extraction | 1,541 | Low | Minor improvement |
| **LOW** | Error trait adoption in openfang crates | N/A | Medium | Consistency |

---

## Recommended Execution Order

1. **Phase 1 (config.rs split)**: Split `openfang-types/src/config.rs` into a `config/` module directory. This is the highest-leverage change -- a single file that every contributor touches.

2. **Phase 2 (channels dedup)**: Extract `AdapterBase` and `WebhookAdapterBase` into `openfang-channels/src/base.rs`. Refactor 5 adapters as proof of concept (telegram, discord, slack, line, viber). Then sweep through the remaining 37.

3. **Phase 3 (openclaw.rs split)**: Split into module directory. Straightforward mechanical refactoring.

4. **Phase 4 (minor cleanups)**: Fix dual validation in arky-config, split openfang-provider-binding lib.rs, decompose openfang-agent-definition.

5. **Phase 5 (error consistency)**: Add `ClassifiedError` impls to openfang-* error types.

---

## Dead Code Summary

| Crate | `#[allow(dead_code)]` Count | Notes |
|-------|---------------------------|-------|
| openfang-migrate/openclaw.rs | 10 | Legacy input struct fields that are parsed but never read |
| openfang-channels | 47 | Scattered across 20+ adapter files |
| openfang-wire/peer.rs | 3 | Minor |
| openfang-extensions/vault.rs | 1 | Minor |

**Total**: 61 `#[allow(dead_code)]` annotations across the analyzed crates. Each should be evaluated: either implement the usage or remove the field.

---

## Appendix: File Size Distribution

Files over 1,000 lines (requiring attention):

| File | Lines | Crate |
|------|-------|-------|
| openfang-migrate/src/openclaw.rs | 4,608 | openfang-migrate |
| openfang-types/src/config.rs | 4,321 | openfang-types |
| openfang-channels/src/bridge.rs | 1,981 | openfang-channels |
| openfang-channels/src/telegram.rs | 1,862 | openfang-channels |
| arky-config/src/loader.rs | 1,794 | arky-config |
| openfang-agent-definition/src/lib.rs | 1,692 | openfang-agent-definition |
| arky-claude-code/src/provider.rs | 1,541 | arky-claude-code |
| openfang-types/src/agent.rs | 1,325 | openfang-types |
| openfang-channels/src/feishu.rs | 1,295 | openfang-channels |
| openfang-wire/src/peer.rs | 1,284 | openfang-wire |
| openfang-provider-binding/src/bridge.rs | 1,209 | openfang-provider-binding |
| arky-claude-code/src/parser.rs | 1,196 | arky-claude-code |
| openfang-types/src/scheduler.rs | 1,183 | openfang-types |
| openfang-types/src/workflow.rs | 1,148 | openfang-types |
| arky-hooks/src/chain.rs | 1,144 | arky-hooks |
| openfang-provider-binding/src/adapter.rs | 1,126 | openfang-provider-binding |
| arky-codex/src/rpc.rs | 1,038 | arky-codex |
| arky-codex/src/provider.rs | 1,033 | arky-codex |
| arky-session/src/sqlite.rs | 1,022 | arky-session |
| openfang-types/src/task.rs | 1,008 | openfang-types |
| arky-config/src/layered.rs | 1,002 | arky-config |
