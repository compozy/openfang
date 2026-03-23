# Compozy Durable Runtime Implementation Plan

**Status:** Current implementation baseline (43-task breakdown)
**Date:** 2026-03-23
**Supersedes:** Original 32-task plan from 2026-03-21

> **Design decisions context:**
> All pre-implementation decisions (task splits, ownership boundaries, phantom
> dependency removals, HITL mechanism, crate placement, and phase assignments)
> are recorded in
> [`docs/plans/2026-03-23-prd-decisions-design.md`](../../docs/plans/2026-03-23-prd-decisions-design.md).
> This plan incorporates those decisions.

This document translates the reset architecture into an implementation order
across **6 phases and 43 tasks**.

It does not redefine the target product contract. It defines the lowest-risk
path to reach that contract.

## 1. Goals

The implementation plan should:

- preserve the accepted public model defined in the API specification
- follow the ownership split defined in the storage model
- realize the schema outline defined in the database schema documentation
- harden the current OpenFang fork without rewriting everything at once
- respect the task ownership boundaries established in the design decisions

## 2. Delivery Principles

### Runtime First

The first meaningful delivery is durable workflow state, not authoring sugar.

That means the earliest phases should prioritize:

- `workflow_run`
- `workflow_checkpoint`
- `workflow_signal`
- `agent_dispatch`
- `hitl_request`

before richer workflow UX is treated as done.

### Domain Before Convenience

`task`, `subtask`, and `looper_run` are product concepts and should land as
their own durable records instead of being simulated through the old OpenFang
task queue.

### File-Backed Definitions Stay Canonical

Definitions remain file-backed throughout delivery.

The new database tables are for:

- durable execution
- product domain state
- projections
- operational metadata

They are not a second source of truth for definitions.

### Avoid Early Table Explosion

The first cut should not introduce every possible helper table.

In particular, the initial runtime should avoid requiring:

- `workflow_step_run`
- general event sourcing infrastructure
- dedicated symbol tables
- excessive join tables for refs that can start as bounded JSON payloads

These can be added later if the simpler model proves insufficient.

## 3. Dependency Graph

### Table dependency order

The durable model has a natural dependency order:

1. `workflow_run`
2. `workflow_checkpoint`
3. `workflow_signal`
4. `agent_dispatch`
5. `hitl_request`
6. `task`
7. `subtask`
8. `looper_run`
9. `looper_subtask`

Interpretation:

- `workflow_run` is the root execution record
- checkpoints, signals, dispatches, and HITL depend on runs
- `task` can optionally point back to `workflow_run`
- `subtask` depends on `task`
- `looper_run` depends on `task` and may optionally point to `workflow_run`
- `looper_subtask` depends on both `looper_run` and `subtask`

### Task dependency chains (43-task breakdown)

The critical paths through the task graph:

```
Infrastructure path:
  1 (Config Split) -> 2 (Dual-DB Bootstrap) -> 3 (Migration Runner)
    -> 6 (runtime.db Schema) -> 16 (Durable Workflow Run Repo)
    -> 9 (compozy.db Workflow Core)

Provider path:
  4 (Arky Workspace) -> 10 (Provider Layering) -> 11 (ProviderBinding)
    -> 12 (Typed Providers) -> 18 (Agent Compile)

Workflow definition path:
  5 (Contract Types) + 8 (Workflow Bootstrap) -> 13 (Workflow v2 Types)
    -> 14 (Compile Pipeline) -> 15 (API Endpoints)

Durable runtime path:
  9 + 16 -> 17 (Signal Persistence) -> 19 (Restart Recovery)
    -> 23 (agent_dispatch) + 24 (hitl_request)

Agent control-plane path:
  18 -> 20 (Agent CRUD) -> 21 (Runtime Ops) -> 22 (Sessions/SSE)

Dispatch + HITL path:
  23 + 12 + 20 -> 29 (Dispatch Integration) -> 30 (HITL Live)
    -> 31 (HITL Post-Restart) + 33 (Dispatch/HITL Control-Plane)

Domain path:
  19 -> 28 (Task/Subtask Schema) -> 32 (Task Control-Plane) -> 34 (Looper)
    -> 37 (Artifact/Doc) -> 39 (Looper Control-Plane) -> 40 (Pack CRUD)
    -> 41 (Pack System) -> 43 (E2E)

Trigger path:
  13 -> 35 (Trigger v2 Types) -> 36 (Event Ingress)
```

Parallel entry points (no dependencies): Tasks 1, 4, 5, 7.

## 4. Phase Plan

### Phase 0: Dual-Database Bootstrap And Foundation (Tasks 1-9)

Objective:

- make `runtime.db` and `compozy.db` first-class migration targets
- establish workspace integration, shared types, and definition consistency
- lay the durable workflow schema foundation in both databases

