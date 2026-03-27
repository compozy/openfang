# Technical Specification: PRD-CLI — Compozy Features in the OpenFang CLI

## Executive Summary

The PRD-Go delivered 12 major features (tasks 32-43) to the OpenFang backend, exposing 100+ new `/api/v1/` endpoints. None of these features are accessible via the CLI or TUI. This spec defines the implementation plan to add **8 new CLI command groups** and **upgrade 1 existing group** so that every Compozy feature is first-class in the terminal.

The implementation follows the existing CLI patterns: clap-derived subcommand enums, `reqwest::blocking` HTTP calls through `daemon_client()`, `--json` flag for machine output, and manual columnar formatting for human output. No new dependencies are required.

## System Architecture

### Domain Placement

All changes live in a single crate:

- `crates/openfang-cli/src/main.rs` — Command enums, dispatch match, handler functions
- `crates/openfang-cli/src/table.rs` — Table rendering (existing, reused)
- `crates/openfang-cli/src/ui.rs` — UI helpers (existing, reused)
- `crates/openfang-cli/src/progress.rs` — Spinners for SSE watch (existing, reused)

No new files or crates are created. All commands follow the same pattern as existing `workflow`, `trigger`, `cron`, and `agent` commands.

### Component Overview

```
User
  │
  ▼
openfang CLI (clap dispatch)
  │
  ▼
daemon_client() ──► reqwest::blocking::Client
  │                     │
  ▼                     ▼
require_daemon()   HTTP {GET,POST,PUT,DELETE}
  │                     │
  ▼                     ▼
daemon_json()      /api/v1/* endpoints (openfang-api)
  │
  ▼
stdout (columnar / JSON)
```

Each new command group follows this exact flow. SSE watch commands add a streaming loop on top.

## Implementation Design

### New Command Groups

#### 1. `openfang task` — Task & Subtask Management (PRD Task 32)

```
openfang task list [--status <s>] [--priority <p>] [--limit <n>] [--json]
openfang task get <task_id> [--json]
openfang task create <file>
openfang task update <task_id> <file>
openfang task delete <task_id>
openfang task replan <task_id> <file>
openfang task subtasks <task_id> [--status <s>] [--json]
openfang task artifacts <task_id> [--json]
openfang task docs <task_id> [--json]
```

**Subcommand Enum:**

```rust
#[derive(Subcommand)]
enum TaskCommands {
    /// List tasks with optional filters.
    List {
        #[arg(long)] status: Option<String>,
        #[arg(long)] priority: Option<String>,
        #[arg(long, default_value = "50")] limit: u32,
        #[arg(long)] json: bool,
    },
    /// Get task details by ID.
    Get {
        task_id: String,
        #[arg(long)] json: bool,
    },
    /// Create a task from a JSON file.
    Create {
        #[arg(value_name = "FILE")] file: PathBuf,
    },
    /// Update a task from a JSON file.
    Update {
        task_id: String,
        #[arg(value_name = "FILE")] file: PathBuf,
    },
    /// Delete a task.
    Delete { task_id: String },
    /// Replan a task (atomic subtask bulk operation).
    Replan {
        task_id: String,
        #[arg(value_name = "FILE")] file: PathBuf,
    },
    /// List subtasks for a task.
    Subtasks {
        task_id: String,
        #[arg(long)] status: Option<String>,
        #[arg(long)] json: bool,
    },
    /// List artifacts linked to a task.
    Artifacts {
        task_id: String,
        #[arg(long)] json: bool,
    },
    /// List docs linked to a task.
    Docs {
        task_id: String,
        #[arg(long)] json: bool,
    },
}
```

**API Mapping:**

| Subcommand | Method | Endpoint |
|------------|--------|----------|
| `list` | GET | `/api/v1/tasks?status=&priority=&limit=` |
| `get` | GET | `/api/v1/tasks/{id}` |
| `create` | POST | `/api/v1/tasks` |
| `update` | PUT | `/api/v1/tasks/{id}` |
| `delete` | DELETE | `/api/v1/tasks/{id}` |
| `replan` | POST | `/api/v1/tasks/{id}/replan` |
| `subtasks` | GET | `/api/v1/tasks/{id}/subtasks` |
| `artifacts` | GET | `/api/v1/tasks/{id}/artifacts` |
| `docs` | GET | `/api/v1/tasks/{id}/docs` |

**List Table Columns:** `ID | TITLE | STATUS | PRIORITY | OWNER | CREATED`

