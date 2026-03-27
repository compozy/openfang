# OpenFang CLI/TUI Refactoring Analysis

**Crate:** `openfang-cli`
**Date:** 2026-03-27
**Total source:** 35,960 lines across 36 `.rs` files

---

## Executive Summary

The `openfang-cli` crate has grown into a 36k-line monolith. The single `main.rs` file
alone is **13,076 lines** -- roughly 10x what a well-structured Rust module should be. It
contains 329 functions spanning command definitions (clap), daemon communication, config
management, provider detection, onboarding wizards, and 100+ CLI command handlers. The TUI
layer (`tui/event.rs` at 2,786 lines) suffers from massive code duplication in its 60
`spawn_*` functions. These issues make the crate difficult to navigate, test, and extend.

This document catalogs every structural problem found and proposes a phased refactoring plan.

---

## 1. File-by-File Analysis

### 1.1 `main.rs` -- 13,076 lines (CRITICAL)

| Metric | Value |
|--------|-------|
| Functions | 329 |
| `cmd_*` handlers | ~160 |
| Subcommand enums | 22 |
| `std::process::exit` calls | 236 |
| `daemon_client()` calls | 131 |
| `daemon_json()` calls | 136 |
| `find_daemon()` calls | 29 |
| `eprintln!` calls | 60 |

**Problems:**

**P1. God file -- everything is in one module.**
The file mixes at least 7 distinct responsibilities:
1. **Clap definitions** (lines 1-1484): 22 subcommand enums, ~500 lines
2. **Tracing/boot setup** (lines 1485-1555): log initialization
3. **Main dispatch** (lines 1556-1933): 380-line match statement
4. **Daemon helpers** (lines 1935-2065): `find_daemon`, `daemon_client`, `daemon_json`
5. **Init/onboarding** (lines 2068-2341): `cmd_init`, provider detection, config writing
6. **Command handlers** (lines 2343-13076): 160+ `cmd_*` functions
7. **Utility functions** scattered throughout: `copy_to_clipboard`, `open_in_browser`,
   `prompt_input`, `copy_dir_recursive`, table printing helpers

**P2. Two duplicate home-directory functions.**
`cli_openfang_home()` (line 1520) and `openfang_home()` (line 10548) do the same thing
but differ only in their `unwrap` vs `exit` behavior on failure. Both are called throughout
the file.

```rust
// Line 1520 -- returns temp_dir on failure
fn cli_openfang_home() -> std::path::PathBuf {
    dirs::home_dir().unwrap_or_else(std::env::temp_dir).join(".openfang")
}

// Line 10548 -- exits on failure
pub(crate) fn openfang_home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| {
        eprintln!("Error: Could not determine home directory");
        std::process::exit(1);
    }).join(".openfang")
}
```

**P3. Repetitive daemon-or-inprocess branching pattern.**
Roughly 40+ `cmd_*` functions follow an identical pattern:

```rust
fn cmd_something(config: Option<PathBuf>, json: bool) {
    if let Some(base) = find_daemon() {
        let client = daemon_client();
        let body = daemon_json(client.get(format!("{base}/api/endpoint")).send());
        if json { print_json_value(&body); return; }
        // ... format output ...
    } else {
        let kernel = boot_kernel(config);
        // ... same logic via kernel ...
    }
}
```

This pattern is never abstracted. Each command re-discovers the daemon, rebuilds the client,
and re-implements the fallback path.

**P4. The `cmd_doctor` function is 925 lines long (lines 2919-3844).**
It performs 12 separate checks with deeply nested `if json`/`if repair` branching. Each check
duplicates the JSON-output and repair-prompt logic.

**P5. Provider list is defined multiple times.**
- `provider_list()` at line 2277 returns a `Vec` of 6 providers
- `provider_keys` array at line 3304 lists 10 providers
- `PROVIDERS` const in `init_wizard.rs` at line 30 lists 22 providers
- `cmd_channel_setup` at line 9375 has its own inline provider/channel catalog

Each has a different subset and different struct shapes, making them impossible to keep in sync.

**P6. Massive inline `cmd_channel_setup` function at ~310 lines (lines 9375-9688).**
Contains an interactive wizard with 5+ nested loops, building channel-specific configuration
UI inline rather than via a shared wizard abstraction.

**P7. `cmd_quick_chat` at ~420 lines (lines 10540-10960)** embeds a full TUI chat loop
inline, duplicating logic already in `tui::chat_runner`.