Tasks:

| # | Title | Key Deliverables |
|---|-------|-----------------|
| 1 | Split Persistence Config For Dual Databases | `PersistenceConfig` with separate paths for `runtime.db` and `compozy.db` |
| 2 | Dual-Database Bootstrap In Kernel Startup | Startup path that opens both databases predictably |
| 3 | Reusable Migration Runner For Both Databases | Migration runner applying ordered migrations per database |
| 4 | Copy Arky Crates Into OpenFang Workspace | Arky provider crates available in the workspace |
| 5 | Shared Definition Contract Types | Lightweight `input`/`output` contract schema for agents and workflows |
| 6 | Initial runtime.db Schema And Stores | `agent_runtime`, `agent_session`, `agent_message`, `schedule_runtime`, `schedule_execution`, `trigger_runtime` tables |
| 7 | Workflow Definition Source-Of-Truth Consistency | File-backed workflow definitions as single source of truth |
| 8 | Workflow Bootstrap And Readiness Semantics | Workflow loading, readiness checks, startup coherence |
| 9 | Initial compozy.db Workflow Core Schema | `workflow_run`, `workflow_checkpoint`, `workflow_signal` tables with `*Store` adapters |

Key tables/systems:

- `schema_migrations` (both DBs)
- `runtime.db`: `agent_runtime`, `agent_session`, `agent_message`, `schedule_runtime`, `schedule_execution`, `trigger_runtime`
- `compozy.db`: `workflow_run`, `workflow_checkpoint`, `workflow_signal`
- Definition file coherence and bootstrap semantics

Exit criteria:

- fork boots with both databases present
- migrations are deterministic and idempotent
- workflow definitions load from files and are coherent at startup
- Arky crates and shared contract types available for downstream tasks

### Phase 1: Providers, Workflow Compile, Durable Runs, Agent Control-Plane (Tasks 10-22)

Objective:

- establish the provider layering stack (workspace, profiles, per-agent config)
- build the workflow v2 definition-to-IR compile pipeline with API endpoints
- make workflow runs durable with transition writing and signal persistence
- deliver agent definition compile and full agent control-plane surfaces

Tasks:

| # | Title | Key Deliverables |
|---|-------|-----------------|
| 10 | Provider Layering For Workspace, Profiles, Agent Config | Three-tier provider layering |
| 11 | ProviderBinding Compile Layer For Compozy Agents | `ProviderBinding` type and compile logic |
| 12 | Typed Provider Integration For Codex And Claude Code | Concrete typed providers for Codex and Claude Code |
| 13 | Workflow v2 Definition Types | Step kinds, flow modes, definition fields |
| 14 | Workflow v2 Compile Pipeline | Validate, normalize, compile to `WorkflowIr` |
| 15 | Workflow v2 API Endpoints | `/validate`, `/compile`, `/compiled` endpoints |
| 16 | Durable Workflow Run Repository And Transition Writer | `TransitionWriter` orchestrating checkpoint + run update atomically |
| 17 | Workflow Signal Persistence And Waiting-State Integration | Signal idempotency, eager-consume path, consume logic |
| 18 | Agent Definition Validation And Compile Pipeline | Agent definition validation and compile to internal forms |
| 19 | Restart Recovery And Durable Run Control Surfaces | Recovery scan, pause/resume/cancel, `running` -> `paused` on crash |
| 20 | Agent Definition CRUD And Compile Routes | Agent definition management API |
| 21 | Agent Runtime Operational Sub-Resources | Runtime state queries for agents |
| 22 | Agent Sessions Messages And SSE Streaming | Session management and streaming endpoints |

Key tables/systems:

- `workflow_run`, `workflow_checkpoint`, `workflow_signal` (runtime use via stores from Task 9)
- `TransitionWriter` (Task 16, business logic over Task 9 stores)
- Workflow v2 compile pipeline (TOML -> JSON -> validate -> normalize -> IR)
- Provider layering and `ProviderBinding`
- Agent compile pipeline (definition -> `AgentManifest` + `ProviderBinding` + metadata)
- Agent CRUD, runtime ops, sessions, messages, SSE

Exit criteria:

- workflow runs are durable and survive restart
- waiting runs resume after restart
- workflow definitions can be validated and compiled via API
- provider layering resolves workspace + profile + per-agent config
- agent definitions compile and are manageable through CRUD endpoints
- agent runtime state, sessions, messages, and SSE streaming are operational

### Phase 2: Dispatch, HITL, Domain, Control-Plane Surfaces (Tasks 23-33)

Objective:

