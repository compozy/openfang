# CLI Features Analysis: UI/Dashboard Implications

## Purpose

This document maps every CLI command group added in the PRD-CLI to its potential
web UI counterpart. For each feature, it answers: what was added, does it belong
in the dashboard, and what kind of UI would serve it best.

The PRD-CLI exposed 10 new command groups (and upgraded 1 existing group) against
the `/api/v1/` endpoints introduced by PRD-Compozy (tasks 32–43). All endpoints
already exist on the backend. The CLI and any future UI are purely consumers.

---

## Task 1: CLI Task and Subtask Commands

**CLI functionality added:**

- `openfang task` — full CRUD (list, get, create, update, delete), plus `replan`
  (atomic bulk subtask replacement), and sub-resource access (`subtasks`,
  `artifacts`, `docs` nested under a task)
- `openfang subtask` — standalone CRUD across all subtasks, with filtering by
  `task_id` and `status`
- API surface: `/api/v1/tasks`, `/api/v1/subtasks`, `/api/v1/tasks/{id}/replan`

**Has UI implications: YES — high priority.**

Tasks and subtasks are the core domain objects of Compozy. Every other object
(runs, dispatches, looper runs, artifacts, docs) references a task. Without a
task management view in the dashboard, users must author and manage task JSON
files by hand and pass them to the CLI. This is acceptable for power users but
creates a significant barrier for everyone else.

**UI needed:**

- A "Tasks" tab in the dashboard with a paginated, filterable table (status,
  priority columns). Columns: ID, title, status, priority, owner, created.
- A task detail drawer or page showing: description, all subtasks in a nested
  list, linked artifacts, linked docs, and a replan action.
- Create/edit forms for tasks and subtasks (JSON editor or structured form).
- A "Replan" action on the task detail view that submits a new subtask set
  atomically.
- Subtask status badges (pending, in_progress, done, failed) with inline
  status-change affordance.

---

## Task 2: CLI Run and Dispatch Commands

**CLI functionality added:**

- `openfang run` — list/get workflow runs, inspect sub-resources (dispatches,
  HITL requests, signals, checkpoints), send signals, pause/resume/cancel, and
  watch live events via SSE
- `openfang dispatch` — list/get dispatch records, inspect child dispatches,
  retry or cancel failed dispatches, watch live events via SSE
- API surface: `/api/v1/runs`, `/api/v1/dispatches`, SSE endpoints
  `/api/v1/runs/{id}/events` and `/api/v1/dispatches/{id}/events`

**Has UI implications: YES — high priority.**

Workflow runs are the execution record for everything Compozy does. The
existing dashboard has no visibility into individual run state, the dispatch
graph, signals, or checkpoints. Without a run view, operators must use the CLI
to debug failed or stalled executions.

**UI needed:**

- A "Runs" tab with a live-updating table of workflow runs. Columns: ID,
  workflow name, status, step count, started, updated. Filter by status and
  workflow.
- A run detail page with tabs:
  - "Dispatches" sub-tab: table of dispatch records with retry/cancel actions.
  - "Signals" sub-tab: list of received signals plus a "Send Signal" form.
  - "Checkpoints" sub-tab: list of checkpoint records.
  - "HITL" sub-tab: any HITL requests attached to this run.
  - "Events" sub-tab: live SSE stream rendered as a scrollable event log.
- Pause/Resume/Cancel buttons on the run detail header.
- The dispatch detail view should show a parent/child tree (dispatch hierarchy).
- Real-time status updates via the SSE event stream — badge counts and status
  pills should update without a page reload.

---

## Task 3: CLI HITL Commands

**CLI functionality added:**

- `openfang hitl` — list/get HITL requests, answer or cancel a request,
  watch the global HITL SSE stream for incoming requests
- API surface: `/api/v1/hitl-requests`, `/api/v1/hitl-requests/stream` (SSE)

**Has UI implications: YES — critical priority.**

HITL requests are the primary mechanism for a workflow to pause and wait for
human input (approval, question answer, etc.). Without a UI, operators must
watch the CLI stream and answer via command line. This creates operational risk:
requests can go unnoticed, blocking entire workflow executions indefinitely.

**UI needed:**

- A "HITL" or "Approvals" tab in the dashboard with a real-time list of pending
  requests. This is the highest-urgency panel because pending items block
  execution.
- Each HITL request card should show: question text (full, not truncated), the
  run it belongs to, the kind (approval, question, file_upload, etc.), and how
  long it has been waiting.
- An inline answer form on each pending request card — a text input and a
  "Submit Answer" button that POSTs to `/api/v1/hitl-requests/{id}/answer`.
- A "Cancel" button per request.
- A badge or notification indicator on the dashboard header (or in the nav) that
  shows the count of pending HITL requests, updating via the global SSE stream.
- A filter for status (pending, answered, cancelled, expired).

---

## Task 4: CLI Looper Commands

**CLI functionality added:**