**P8. 236 calls to `std::process::exit(1)`.**
Error handling is done via scattered `exit(1)` calls rather than Result-based propagation. This
makes the code untestable and prevents proper cleanup.

---

### 1.2 `tui/event.rs` -- 2,786 lines (HIGH)

| Metric | Value |
|--------|-------|
| `spawn_*` functions | 60 |
| `std::thread::spawn` calls | 60 |
| HTTP client builder calls | 21 |
| `AppEvent` variants | 78 |

**Problems:**

**P9. Extreme code duplication in spawn functions.**
Nearly every `spawn_*` function follows the exact same skeleton:

```rust
pub fn spawn_fetch_X(backend: BackendRef, tx: mpsc::Sender<AppEvent>) {
    std::thread::spawn(move || match backend {
        BackendRef::Daemon(base_url) => {
            let client = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap_or_else(|_| reqwest::blocking::Client::new());
            if let Ok(resp) = client.get(format!("{base_url}/api/X")).send() {
                if let Ok(body) = resp.json::<serde_json::Value>() {
                    // ... parse fields ...
                    let _ = tx.send(AppEvent::XLoaded(result));
                }
            }
        }
        BackendRef::InProcess(_) => {
            let _ = tx.send(AppEvent::XLoaded(Vec::new()));
        }
    });
}
```

The HTTP client construction (5 lines) is copy-pasted 21 times. The thread-spawn + backend
match boilerplate appears 60 times. This could be reduced to a single generic helper:

```rust
fn spawn_backend_task<F, T>(backend: BackendRef, tx: mpsc::Sender<AppEvent>, f: F)
where
    F: FnOnce(&BackendRef) -> T + Send + 'static,
    T: Into<AppEvent>,
```

**P10. `AppEvent` enum has 78 variants.**
This monolithic enum is the sole communication channel between background threads and the UI.
Many variants carry identical payloads (`Vec<SomeInfo>`). It should be split into domain-
specific sub-enums or use a generic `DataLoaded { tab: Tab, data: Box<dyn Any> }` pattern.

**P11. JSON deserialization is manual everywhere.**
Rather than defining serde `Deserialize` on the `FooInfo` types and using
`resp.json::<Vec<FooInfo>>()`, every spawn function manually extracts fields with
`body["field"].as_str().unwrap_or("")`. This is fragile, verbose, and duplicated 60 times.

---

### 1.3 `tui/mod.rs` -- 2,426 lines (HIGH)

| Metric | Value |
|--------|-------|
| Screen state fields on `App` | 20 |
| `handle_*_action` methods | 19 |
| `refresh_*` methods | 18 |

**Problems:**

**P12. `App` struct is a god object.**
It holds state for all 19 screens, the backend connection, chat target, kernel boot status,
and Ctrl+C tracking. It has 30+ fields and implements all event routing, all action handling,
all refresh triggering, and all rendering dispatch.

**P13. `handle_event` method (lines 226-610) is 384 lines.**
It's a single `match` over 78 `AppEvent` variants, each arm directly mutating screen state.
This violates separation of concerns -- the central coordinator knows the internal state
shape of every screen.

**P14. `handle_key` method (lines 612-882) is 270 lines.**
Contains tab-switching logic (F1-F12, Alt+1-9, Ctrl+arrows, Tab/BackTab) that is largely
a lookup table but is written as cascading match arms.

**P15. The 19 `handle_*_action` methods (lines 1285-1730) are repetitive.**
Most follow the same pattern: match action variant, call `self.refresh_*()` or spawn a
background task. This could be driven by a trait on each screen's action type.

**P16. The 18 `refresh_*` methods (lines 983-1135) are cookie-cutter.**
Every one does:
```rust
fn refresh_foo(&mut self) {
    if let Some(backend) = self.backend.to_ref() {
        self.foo.loading = true;
        event::spawn_fetch_foo(backend, self.event_tx.clone());
    }
}
```

---

### 1.4 `tui/screens/init_wizard.rs` -- 2,346 lines (MEDIUM)

**Problems:**

**P17. Standalone event loop duplicates the TUI harness.**
The init wizard runs its own `ratatui::init()`, event loop, and terminal restore -- separate
from the main TUI `run()` function. This means two independent TUI harnesses in the codebase.