- make agent execution and in-step human pauses recoverable
- land workflow definition CRUD and schedule control-plane
- establish the task/subtask domain model
- integrate dispatch with provider-native sessions
- deliver HITL live pause/resume and post-restart reconstruction

Tasks:

| # | Title | Key Deliverables |
|---|-------|-----------------|
| 23 | agent_dispatch Schema And Persistence Layer | `agent_dispatch` table and store |
| 24 | hitl_request Schema And Persistence Layer | `hitl_request` table and store |
| 25 | Workflow Definition CRUD Control-Plane Surfaces | Create, update, delete, list workflow definitions |
| 26 | Schedule Control-Plane Surfaces | Schedule CRUD, enable/disable, run-now |
| 27 | Skills Listing Endpoint | `GET /api/v1/skills` for API-visible skills listing |
| 28 | Task And Subtask Domain Schema And Repositories | `task` and `subtask` tables with repositories |
| 29 | Dispatch Runtime Integration With Provider-Native Sessions | Dispatch lifecycle with real provider sessions |
| 30 | HITL Single-Turn Live Pause And Resume | `tokio::oneshot` channel mechanism for in-flight HITL |
| 31 | HITL Post-Restart Reconstruction | Recovery of pending `hitl_request` rows after restart |
| 32 | Task And Subtask Control-Plane Plus Replanning | Task/subtask CRUD, replan endpoint |
| 33 | Dispatch And HITL Control-Plane Surfaces | Dispatch and HITL listing, detail, retry, answer endpoints |

Key tables/systems:

- `compozy.db`: `agent_dispatch`, `hitl_request`, `task`, `subtask`
- Workflow definition CRUD (file-backed via `definition_store.rs`)
- Schedule control-plane
- Skills listing
- Dispatch integration with provider sessions
- HITL mechanism: `tokio::oneshot` channel + HashMap registry for live; checkpoint-based reconstruction for restart

Exit criteria:

- dispatches are durable and recoverable after restart
- HITL mid-step pauses and resumes without losing the run
- HITL pending requests survive restart and can be answered post-recovery
- tasks and subtasks are queryable and mutable independently of workflows
- replanning changes subtask structure without replacing task identity
- workflow definitions are manageable via CRUD
- schedules are controllable through the control-plane

### Phase 3: Looper Runtime, Triggers, Event Ingress (Tasks 34-36)

Objective:

- make the looper a durable executor over subtasks
- establish trigger v2 definition types and CRUD
- build the event ingress pipeline and match engine

Tasks:

| # | Title | Key Deliverables |
|---|-------|-----------------|
| 34 | Looper Durable Schema And Runtime | `looper_run`, `looper_subtask` tables; durable looper execution |
| 35 | Trigger v2 Types And Definition CRUD | Trigger v2 definition types and file-backed CRUD |
| 36 | Event Ingress Pipeline And Match Engine | Event intake, filter matching, target dispatch |

Key tables/systems:

- `compozy.db`: `looper_run`, `looper_subtask`
- Trigger v2 definition types (match, target kinds: `agent_message`, `workflow_start`, `workflow_signal`)
- Event ingress pipeline with filter matching

Execution rules for the looper first cut:

- looper reads subtasks from the canonical `subtask` table
- looper policy defines the concurrency envelope
- subtasks may narrow execution through `depends_on` and `parallelizable`

Exit criteria:

- sequential and bounded-parallel looper runs are durable
- looper state and subtask progress survive restart
- trigger v2 definitions are manageable via CRUD
- events can be ingested and matched to targets

### Phase 4: Artifacts, Looper API, Pack CRUD (Tasks 37-40)

Objective:

- add artifact and document versioning and direct read access
- deliver looper control-plane with SSE surfaces
- establish pack list/detail/CRUD endpoints

Tasks:

| # | Title | Key Deliverables |
|---|-------|-----------------|
| 37 | Artifact And Doc Versioning | `artifact`, `artifact_version`, `doc`, `doc_version` tables |
| 38 | Artifact And Doc Standalone Read Endpoints | Direct API access for artifacts and docs without going through tasks |
| 39 | Looper Control-Plane And SSE Surfaces | Looper listing, detail, pause/resume/cancel, SSE events |
| 40 | Pack List Detail And CRUD Endpoints | Pack listing, detail, CRUD API surfaces |

Key tables/systems:

- `compozy.db`: `artifact`, `artifact_version`, `doc`, `doc_version`
- Artifact/doc standalone read endpoints
- Looper control-plane + SSE
- Pack CRUD (prerequisite for pack system install/upgrade)

Exit criteria:

- artifact/doc identity and versioning are durable enough for first-party SDLC flows
- artifacts and docs are accessible via standalone endpoints
- looper runs are fully controllable through the API with live SSE events
- packs are listable, inspectable, and manageable via CRUD