---

#### 2. `openfang subtask` — Standalone Subtask Operations (PRD Task 32)

```
openfang subtask list [--task_id <id>] [--status <s>] [--json]
openfang subtask get <subtask_id> [--json]
openfang subtask create <task_id> <file>
openfang subtask update <subtask_id> <file>
openfang subtask delete <subtask_id>
```

**API Mapping:**

| Subcommand | Method | Endpoint |
|------------|--------|----------|
| `list` | GET | `/api/v1/subtasks?task_id=&status=` |
| `get` | GET | `/api/v1/subtasks/{id}` |
| `create` | POST | `/api/v1/tasks/{task_id}/subtasks` |
| `update` | PUT | `/api/v1/subtasks/{id}` |
| `delete` | DELETE | `/api/v1/subtasks/{id}` |

**List Table Columns:** `ID | TASK_ID | TITLE | STATUS | KIND | ASSIGNEE`

---

#### 3. `openfang dispatch` — Dispatch Control Plane (PRD Task 33)

```
openfang dispatch list [--run_id <id>] [--status <s>] [--json]
openfang dispatch get <dispatch_id> [--json]
openfang dispatch children <dispatch_id> [--json]
openfang dispatch retry <dispatch_id>
openfang dispatch cancel <dispatch_id>
openfang dispatch watch <dispatch_id>
```

**API Mapping:**

| Subcommand | Method | Endpoint |
|------------|--------|----------|
| `list` | GET | `/api/v1/dispatches?run_id=&status=` |
| `get` | GET | `/api/v1/dispatches/{id}` |
| `children` | GET | `/api/v1/dispatches/{id}/children` |
| `retry` | POST | `/api/v1/dispatches/{id}/retry` |
| `cancel` | POST | `/api/v1/dispatches/{id}/cancel` |
| `watch` | GET | `/api/v1/dispatches/{id}/events` (SSE) |

**List Table Columns:** `ID | RUN_ID | STEP | KIND | TARGET | STATUS | UPDATED`

---

#### 4. `openfang hitl` — Human-in-the-Loop Requests (PRD Task 33)

```
openfang hitl list [--run_id <id>] [--status <s>] [--json]
openfang hitl get <hitl_id> [--json]
openfang hitl answer <hitl_id> <response>
openfang hitl cancel <hitl_id>
openfang hitl watch
```

**API Mapping:**

| Subcommand | Method | Endpoint |
|------------|--------|----------|
| `list` | GET | `/api/v1/hitl-requests?run_id=&status=` |
| `get` | GET | `/api/v1/hitl-requests/{id}` |
| `answer` | POST | `/api/v1/hitl-requests/{id}/answer` |
| `cancel` | POST | `/api/v1/hitl-requests/{id}/cancel` |
| `watch` | GET | `/api/v1/hitl-requests/stream` (SSE) |

**List Table Columns:** `ID | RUN_ID | KIND | STATUS | QUESTION | CREATED`

---

#### 5. `openfang looper` — Looper Run Management (PRD Tasks 34, 39)

```
openfang looper list [--task_id <id>] [--status <s>] [--json]
openfang looper get <looper_id> [--json]
openfang looper create <file>
openfang looper subtasks <looper_id> [--json]
openfang looper pause <looper_id>
openfang looper resume <looper_id>
openfang looper cancel <looper_id>
openfang looper watch <looper_id>
```

**API Mapping:**

| Subcommand | Method | Endpoint |
|------------|--------|----------|
| `list` | GET | `/api/v1/looper-runs?task_id=&status=` |
| `get` | GET | `/api/v1/looper-runs/{id}` |
| `create` | POST | `/api/v1/looper-runs` |
| `subtasks` | GET | `/api/v1/looper-runs/{id}/subtasks` |
| `pause` | POST | `/api/v1/looper-runs/{id}/pause` |
| `resume` | POST | `/api/v1/looper-runs/{id}/resume` |
| `cancel` | POST | `/api/v1/looper-runs/{id}/cancel` |
| `watch` | GET | `/api/v1/looper-runs/{id}/events` (SSE) |

**List Table Columns:** `ID | TASK_ID | STATUS | MODE | PROGRESS | UPDATED`

---

#### 6. `openfang event` — Event Ingress (PRD Task 36)

```
openfang event send <file>
openfang event dry-run <file>
```

**API Mapping:**