- `openfang looper` — list/get looper runs, create new runs (from JSON policy),
  inspect looper subtask execution state, pause/resume/cancel, watch live events
- API surface: `/api/v1/looper-runs`, SSE `/api/v1/looper-runs/{id}/events`
- Key display: a progress column showing `completed/total` subtasks

**Has UI implications: YES — medium priority.**

The looper is Compozy's iterative executor for running subtasks sequentially or
in parallel. Its runs have rich progress state that is well-suited to a visual
progress display. The CLI can show `3/12` in a column, but a UI can show a
progress bar, a per-subtask status grid, and real-time updates.

**UI needed:**

- A "Looper" tab (or a section within the run detail page, if looper runs are
  always associated with a task's run) showing looper runs in a table. Columns:
  ID, task, status, mode (sequential/parallel), progress (bar + N/total),
  updated.
- A looper run detail view with:
  - A subtask execution grid showing each subtask's status (pending, running,
    done, failed) with dispatch references.
  - A live progress bar driven by the SSE event stream.
  - Pause/Resume/Cancel controls.
- A "Create Looper Run" form that lets users select a task, choose subtasks, and
  configure the execution policy (mode, max_parallelism, selection strategy).

---

## Task 5: CLI Event Ingress Commands

**CLI functionality added:**

- `openfang event send <file>` — inject an event into the trigger match engine
- `openfang event dry-run <file>` — preview which triggers would fire without
  actually executing them
- API surface: `/api/v1/events`, `/api/v1/events/dry-run`

**Has UI implications: YES — medium priority.**

Event ingress is primarily a developer/operator tool for testing and manual
triggering. The dry-run mode is especially useful for verifying trigger
configurations before relying on them in production. A UI form would lower the
barrier significantly compared to authoring JSON files manually.

**UI needed:**

- An "Event Tester" panel, most naturally placed in a "Triggers" tab or as a
  sub-panel of trigger detail.
- A JSON editor (or structured form) for authoring an event payload: event name,
  source, payload object, idempotency key.
- A "Dry Run" button that submits to `/api/v1/events/dry-run` and displays which
  triggers matched, what effects would fire, and an explanation.
- A "Send Event" button that actually fires the event and shows the result:
  event ID, matched trigger count, effect count, any failures.
- The result panel should link to any workflow runs that were triggered.

---

## Task 6: CLI Artifact and Doc Commands

**CLI functionality added:**

- `openfang artifact` — list/get artifacts, browse version history
- `openfang doc` — list/get docs, browse version history
- Both are read-only; artifacts and docs are created by workflows and looper runs
- API surface: `/api/v1/artifacts`, `/api/v1/docs`
- Key display: version history with SHA-256 hash, created-by provenance, and
  timestamp

**Has UI implications: YES — medium priority.**

Artifacts and docs are the primary outputs of Compozy workflows. Without a UI
view, users cannot browse what was produced, inspect content, or trace which
agent or workflow created a version. The version history and provenance metadata
are especially useful for auditing.

**UI needed:**

- An "Artifacts" tab and a "Docs" tab (or a unified "Outputs" tab with type
  filter) showing a table per type. Columns: ID, type, title, current version,
  task link, created.
- A filter by type and by task.
- An artifact/doc detail panel showing:
  - Current version content (rendered if Markdown doc, raw if binary artifact).
  - Version history table: version number, SHA-256 hash, created-by (agent or
    workflow reference), created-at timestamp.
  - A "View Version" action to inspect a previous version's content.
- Links from task detail view to its associated artifacts and docs.

---

## Task 7: CLI Pack Commands

**CLI functionality added:**

- `openfang pack` — list/get installed packs, inspect managed objects, install
  from a source, upgrade with optional dry-run preview, uninstall, fork
- API surface: `/api/v1/packs`, install/upgrade/uninstall/fork action endpoints
- Key feature: `upgrade --dry-run` previews changes (added, changed, removed
  objects) before committing

**Has UI implications: YES — medium priority.**

Packs are Compozy's distribution format for bundles of workflows, triggers,
skills, and agent templates. A marketplace or library view in the dashboard
would make discovery and installation accessible without requiring CLI knowledge.
The dry-run preview is a strong candidate for a modal confirmation dialog.

**UI needed:**

- A "Packs" tab listing installed packs. Columns: name, version, source, object
  count, installed date.
- An "Install Pack" form with a source input (pack name, Git URL, local path).
- A pack detail page showing:
  - Metadata: name, version, source, description.
  - "Objects" sub-tab: table of managed objects (type, name, status).
  - "Upgrade" button that first calls the dry-run endpoint and shows a diff
    preview modal (added, changed, removed objects) before the user confirms.
  - "Uninstall" button with a confirmation dialog.
  - "Fork" button that creates a user-owned copy and navigates to the new pack.
- Status badges on each managed object (active, disabled, etc.).

---

## Task 8: CLI Trigger V2 Upgrade