**P18. Provider list duplicated.**
The `PROVIDERS` const (22 entries) duplicates data from `main.rs::provider_list()` and
`main.rs::provider_keys` with a different struct shape (`ProviderInfo` vs tuples).

**P19. 6-step wizard state machine is a single function.**
Key handling, state transitions, config writing, and rendering are all in one file with no
sub-module decomposition.

---

### 1.5 `tui/screens/agents.rs` -- 1,532 lines (MEDIUM)

**Problems:**

**P20. `AgentSelectState` struct has 18 fields.**
It manages the agent list, search, detail view, creation wizard (5 steps), skill editor,
and MCP editor. These are at least 4 independent sub-features crammed into one struct.

**P21. `handle_key` function is ~400 lines.**
Routes through 12 `AgentSubScreen` variants with deeply nested key-matching logic.

---

### 1.6 `tui/screens/channels.rs` -- 944 lines (MEDIUM)

**P22. Inline channel metadata catalog.**
Channel names, env vars, and categories are hardcoded in `build_default_channels()` rather
than loaded from a shared registry.

---

### 1.7 `tui/chat_runner.rs` -- 805 lines (LOW)

**P23. Duplicates ~50% of `tui/mod.rs` chat logic.**
Contains its own event loop, daemon/in-process detection, streaming handling, and slash command
processing. The `run_chat_tui` function is a mini-TUI on its own that could share infrastructure
with the main TUI.

---

### 1.8 Other screen files (LOW individually)

| File | Lines | Notes |
|------|-------|-------|
| `comms.rs` | 762 | Contains its own topology graph rendering |
| `workflows.rs` | 705 | Clean but large |
| `wizard.rs` | 685 | Provider wizard, overlaps with init_wizard |
| `skills.rs` | 630 | Tab management within tabs |
| `settings.rs` | 622 | Manages providers, models, tools in sub-tabs |
| `extensions.rs` | 589 | Browse + health sub-views |
| `triggers.rs` | 557 | Form-based creation flow |
| `memory.rs` | 557 | Agent selector + KV editor |

Most screens follow a consistent pattern (`State`, `Action` enum, `handle_key`, `draw`). This
is a good foundation for a trait-based abstraction.

---

### 1.9 Support files (LOW)

| File | Lines | Notes |
|------|-------|-------|
| `launcher.rs` | 604 | Standalone ratatui launcher menu |
| `mcp.rs` | 439 | MCP stdio server |
| `progress.rs` | 322 | Progress bar widgets |
| `dotenv.rs` | 249 | .env file loader |
| `table.rs` | 248 | Table formatting utilities |
| `templates.rs` | 137 | Agent template loader |
| `bundled_agents.rs` | 134 | Embedded agent TOML files |
| `theme.rs` | 139 | Color/style constants |
| `ui.rs` | 122 | CLI output helpers |

These are well-sized and focused. No refactoring needed.

---

## 2. Duplicated Code Patterns

### 2.1 HTTP Client Construction (21 occurrences in event.rs)

```rust
let client = reqwest::blocking::Client::builder()
    .timeout(Duration::from_secs(5))
    .build()
    .unwrap_or_else(|_| reqwest::blocking::Client::new());
```

**Fix:** Single `fn make_client(timeout_secs: u64) -> reqwest::blocking::Client` helper.

### 2.2 Daemon-or-Kernel Branching (40+ occurrences in main.rs)

```rust
if let Some(base) = find_daemon() {
    let client = daemon_client();
    let body = daemon_json(client.get(...).send());
    ...
} else {
    let kernel = boot_kernel(config);
    ...
}
```

**Fix:** Introduce a `Backend` enum or trait with `fn request(&self, path, method) -> Value`.

### 2.3 Spawn-Thread-Match-Backend (60 occurrences in event.rs)

```rust
pub fn spawn_fetch_X(backend: BackendRef, tx: mpsc::Sender<AppEvent>) {
    std::thread::spawn(move || match backend {
        BackendRef::Daemon(url) => { ... }
        BackendRef::InProcess(kernel) => { ... }
    });
}
```

**Fix:** Generic `spawn_bg` function that takes a closure.

### 2.4 JSON Field Extraction (hundreds of occurrences)

```rust
body["field"].as_str().unwrap_or("?").to_string()
```

**Fix:** Derive `serde::Deserialize` on the target types and use `resp.json::<T>()`.

### 2.5 Provider/Model Metadata (4 separate definitions)