| Subcommand | Method | Endpoint |
|------------|--------|----------|
| `send` | POST | `/api/v1/events` |
| `dry-run` | POST | `/api/v1/events/dry-run` |

---

#### 7. `openfang artifact` — Artifact Browsing (PRD Tasks 37, 38)

```
openfang artifact list [--type <t>] [--task_id <id>] [--json]
openfang artifact get <artifact_id> [--json]
openfang artifact versions <artifact_id> [--json]
```

**API Mapping:**

| Subcommand | Method | Endpoint |
|------------|--------|----------|
| `list` | GET | `/api/v1/artifacts?artifact_type=&task_id=` |
| `get` | GET | `/api/v1/artifacts/{id}` |
| `versions` | GET | `/api/v1/artifacts/{id}/versions` |

**List Table Columns:** `ID | TYPE | TITLE | VERSION | TASK_ID | CREATED`

---

#### 8. `openfang doc` — Doc Browsing (PRD Tasks 37, 38)

```
openfang doc list [--type <t>] [--task_id <id>] [--json]
openfang doc get <doc_id> [--json]
openfang doc versions <doc_id> [--json]
```

**API Mapping:**

| Subcommand | Method | Endpoint |
|------------|--------|----------|
| `list` | GET | `/api/v1/docs?doc_type=&task_id=` |
| `get` | GET | `/api/v1/docs/{id}` |
| `versions` | GET | `/api/v1/docs/{id}/versions` |

**List Table Columns:** `ID | TYPE | TITLE | VERSION | TASK_ID | CREATED`

---

#### 9. `openfang pack` — Pack Management (PRD Tasks 40, 41)

```
openfang pack list [--json]
openfang pack get <pack_id> [--json]
openfang pack objects <pack_id> [--json]
openfang pack install <source>
openfang pack upgrade <pack_id> [--dry-run]
openfang pack uninstall <pack_id>
openfang pack fork <pack_id>
```

**API Mapping:**

| Subcommand | Method | Endpoint |
|------------|--------|----------|
| `list` | GET | `/api/v1/packs` |
| `get` | GET | `/api/v1/packs/{id}` |
| `objects` | GET | `/api/v1/packs/{id}/objects` |
| `install` | POST | `/api/v1/packs/install` |
| `upgrade` | POST | `/api/v1/packs/{id}/upgrade` |
| `upgrade --dry-run` | POST | `/api/v1/packs/{id}/upgrade/dry-run` |
| `uninstall` | POST | `/api/v1/packs/{id}/uninstall` |
| `fork` | POST | `/api/v1/packs/{id}/fork` |

**List Table Columns:** `ID | NAME | VERSION | SOURCE | OBJECTS | INSTALLED`

---

#### 10. `openfang run` — Workflow Run Control (PRD Tasks 39, 42)

```
openfang run list [--workflow_id <id>] [--status <s>] [--json]
openfang run get <run_id> [--json]
openfang run dispatches <run_id> [--json]
openfang run hitl <run_id> [--json]
openfang run signals <run_id> [--json]
openfang run signal <run_id> <name> <payload_json>
openfang run checkpoints <run_id> [--json]
openfang run pause <run_id>
openfang run resume <run_id>
openfang run cancel <run_id>
openfang run watch <run_id>
```

**API Mapping:**

| Subcommand | Method | Endpoint |
|------------|--------|----------|
| `list` | GET | `/api/v1/runs?workflow_id=&status=` |
| `get` | GET | `/api/v1/runs/{id}` |
| `dispatches` | GET | `/api/v1/runs/{id}/dispatches` |
| `hitl` | GET | `/api/v1/runs/{id}/hitl-requests` |
| `signals` | GET | `/api/v1/runs/{id}/signals` |
| `signal` | POST | `/api/v1/runs/{id}/signals` |
| `checkpoints` | GET | `/api/v1/runs/{id}/checkpoints` |
| `pause` | POST | `/api/v1/runs/{id}/pause` |
| `resume` | POST | `/api/v1/runs/{id}/resume` |
| `cancel` | POST | `/api/v1/runs/{id}/cancel` |
| `watch` | GET | `/api/v1/runs/{id}/events` (SSE) |

**List Table Columns:** `ID | WORKFLOW | STATUS | STEPS | STARTED | UPDATED`

---

#### 11. Upgrade `openfang trigger` — Trigger v2 Features (PRD Task 35)

Existing trigger commands remain. New subcommands added:

```
openfang trigger get <trigger_id> [--json]        # NEW
openfang trigger update <trigger_id> <file>        # NEW
openfang trigger enable <trigger_id>               # NEW
openfang trigger disable <trigger_id>              # NEW
openfang trigger test <trigger_id> <event_json>    # NEW
openfang trigger fork <trigger_id>                 # NEW
openfang trigger validate <file>                   # NEW
openfang trigger compile <file>                    # NEW
openfang trigger runtime <trigger_id> [--json]     # NEW
```

**API Mapping:**

| Subcommand | Method | Endpoint |
|------------|--------|----------|
| `get` | GET | `/api/v1/triggers/{id}` |
| `update` | PUT | `/api/v1/triggers/{id}` |
| `enable` | POST | `/api/v1/triggers/{id}/enable` |
| `disable` | POST | `/api/v1/triggers/{id}/disable` |
| `test` | POST | `/api/v1/triggers/{id}/test` |
| `fork` | POST | `/api/v1/triggers/{id}/fork` |
| `validate` | POST | `/api/v1/triggers/validate` |
| `compile` | POST | `/api/v1/triggers/compile` |
| `runtime` | GET | `/api/v1/triggers/{id}/runtime` |

---

### Core Interfaces

#### Handler Pattern (all commands follow this)

```rust
fn cmd_task_list(status: Option<&str>, priority: Option<&str>, limit: u32, json: bool) {
    let base = require_daemon("task list");
    let client = daemon_client();

    let mut url = format!("{base}/api/v1/tasks?limit={limit}");
    if let Some(s) = status { url.push_str(&format!("&status={s}")); }
    if let Some(p) = priority { url.push_str(&format!("&priority={p}")); }

    let body = daemon_json(client.get(&url).send());

    if json {
        println!("{}", serde_json::to_string_pretty(&body).unwrap_or_default());
        return;
    }

    // ... columnar output
}
```

#### SSE Watch Pattern (new, for `watch` subcommands)

```rust
fn cmd_run_watch(run_id: &str) {
    let base = require_daemon("run watch");
    let client = daemon_client();
    let mut spinner = progress::Spinner::new(&format!("Watching run {run_id}..."));

    let resp = client
        .get(format!("{base}/api/v1/runs/{run_id}/events"))
        .header("Accept", "text/event-stream")
        .send();

    match resp {
        Ok(r) => {
            spinner.finish();
            let reader = std::io::BufReader::new(r);
            for line in reader.lines().map_while(Result::ok) {
                if line.starts_with("data:") {
                    let data = &line[5..].trim();
                    if let Ok(evt) = serde_json::from_str::<serde_json::Value>(data) {
                        // Pretty-print event
                        println!("[{}] {}: {}",
                            evt["timestamp"].as_str().unwrap_or("?"),
                            evt["kind"].as_str().unwrap_or("?"),
                            evt["summary"].as_str().unwrap_or(""),
                        );
                    }
                }
            }
        }
        Err(e) => {
            spinner.finish();
            ui::error(&format!("SSE connection failed: {e}"));
            std::process::exit(1);
        }
    }
}
```

### Data Models

No new Rust types are needed in the CLI. All request bodies are read from JSON files (`serde_json::Value`) and all responses are parsed as `serde_json::Value` via the existing `daemon_json()` helper. This matches the pattern used by `cmd_workflow_create`, `cmd_workflow_update`, etc.

### API Endpoints

All endpoints are already implemented and registered in `crates/openfang-api/src/server.rs`. See the API Mapping tables above for the complete list. The CLI is purely a consumer.

## Impact Analysis

| Affected Component | Type of Impact | Description & Risk Level | Required Action |
|---|---|---|---|
| `crates/openfang-cli/src/main.rs` | Code addition | Add ~10 new enum variants + ~65 handler functions. Low risk (additive only). | None |
| `Commands` enum | Code addition | Add 10 new `#[command(subcommand)]` variants. Low risk. | None |
| `TriggerCommands` enum | Code modification | Add 9 new variants to existing enum. Low risk. | Verify existing tests pass |
| CLI `--help` output | UX change | 10 new command groups visible in help. Low risk. | Verify help formatting |
| Shell completions | Auto-generated | New commands appear in completions. No manual work. | Regenerate completions |

No backend changes. No database changes. No new dependencies. No breaking changes.

## Testing Approach

### Unit Tests

No unit tests for CLI handlers (consistent with existing codebase pattern). The CLI commands are thin HTTP wrappers and are validated via integration tests.