**CLI functionality added (9 new subcommands on existing `trigger` group):**

- `get` — fetch trigger detail by ID
- `update` — update trigger definition from a JSON file
- `enable` / `disable` — toggle trigger active state
- `test <id> <event_json>` — dry-run a trigger against a synthetic event (shows
  whether it would match, what target would be resolved, whether dispatch would
  fire, and an explanation)
- `fork <id>` — create a user-owned copy of a trigger
- `validate <file>` — validate a trigger definition file (returns valid/invalid
  with structured issues list)
- `compile <file>` — compile a trigger definition and return the compiled payload
- `runtime <id>` — inspect a trigger's runtime status (last fired, match count,
  etc.)
- API surface: `/api/v1/triggers/{id}` (GET/PUT), enable/disable/test/fork/
  validate/compile/runtime endpoints

**Has UI implications: YES — high priority.**

The existing dashboard likely shows triggers in read-only list form. The v2
additions enable a full trigger authoring and testing workflow. The `test`
command in particular is a key developer tool: verifying that a trigger
definition correctly matches the events it should respond to, before deploying
it. The `enable`/`disable` toggle is the most obvious candidate for a UI button.

**UI needed:**

- Upgrade the existing "Triggers" tab to support:
  - An enable/disable toggle switch on each trigger row and on the detail view.
  - A "Get" detail panel showing trigger definition, runtime stats (last fired,
    match count from the `runtime` endpoint), and current status.
  - An "Edit" form (JSON editor or structured form) that calls the `update`
    endpoint.
  - A "Fork" action that duplicates the trigger definition as a new user-owned
    trigger.
- A trigger authoring sidebar or modal with:
  - A JSON editor for the trigger definition.
  - A "Validate" button that calls `/api/v1/triggers/validate` and shows
    inline issue annotations.
  - A "Compile" button that calls `/api/v1/triggers/compile` and shows the
    compiled result summary.
- A "Test Trigger" panel (either inline or as a modal):
  - A JSON editor for the synthetic event payload.
  - A "Run Test" button that calls `/api/v1/triggers/{id}/test`.
  - A result display: matched (yes/no), resolved target, would_dispatch (yes/no),
    and the explanation string.
- A "Runtime" panel on the trigger detail showing last-fire time, total match
  count, and any error history.

---

## Cross-Cutting UI Observations

### Real-Time Updates (SSE)

Four domains have SSE event streams: runs, dispatches, HITL requests, and looper
runs. The CLI uses `BufReader` line iteration for these. A web UI can use the
native `EventSource` browser API, which is simpler and more robust. All four
SSE-capable domains should drive live UI updates without polling:

| Stream | Endpoint | UI Usage |
|--------|----------|----------|
| Run events | `/api/v1/runs/{id}/events` | Live status pill, event log tab |
| Dispatch events | `/api/v1/dispatches/{id}/events` | Dispatch status updates |
| HITL global | `/api/v1/hitl-requests/stream` | Pending count badge, notification |
| Looper events | `/api/v1/looper-runs/{id}/events` | Progress bar, subtask grid |

### Prioritization for UI Build Order

| Priority | Domain | Rationale |
|----------|--------|-----------|
| 1 (critical) | HITL / Approvals | Blocks execution; operators need immediate visibility |
| 2 (high) | Tasks + Subtasks | Core domain; all other objects reference tasks |
| 3 (high) | Runs + Dispatches | Primary execution visibility and debugging surface |
| 4 (high) | Trigger v2 features | Enable/disable and test are immediately useful |
| 5 (medium) | Looper | Progress visualization is a strong UI improvement over CLI |
| 6 (medium) | Packs | Discovery and upgrade preview benefit from visual diff |
| 7 (medium) | Artifacts + Docs | Output browsing; version history benefits from UI |
| 8 (medium) | Event ingress | Developer/operator tool; dry-run result panel is useful |

### Data Model Notes (from `openfang-types` and `openfang-kernel`)

The kernel exposes a rich set of subsystems relevant to UI design:

- `WorkflowEngine` manages runs, signals, HITL, and checkpoints — these are the
  primary data sources for the Runs and HITL UI domains.
- `LooperRuntime` manages looper runs and their subtask execution — this drives
  the Looper UI domain.
- `TriggerV2Engine` manages trigger definitions and matching — this is the
  backend for the Trigger v2 UI features (test, validate, compile, runtime).
- `PackRegistry` manages installed packs — this is the backend for the Packs UI.
- `MeteringEngine` tracks cost; already partially exposed in the existing
  dashboard budget view.
- `AuditLog` (Merkle hash chain) is not currently exposed in any CLI or UI and
  may be a future UI feature (audit trail viewer).

The config model (`KernelConfig`, `ChannelOverrides`, `UserConfig`) drives the
existing Settings panel in the dashboard. No CLI PRD tasks touched config, so no
new UI fields are implied from this PRD.