Defined in `main.rs::provider_list()`, `main.rs::provider_keys`, `init_wizard::PROVIDERS`,
and `cmd_channel_setup`. All have different struct types and different subsets.

**Fix:** Single `providers.rs` module with one canonical `ProviderInfo` struct and const array.

---

## 3. Recommended Module Structure

### Target layout for `src/`:

```
src/
  main.rs                    (~200 lines: clap parse + dispatch only)
  cli.rs                     (Cli, Commands, all subcommand enums)
  daemon.rs                  (find_daemon, daemon_client, daemon_json, daemon_watch_client)
  providers.rs               (canonical provider list, test_api_key)
  home.rs                    (openfang_home, restrict_*_permissions, read_api_key)
  commands/
    mod.rs                   (re-exports)
    init.rs                  (cmd_init, cmd_init_quick, cmd_init_interactive)
    start_stop.rs            (cmd_start, cmd_stop, start_daemon_background)
    agent.rs                 (cmd_agent_*)
    workflow.rs              (cmd_workflow_*)
    task.rs                  (cmd_task_*, cmd_subtask_*)
    run.rs                   (cmd_run_*)
    dispatch.rs              (cmd_dispatch_*)
    hitl.rs                  (cmd_hitl_*)
    looper.rs                (cmd_looper_*)
    event.rs                 (cmd_event_*)
    artifact_doc.rs           (cmd_artifact_*, cmd_doc_*)
    pack.rs                  (cmd_pack_*)
    trigger.rs               (cmd_trigger_*)
    config.rs                (cmd_config_*)
    model.rs                 (cmd_models_*)
    skill.rs                 (cmd_skill_*)
    channel.rs               (cmd_channel_*)
    hand.rs                  (cmd_hand_*)
    budget.rs                (cmd_budget_*)
    a2a.rs                   (cmd_a2a_*)
    peers.rs                 (cmd_peers_*)
    misc.rs                  (doctor, status, health, sessions, logs, etc.)
    system.rs                (cmd_system_*, cmd_reset, cmd_uninstall)
    integration.rs           (cmd_integration_*, cmd_vault_*)
  helpers.rs                 (prompt_input, copy_to_clipboard, open_in_browser, etc.)
  tui/
    mod.rs                   (~400 lines: App struct, run(), draw dispatch)
    app_state.rs             (App field definitions, constructor)
    event.rs                 (~500 lines: AppEvent, spawn_event_thread)
    backend.rs               (BackendRef, Backend, spawn_bg, make_client)
    fetch.rs                 (spawn_fetch_* consolidated with generic helpers)
    actions.rs               (handle_*_action consolidated)
    tab_bar.rs               (tab bar rendering)
    screen_trait.rs           (Screen trait: handle_key, draw, tick, on_enter)
    chat_runner.rs
    theme.rs
    screens/
      (unchanged -- these are already well-structured)
```

---

## 4. Prioritized Refactoring Plan

### Phase 1: Split `main.rs` (CRITICAL, ~2 days)

| Task | Lines moved | Risk |
|------|-------------|------|
| Extract clap definitions to `cli.rs` | ~1,500 | Low |
| Extract daemon helpers to `daemon.rs` | ~150 | Low |
| Extract home dir / permissions to `home.rs` | ~80 | Low |
| Extract provider catalog to `providers.rs` | ~100 | Low |
| Extract helpers to `helpers.rs` | ~200 | Low |
| Split `cmd_*` handlers into `commands/` modules by domain | ~9,000 | Medium |

After Phase 1, `main.rs` should be ~200 lines: parse CLI, dispatch to command modules.

### Phase 2: Consolidate `tui/event.rs` (HIGH, ~1 day)

| Task | Lines saved | Risk |
|------|-------------|------|
| Create `backend.rs` with `make_client()` and `spawn_bg()` | saves ~500 | Low |
| Derive `Deserialize` on screen info types, replace manual parsing | saves ~800 | Medium |
| Split `AppEvent` into domain-specific sub-enums | neutral | Medium |

After Phase 2, `event.rs` should be ~800 lines.

### Phase 3: Slim down `tui/mod.rs` (HIGH, ~1 day)

| Task | Lines saved | Risk |
|------|-------------|------|
| Extract `handle_event` into `actions.rs` | saves ~400 | Low |
| Extract tab bar to `tab_bar.rs` | saves ~120 | Low |
| Consolidate `refresh_*` with a generic refresh dispatcher | saves ~150 | Low |
| Move `App` field definitions to `app_state.rs` | neutral | Low |