### Phase 5: Pack System, Retention, E2E Hardening (Tasks 41-43)

Objective:

- deliver pack install/upgrade/bootstrap lifecycle
- add retention policies and remaining SSE endpoints
- validate the full system with end-to-end integration tests

Tasks:

| # | Title | Key Deliverables |
|---|-------|-----------------|
| 41 | Pack System Install Upgrade And Bootstrap | Pack install, upgrade, uninstall, fork, bootstrap |
| 42 | Retention Policies And Remaining SSE Endpoints | Configurable retention, remaining SSE surfaces |
| 43 | E2E Integration Test And Restart Recovery Regression | Full integration test suite, restart recovery regression |

Key tables/systems:

- Pack system lifecycle (install, upgrade, uninstall, fork, bootstrap)
- Retention policies for runtime and domain tables
- Remaining SSE endpoints (bounded replay, reset semantics)
- E2E integration test covering the full durable workflow lifecycle

Exit criteria:

- packs can be installed, upgraded, forked, and bootstrapped
- retention policies are configurable and applied
- all SSE endpoints deliver bounded replay with `Last-Event-ID` support
- E2E test exercises: workflow create -> run -> dispatch -> HITL -> signal -> restart -> recovery -> completion
- restart recovery regression passes for all durable state

## 5. Minimal First Shipping Slice

If the fork needs a sharply bounded first slice, the minimum useful set is
**Phases 0-2 (Tasks 1-33)**, which delivers:

Tables:

- `runtime.db`: `agent_runtime`, `agent_session`, `agent_message`, `schedule_runtime`, `schedule_execution`, `trigger_runtime`
- `compozy.db`: `workflow_run`, `workflow_checkpoint`, `workflow_signal`, `agent_dispatch`, `hitl_request`, `task`, `subtask`

Capabilities:

- durable workflow runs with restart recovery
- workflow v2 definition compile pipeline and API
- provider layering with typed Codex/Claude Code support
- agent definition compile and full control-plane (CRUD, runtime, sessions, SSE)
- durable delegation with provider-native sessions
- in-step HITL (live pause/resume and post-restart reconstruction)
- task/subtask domain surfaces with replanning
- workflow definition CRUD and schedule control-plane
- dispatch and HITL control-plane surfaces

Phases 3-5 (looper, triggers, artifacts, packs, retention, E2E) can trail
behind this slice if the initial flows do not require deep artifact history,
event-driven triggers, or looper execution yet.

## 6. Phase-To-Table Mapping

| Phase | Database | Tables Created |
|-------|----------|---------------|
| **0** | `runtime.db` | `agent_runtime`, `agent_session`, `agent_message`, `schedule_runtime`, `schedule_execution`, `trigger_runtime` |
| **0** | `compozy.db` | `workflow_run`, `workflow_checkpoint`, `workflow_signal` |
| **2** | `compozy.db` | `agent_dispatch`, `hitl_request`, `task`, `subtask` |
| **3** | `compozy.db` | `looper_run`, `looper_subtask` |
| **4** | `compozy.db` | `artifact`, `artifact_version`, `doc`, `doc_version` |

## 7. Initial Index Strategy

The exact SQL design remains open, but the first migration set should at least
plan for these lookup patterns:

### `workflow_run`

- by `workflow_id`
- by `status`
- by `updated_at`

### `workflow_checkpoint`

- by `run_id`, ordered by `created_at`

### `workflow_signal`

- by `run_id`
- by `run_id + consumed`
- by `run_id + name`

### `agent_dispatch`

- by `run_id`
- by `status`
- by `parent_dispatch_id`

### `hitl_request`

- by `run_id`
- by `dispatch_id`
- by `status`

### `task`

- by `status`
- by `priority`
- by `source_run_id`
- by `updated_at`

### `subtask`

- by `task_id + position`
- by `task_id + status`
- by `assignee_kind + assignee_ref`

### `looper_run`

- by `task_id`
- by `source_run_id`
- by `status`

### `looper_subtask`

- by `looper_run_id + status`
- by `subtask_id`

## 8. What Stays Deferred

The first durable implementation should explicitly defer:

- `workflow_step_run` as a mandatory first-class table
- full event sourcing
- deep replay of every intermediate symbol mutation
- fully normalized refs for every linked domain object
- aggressive upstream sync work

These may become necessary later, but they should not block the first durable
cut.

## 9. Relationship To Open Questions

This plan resolves the architectural open question about the lowest-risk
migration path from the current in-memory runtime to a durable one.

It does **not** yet resolve:

- exact SQL types
- exact index implementation
- retention policies
- artifact/doc versioning details
- upstreaming strategy

Those remain implementation and maintenance questions, not architecture
blockers.