### Integration Tests

For each new command group, verify:

1. **`--help` renders** — `openfang task --help` exits 0 and shows all subcommands
2. **Without daemon** — Commands print "requires a running daemon" and exit 1
3. **With daemon** — Commands hit correct endpoints and parse responses
4. **`--json` flag** — Returns valid JSON to stdout
5. **Error handling** — API errors (404, 422, 500) display helpful messages

**SSE watch commands** (dispatch watch, hitl watch, looper watch, run watch):
- Verify connection establishment
- Verify graceful disconnect on Ctrl+C
- Verify `Last-Event-ID` replay header

### Smoke Test Script

After implementation, run against a live daemon:

```bash
# Tasks
openfang task list
openfang task list --json

# Dispatches
openfang dispatch list
openfang dispatch list --run_id <id>

# HITL
openfang hitl list
openfang hitl list --status pending

# Looper
openfang looper list

# Events
echo '{"event":"test","source":"cli"}' > /tmp/evt.json
openfang event dry-run /tmp/evt.json

# Artifacts & Docs
openfang artifact list
openfang doc list

# Packs
openfang pack list

# Runs
openfang run list

# Triggers v2
openfang trigger list
openfang trigger validate /tmp/trigger.json
```

## Development Sequencing

### Build Order

Implementation is ordered by dependency and user-facing priority:

| Phase | Commands | Rationale | Estimated Handlers |
|-------|----------|-----------|-------------------|
| **1** | `task`, `subtask` | Core domain — everything else references tasks | 14 handlers |
| **2** | `run`, `dispatch` | Workflow execution visibility | 16 handlers |
| **3** | `hitl` | Unblocks human interaction from CLI | 5 handlers |
| **4** | `looper` | Iterative execution control | 8 handlers |
| **5** | `event` | Trigger ingestion (lightweight) | 2 handlers |
| **6** | `artifact`, `doc` | Read-only browsing (lightweight) | 6 handlers |
| **7** | `pack` | Pack lifecycle management | 7 handlers |
| **8** | `trigger` upgrade | Adds v2 features to existing group | 9 handlers |

Total: **~67 new handler functions** across 10 command groups.

### Technical Dependencies

- Daemon must be running with Compozy features enabled
- No new crate dependencies required
- `reqwest::blocking` already supports SSE via `BufReader` line iteration
- All API types are `serde_json::Value` (no shared type imports needed)

## Monitoring & Observability

No new monitoring required. CLI commands are stateless HTTP calls. Errors are logged by the API server, not the CLI.

## Technical Considerations

### Key Decisions

1. **No new files** — All code goes in `main.rs` following the existing monolithic pattern. While large (~6500 lines), this is consistent and avoids refactoring risk.

2. **`serde_json::Value` over typed structs** — The CLI parses all responses as dynamic JSON. This avoids coupling CLI to server types and makes the CLI resilient to API additions.

3. **SSE via `BufReader`** — Uses the existing `reqwest::blocking` client with line-by-line reading for watch commands. No need for async or eventsource crates.

4. **JSON file input for create/update** — Matches the existing `workflow create <file>` pattern. Users author JSON files and pass them to the CLI, avoiding complex inline argument parsing.

5. **Separate `run` command** — Workflow runs get their own top-level command (`openfang run`) rather than nesting under `openfang workflow run` because runs have rich sub-resources (dispatches, HITL, signals, SSE) that would make the nesting too deep.

6. **Separate `subtask` command** — Subtasks can be queried across tasks (`/api/v1/subtasks`), so they need a top-level command in addition to `task subtasks <id>`.

### Known Risks

1. **`main.rs` size** — Adding ~67 handlers increases the file to ~8000 lines. Acceptable for now; future refactor can split into modules.

2. **SSE reliability** — `reqwest::blocking` SSE reading may miss reconnection semantics. Acceptable for CLI; the TUI handles robust streaming.

3. **API version drift** — If `/api/v1/` endpoints change schema, CLI may show `"?"` for missing fields. Mitigated by using defensive `unwrap_or("?")` pattern.

### Standards Compliance

- Follows existing CLI patterns (clap derive, `daemon_client()`, `daemon_json()`)
- Uses `ui::error()`, `ui::success()`, `ui::hint()` for consistent messaging
- No `unwrap()` in handlers (uses `unwrap_or()` or `unwrap_or_default()`)
- No new dependencies
- `make fmt && make lint && make test` must pass after each phase