After Phase 3, `tui/mod.rs` should be ~800 lines.

### Phase 4: Deduplicate provider data (MEDIUM, ~0.5 days)

| Task | Impact |
|------|--------|
| Create single `ProviderInfo` struct in `providers.rs` | Single source of truth |
| Update `init_wizard.rs` to use shared `PROVIDERS` | Removes 200 lines of duplication |
| Update `cmd_doctor` to use shared list | Consistency |

### Phase 5: Introduce Screen trait (MEDIUM, ~1 day)

| Task | Impact |
|------|--------|
| Define `trait Screen { fn handle_key(); fn draw(); fn tick(); fn on_enter(); }` | Polymorphic dispatch |
| Implement for all 19 screens | Eliminates 19 match arms in `draw()`, `handle_key()`, `handle_tick()` |
| Replace `handle_*_action` methods with trait method | Eliminates 19 boilerplate methods |

### Phase 6: Merge `chat_runner.rs` into main TUI (LOW, ~0.5 days)

| Task | Impact |
|------|--------|
| Make `openfang chat` launch the main TUI directly to the Chat tab | Removes 805 lines of duplication |
| Remove standalone chat event loop | Single TUI harness |

---

## 5. Anti-Patterns Found

### 5.1 Exit-on-error pattern (236 occurrences)

```rust
// BAD: untestable, prevents cleanup
fn cmd_foo() {
    let x = something().unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        std::process::exit(1);
    });
}
```

**Better:** Return `Result<(), CliError>` and let `main()` handle the exit code.

### 5.2 God enum (`AppEvent` with 78 variants)

Every background operation communicates through a single channel with a flat enum. This makes
it impossible to reason about which events a specific screen cares about.

### 5.3 Stringly-typed JSON field access

```rust
// BAD: no compile-time checking, silent failures
body["agent_count"].as_u64().unwrap_or(0)
```

**Better:** Define typed response structs with `#[derive(Deserialize)]`.

### 5.4 Thread-per-request model

Every background fetch spawns a new OS thread. For 19 tabs that each refresh on entry, this
means 19+ short-lived threads. A thread pool or async runtime would be more efficient.

### 5.5 No separation between CLI and TUI concerns

`main.rs` mixes CLI-only code (output formatting, prompts) with shared logic (daemon
detection, kernel booting). The TUI modules import from `crate::` to reach `daemon_client()`
and `find_daemon()` -- these should be in a shared module, not in `main.rs`.

---

## 6. Quantified Impact

| Metric | Current | After Refactoring |
|--------|---------|-------------------|
| `main.rs` lines | 13,076 | ~200 |
| Largest file | 13,076 | ~1,500 (init_wizard) |
| `event.rs` lines | 2,786 | ~800 |
| `tui/mod.rs` lines | 2,426 | ~800 |
| Duplicate HTTP client builders | 21 | 1 |
| Duplicate provider definitions | 4 | 1 |
| Duplicate home-dir functions | 2 | 1 |
| Duplicate spawn patterns | 60 | ~10 (via generic) |
| `std::process::exit` calls | 236 | ~5 (main + clap) |
| Total files | 36 | ~55 |
| Total lines (estimated) | 35,960 | ~30,000 |

---

## 7. Risk Assessment

| Risk | Mitigation |
|------|------------|
| Breakage during moves | Pure file splits with `pub use` re-exports, no logic changes |
| Git blame disruption | One commit per phase, clear commit messages |
| Merge conflicts with active TUI work | Phase 1 (main.rs) is safest; Phase 5 touches screens |
| Visibility changes | Use `pub(crate)` consistently; integration tests catch regressions |
| Circular dependencies | `commands/` depends on `daemon.rs` and `home.rs`, never the reverse |

---

## 8. Files Not Requiring Refactoring

The following files are well-sized and well-focused:

- `bundled_agents.rs` (134 lines)
- `dotenv.rs` (249 lines)
- `launcher.rs` (604 lines)
- `mcp.rs` (439 lines)
- `progress.rs` (322 lines)
- `table.rs` (248 lines)
- `templates.rs` (137 lines)
- `tui/theme.rs` (139 lines)
- `tui/screens/mod.rs` (22 lines)
- `ui.rs` (122 lines)
- All individual screen files under `tui/screens/` (each 200-950 lines, well-structured)
