# Compozy Technical Specification Summary

**Baseline:** `tasks/prd-compozy/docs/`
**Date:** 2026-03-21
**Status:** Current architecture baseline

This document is a navigational summary of the Compozy architecture specification.
Every section links back to the authoritative source documents in `docs/`.

---

## 1. What Is Compozy

Compozy is an open-source, single-user, local-first Agent Operating System.
It continues as a **fork of OpenFang**, which remains the programmable platform core.
Compozy adds a product domain layer on top: durable workflows, tasks, subtasks,
domain primitives, and a unified public control plane.

There is no SaaS, billing, auth, or multi-tenancy. CLI and API are the primary
control plane; UI is a later client.

> Source: [DESIGN.md](docs/DESIGN.md) sections 1-2,
> [ADR-001](docs/adrs/001-openfang-as-programmable-platform-core.md),
> [ADR-002](docs/adrs/002-open-source-local-first-scope.md),
> [ADR-022](docs/adrs/022-single-public-compozy-namespace.md),
> [ADR-031](docs/adrs/031-cli-and-api-as-primary-control-plane.md)

---

## 2. Architecture Layers

```
Compozy Product Layer (domain, primitives, durable workflows)
  |
OpenFang Fork Core (agents, skills, triggers, schedulers, channels, runtime)
  |
Runtime Layer (providers, Arky crates, tool execution)
```

### Persistence Model (three layers)

| Layer                   | What it stores                                                                                                              | Location                     |
| ----------------------- | --------------------------------------------------------------------------------------------------------------------------- | ---------------------------- |
| File-backed definitions | agents, workflows, triggers, schedules, skills, packs, templates                                                            | `~/.compozy/`                |
| `runtime.db`            | Platform-core runtime state (agent runtime, sessions, messages, schedule runtime)                                           | `~/.compozy/data/runtime.db` |
| `compozy.db`            | Product domain + durable workflow execution (runs, checkpoints, dispatches, HITL, tasks, subtasks, looper, artifacts, docs) | `~/.compozy/data/compozy.db` |

Rules: no cross-database SQL joins; definitions never duplicated in databases;
mutations to config-first resources write files first, then reload.

> Source: [STORAGE-MODEL.md](docs/STORAGE-MODEL.md),
> [DATABASE-SCHEMA.md](docs/DATABASE-SCHEMA.md),
> [ADR-003](docs/adrs/003-separate-compozy-domain-database.md),
> [ADR-037](docs/adrs/037-file-backed-definitions-and-db-ownership.md)

---

## 3. Config-First Surface

Definitions are authored in **TOML** (source-friendly), transported as **JSON** (API),
and compiled to **internal IR** (execution). The pipeline is:

```
TOML/JSON -> schema validation -> reference validation -> semantic validation -> normalization -> compile -> IR
```

Validation is bounded to four layers (schema, reference, semantic, normalization)
and must NOT boot providers, call network, or execute templates.

> Source: [DESIGN.md](docs/DESIGN.md) sections 3, 6, 7,
> [ADR-004](docs/adrs/004-config-first-agents-and-workflows.md),
> [ADR-040](docs/adrs/040-toml-authoring-json-transport-ir-execution.md),
> [ADR-041](docs/adrs/041-bounded-layered-definition-validation.md)

---

## 4. Agent Model

### Public `agent_definition` schema

Top-level: `id`, `name`, `version`, `description`, `enabled`, `group`, `tags`

Main blocks:

- **provider** -- `driver`, `model`, `profile`, `defaults`, `config`, optional `request_extra`
- **prompt** -- `system`, `instructions`, `skills`
- **capabilities** -- `tools`, `primitives`, `delegation`, `workspace`, `network`
- **runtime** -- `autonomous`, `memory_policy`, `hitl`
- **input/output** -- shared lightweight definition contract schema

Compiles into three internal forms: `AgentManifest` + `ProviderBinding` + `AgentProductMetadata`.

### Provider layering (three tiers)

1. **Installation/workspace** -- credentials, binaries, environment, transport
2. **Profiles** -- reusable middle layer avoiding repetition
3. **Per-agent config** -- driver, model, profile, typed behavior, constrained `request_extra`

> Source: [DESIGN.md](docs/DESIGN.md) sections 4-5,
> [API-SPEC.md](docs/API-SPEC.md) Agent Resource section,
> [ADR-014](docs/adrs/014-agentmanifest-as-base-agent-schema.md),
> [ADR-027](docs/adrs/027-provider-specific-agent-configuration.md),
> [ADR-028](docs/adrs/028-agent-group-as-primary-organization.md),
> [ADR-029](docs/adrs/029-agent-definition-public-schema.md),
> [ADR-043](docs/adrs/043-provider-layering-and-constrained-request-extra.md)

---

## 5. Workflow v2

Evolves the OpenFang model rather than replacing it.

### Step kinds

`agent`, `primitive`, `workflow`, `wait_signal`, `start_looper`, `emit_event`, `collect`, `noop`

### Flow modes

`sequential`, `fan_out`, `conditional`, `loop`

### Durable execution objects

`workflow_run`, `workflow_checkpoint`, `workflow_signal`

Runs survive restart. Conservative recovery: `running` downgrades to `paused`;
`waiting_signal` persists; completed/failed/cancelled stay unchanged.

> Source: [DESIGN.md](docs/DESIGN.md) sections 9, 14,
> [API-SPEC.md](docs/API-SPEC.md) Workflow and Runs sections,
> [ADR-005](docs/adrs/005-durable-workflow-runtime.md),
> [ADR-017](docs/adrs/017-workflow-v2-as-minimal-evolution-of-openfang.md),
> [ADR-021](docs/adrs/021-runtime-first-workflow-hardening.md),
> [ADR-024](docs/adrs/024-workflow-v2-public-schema.md)

---

## 6. Trigger v2

Extends OpenFang trigger model with explicit targets beyond agent-wakeup.

- **Match:** `event`, `source`, `contains`, `filters`
- **Target kinds:** `agent_message`, `workflow_start`, `workflow_signal`

> Source: [DESIGN.md](docs/DESIGN.md) section 15,
> [API-SPEC.md](docs/API-SPEC.md) Trigger Resource section,
> [ADR-019](docs/adrs/019-trigger-v2-with-explicit-targets.md),
> [ADR-025](docs/adrs/025-trigger-v2-public-schema.md)

---

## 7. Delegation, HITL, and Autonomy

- **Autonomous by default** -- no interactive permission prompts
- **HITL** is the only intentional pause, appears only when explicitly modeled
- **Capabilities** describe availability (git, network, github), not approval gates
- **Agent delegation** (`call`, `send`, `spawn`) creates durable `agent_dispatch` records
- **HITL can occur inside active agent steps** (not just workflow-level pauses)
- **`hitl_request`** links to run, step, and optional dispatch

> Source: [DESIGN.md](docs/DESIGN.md) section 11,
> [ADR-007](docs/adrs/007-autonomous-by-default-explicit-hitl.md),
> [ADR-008](docs/adrs/008-open-tool-access-capabilities-as-availability.md),
> [ADR-009](docs/adrs/009-persisted-agent-delegation-and-lineage.md),
> [ADR-018](docs/adrs/018-hitl-can-occur-inside-active-agent-steps.md)

---

## 8. Domain Primitives

Reusable primitives shared by all agents and workflows (not hidden in a closed SDLC pipeline).

| Namespace      | Purpose                    |
| -------------- | -------------------------- |
| `artifact.*`   | Versioned output artifacts |
| `doc.*`        | Versioned documents        |
| `hitl.*`       | Human interaction requests |
| `task.*`       | Root durable work objects  |
| `subtask.*`    | Executable child units     |
| `agent.*`      | Agent delegation           |
| `capability.*` | Environment availability   |

**Task** is the main durable work object (replaces old "issue").
**Subtask** is the executable child unit within a task.
SDLC is one first-party workflow package built on these same primitives.

> Source: [DESIGN.md](docs/DESIGN.md) section 10,
> [ADR-006](docs/adrs/006-reusable-compozy-domain-primitives.md),
> [ADR-010](docs/adrs/010-sdlc-as-first-party-workflow-package.md),
> [ADR-045](docs/adrs/045-task-subtask-domain-model.md),
> [ADR-047](docs/adrs/047-keep-task-and-subtask-as-public-domain-names.md)

---

## 9. Looper

A specialized executor for iterative subtask work (not a separate workflow engine).
Runs on the same durable workflow foundation.

- **Execution policy:** `mode` (sequential/parallel), `max_parallelism`, `selection`
- Reads from canonical subtask table
- Subtasks narrow via `depends_on` and `parallelizable`

> Source: [DESIGN.md](docs/DESIGN.md) section 13,
> [API-SPEC.md](docs/API-SPEC.md) Looper Runs section,
> [ADR-011](docs/adrs/011-looper-as-specialized-executor.md),
> [ADR-046](docs/adrs/046-explicit-looper-execution-policy.md)

---

## 10. Packs and Safe Forking

- Packs are versioned distribution units
- Managed pack content is read-only; customization via explicit **fork** (shadow)
- Fork creates user-owned override in top-level dirs, records provenance
- Upgrades don't overwrite forks; built-ins treated as bundled first-party packs

> Source: [DESIGN.md](docs/DESIGN.md) section 16,
> [API-SPEC.md](docs/API-SPEC.md) Packs Resource section,
> [ADR-044](docs/adrs/044-versioned-packs-explicit-upgrades-and-safe-forks.md)

---

## 11. Public API Surface

Single namespace: **`/api/v1`** (HTTP) and **`compozy`** (CLI).

### Resources and key surfaces

| Resource      | Definition CRUD | Validate/Compile |                Operational                |       Watch (SSE)       |
| ------------- | :-------------: | :--------------: | :---------------------------------------: | :---------------------: |
| Agents        |        Y        |        Y         |        runtime, sessions, messages        |     messages/stream     |
| Workflows     |        Y        |        Y         |               runtime, runs               |    runs/{id}/events     |
| Triggers      |        Y        |        Y         |       runtime, enable/disable, test       |           --            |
| Schedules     |        Y        |        Y         |     runtime, enable/disable, run-now      |           --            |
| Runs          |       --        |        --        | list, detail, pause/resume/cancel, signal |    runs/{id}/events     |
| Dispatches    |       --        |        --        |   list, detail, retry, cancel, children   | dispatches/{id}/events  |
| HITL Requests |       --        |        --        |           list, detail, answer            |  hitl-requests/stream   |
| Tasks         |        Y        |        --        | subtasks, artifacts, docs, files, replan  |           --            |
| Subtasks      |        Y        |        --        |               list, detail                |           --            |
| Looper Runs   |       --        |        --        |    create, detail, pause/resume/cancel    | looper-runs/{id}/events |
| Packs         |       --        |        --        |  list, install, upgrade, uninstall, fork  |           --            |
| Events        |       --        |        --        |        POST ingress (with dry-run)        |           --            |

### Payload conventions

- Definition IDs: stable, user-controlled (`prd-writer`)
- Runtime IDs: opaque (`run_123`, `dispatch_456`)
- Lists: `{ items, next_cursor }`
- Validation: `{ valid, issues, normalized }`
- Compilation: `{ definition_id, normalized, compiled }`
- Dry-run: `{ would_execute, resolved, effects, explanation }`
- SSE: bounded replay with `Last-Event-ID`, reset+snapshot semantics

> Source: [API-SPEC.md](docs/API-SPEC.md) (complete contract),
> [ADR-023](docs/adrs/023-public-api-exposure-rules.md),
> [ADR-030](docs/adrs/030-agent-api-definition-and-operational-surfaces.md),
> [ADR-032](docs/adrs/032-workflow-api-definition-and-operational-surfaces.md),
> [ADR-033](docs/adrs/033-trigger-api-definition-and-operational-surfaces.md),
> [ADR-034](docs/adrs/034-canonical-control-plane-payload-conventions.md),
> [ADR-036](docs/adrs/036-explicit-watch-surfaces-for-live-operations.md),
> [ADR-038](docs/adrs/038-validate-compile-dry-run-and-explain-semantics.md),
> [ADR-039](docs/adrs/039-bounded-sse-replay-and-reset-semantics.md)

---

## 12. Database Schema

### `runtime.db` tables

| Table                | Purpose                                 |
| -------------------- | --------------------------------------- |
| `agent_runtime`      | Runtime projection for loaded agents    |
| `agent_session`      | Session metadata for direct agent use   |
| `agent_message`      | Message history                         |
| `schedule_runtime`   | Runtime state for file-backed schedules |
| `schedule_execution` | Recent schedule fire receipts           |
| `trigger_runtime`    | Runtime state for file-backed triggers  |

### `compozy.db` tables

| Table                           | Purpose                                        |
| ------------------------------- | ---------------------------------------------- |
| `workflow_run`                  | Durable workflow execution root                |
| `workflow_checkpoint`           | State transition and recovery trail            |
| `workflow_signal`               | Signals delivered to/consumed by runs          |
| `agent_dispatch`                | Delegated execution inside workflow runs       |
| `hitl_request`                  | Human interaction requests                     |
| `looper_run`                    | Looper execution root                          |
| `looper_subtask`                | Subtask-level execution for looper             |
| `task`                          | Product-domain root work object                |
| `subtask`                       | Executable child work items                    |
| `artifact` / `artifact_version` | Stable artifact identity + immutable revisions |
| `doc` / `doc_version`           | Stable document identity + immutable revisions |

> Source: [DATABASE-SCHEMA.md](docs/DATABASE-SCHEMA.md),
> [STORAGE-MODEL.md](docs/STORAGE-MODEL.md),
> [ADR-050](docs/adrs/050-initial-dual-database-migration-slice.md)

---

## 13. Implementation Phases

| Phase | Goal                                      | Key Tables/Systems                                       | Source Tasks            |
| ----- | ----------------------------------------- | -------------------------------------------------------- | ----------------------- |
| **0** | Dual-database bootstrap + migration infra | `schema_migrations` (both DBs), definitions coherence    | Tasks 1-9               |
| **1** | Durable workflow core + providers + agents | `workflow_run`, `workflow_checkpoint`, `workflow_signal`  | Tasks 10-22             |
| **2** | Delegation, HITL, domain, control-plane   | `agent_dispatch`, `hitl_request`, `task`, `subtask`      | Tasks 23-33             |
| **3** | Looper + triggers                         | `looper_run`, `looper_subtask`, trigger v2               | Tasks 34-36             |
| **4** | Artifacts, looper API, packs              | `artifact`, `doc`, versioning, pack CRUD                 | Tasks 37-40             |
| **5** | Final hardening + E2E                     | Pack system, retention, SSE, E2E test                    | Tasks 41-43             |

Delivery principle: **runtime first** (durable state before authoring UX),
**domain before convenience** (task/subtask as real records, not queue simulation).

> Source: [IMPLEMENTATION-PLAN.md](docs/IMPLEMENTATION-PLAN.md),
> [INITIAL-RUNTIME-MIGRATIONS.md](docs/INITIAL-RUNTIME-MIGRATIONS.md),
> [ADR-049](docs/adrs/049-phased-durable-runtime-implementation-order.md)

---

## 14. Task Breakdown (43 tasks)

> Renumbered per [PRD Decisions Design](../docs/plans/2026-03-23-prd-decisions-design.md).
> Original 32 tasks were split and augmented to 43 after readiness analysis.

| #   | Title                                                        | Domain              | Phase | Deps            |
| --- | ------------------------------------------------------------ | ------------------- | ----- | --------------- |
| 1   | Split Persistence Config For Dual Databases                  | infra/persistence   | 0     | --              |
| 2   | Dual-Database Bootstrap In Kernel Startup                    | infra/persistence   | 0     | 1               |
| 3   | Reusable Migration Runner For Both Databases                 | infra/persistence   | 0     | 1,2             |
| 4   | Copy Arky Crates Into OpenFang Workspace                     | providers/arky      | 0     | --              |
| 5   | Shared Definition Contract Types                             | types/schema        | 0     | --              |
| 6   | Initial runtime.db Schema And Stores                         | infra/persistence   | 0     | 2,3             |
| 7   | Workflow Definition Source-Of-Truth Consistency               | engine/workflow     | 0     | --              |
| 8   | Workflow Bootstrap And Readiness Semantics                    | engine/workflow     | 0     | 2,7             |
| 9   | Initial compozy.db Workflow Core Schema                      | infra/persistence   | 0     | 2,3             |
| 10  | Provider Layering For Workspace, Profiles, Agent Config      | providers/arky      | 1     | 4               |
| 11  | ProviderBinding Compile Layer For Compozy Agents             | providers/compile   | 1     | 10              |
| 12  | Typed Provider Integration For Codex And Claude Code         | providers/arky      | 1     | 11              |
| 13  | Workflow v2 Definition Types                                 | engine/workflow     | 1     | 5,8             |
| 14  | Workflow v2 Compile Pipeline                                 | engine/workflow     | 1     | 13              |
| 15  | Workflow v2 API Endpoints                                    | engine/workflow     | 1     | 14              |
| 16  | Durable Workflow Run Repository And Transition Writer        | engine/workflow     | 1     | 6,8,9           |
| 17  | Workflow Signal Persistence And Waiting-State Integration    | engine/workflow     | 1     | 9,16            |
| 18  | Agent Definition Validation And Compile Pipeline             | agents/compile      | 1     | 5,10,11,12      |
| 19  | Restart Recovery And Durable Run Control Surfaces            | engine/workflow     | 1     | 6,16,17         |
| 20  | Agent Definition CRUD And Compile Routes                     | api/agents          | 1     | 18              |
| 21  | Agent Runtime Operational Sub-Resources                      | api/agents          | 1     | 20              |
| 22  | Agent Sessions Messages And SSE Streaming                    | api/agents          | 1     | 21              |
| 23  | agent_dispatch Schema And Persistence Layer                  | engine/dispatch     | 2     | 9,19            |
| 24  | hitl_request Schema And Persistence Layer                    | engine/hitl         | 2     | 9,19            |
| 25  | Workflow Definition CRUD Control-Plane Surfaces              | api/workflows       | 2     | 14,19           |
| 26  | Schedule Control-Plane Surfaces                              | api/schedules       | 2     | 6,19            |
| 27  | Skills Listing Endpoint                                      | api/skills          | 2     | 26              |
| 28  | Task And Subtask Domain Schema And Repositories              | domain/tasks        | 2     | 19              |
| 29  | Dispatch Runtime Integration With Provider-Native Sessions   | engine/dispatch     | 2     | 12,20,23        |
| 30  | HITL Single-Turn Live Pause And Resume                       | engine/hitl         | 2     | 24,29           |
| 31  | HITL Post-Restart Reconstruction                             | engine/hitl         | 2     | 30              |
| 32  | Task And Subtask Control-Plane Plus Replanning               | api/tasks           | 2     | 28              |
| 33  | Dispatch And HITL Control-Plane Surfaces                     | api/dispatch        | 2     | 29,30           |
| 34  | Looper Durable Schema And Runtime                            | engine/looper       | 3     | 28              |
| 35  | Trigger v2 Types And Definition CRUD                         | engine/triggers     | 3     | 13              |
| 36  | Event Ingress Pipeline And Match Engine                      | engine/triggers     | 3     | 35,19           |
| 37  | Artifact And Doc Versioning                                  | domain/artifacts    | 4     | 32,34           |
| 38  | Artifact And Doc Standalone Read Endpoints                   | api/artifacts       | 4     | 37              |
| 39  | Looper Control-Plane And SSE Surfaces                        | api/looper          | 4     | 33,32,34        |
| 40  | Pack List Detail And CRUD Endpoints                          | api/packs           | 4     | 39              |
| 41  | Pack System Install Upgrade And Bootstrap                    | engine/packs        | 5     | 40              |
| 42  | Retention Policies And Remaining SSE Endpoints               | engine/retention    | 5     | 39,33           |
| 43  | E2E Integration Test And Restart Recovery Regression         | integration         | 5     | 41,42           |

---

## 15. ADR Index

All 50 ADRs are in [`docs/adrs/`](docs/adrs/).

### Platform Foundation (001-010)

| #   | Decision                                                                                                         |
| --- | ---------------------------------------------------------------------------------------------------------------- |
| 001 | [OpenFang as programmable platform core](docs/adrs/001-openfang-as-programmable-platform-core.md)                |
| 002 | [Open source, local-first scope](docs/adrs/002-open-source-local-first-scope.md)                                 |
| 003 | [Separate Compozy domain database](docs/adrs/003-separate-compozy-domain-database.md)                            |
| 004 | [Config-first agents and workflows](docs/adrs/004-config-first-agents-and-workflows.md)                          |
| 005 | [Durable workflow runtime](docs/adrs/005-durable-workflow-runtime.md)                                            |
| 006 | [Reusable domain primitives](docs/adrs/006-reusable-compozy-domain-primitives.md)                                |
| 007 | [Autonomous by default, explicit HITL](docs/adrs/007-autonomous-by-default-explicit-hitl.md)                     |
| 008 | [Open tool access, capabilities as availability](docs/adrs/008-open-tool-access-capabilities-as-availability.md) |
| 009 | [Persisted agent delegation and lineage](docs/adrs/009-persisted-agent-delegation-and-lineage.md)                |
| 010 | [SDLC as first-party workflow package](docs/adrs/010-sdlc-as-first-party-workflow-package.md)                    |

### Specialization and Extension (011-020)

| #   | Decision                                                                                                                        |
| --- | ------------------------------------------------------------------------------------------------------------------------------- |
| 011 | [Looper as specialized executor](docs/adrs/011-looper-as-specialized-executor.md)                                               |
| 012 | [Arky provider depth for Claude Code and Codex](docs/adrs/012-arky-provider-depth-for-claude-code-and-codex.md)                 |
| 013 | [Optional delegated repo/workspace automation](docs/adrs/013-optional-delegated-repo-and-workspace-automation.md)               |
| 014 | [AgentManifest as base agent schema](docs/adrs/014-agentmanifest-as-base-agent-schema.md)                                       |
| 015 | [OpenFang skills and schedules remain canonical](docs/adrs/015-openfang-skills-and-schedules-remain-canonical.md)               |
| 016 | [Extend triggers/workflows, don't redefine everything](docs/adrs/016-extend-triggers-and-workflows-dont-redefine-everything.md) |
| 017 | [Workflow v2 as minimal evolution](docs/adrs/017-workflow-v2-as-minimal-evolution-of-openfang.md)                               |
| 018 | [HITL inside active agent steps](docs/adrs/018-hitl-can-occur-inside-active-agent-steps.md)                                     |
| 019 | [Trigger v2 with explicit targets](docs/adrs/019-trigger-v2-with-explicit-targets.md)                                           |
| 020 | [Compozy-owned workflow/trigger surfaces](docs/adrs/020-compozy-owned-workflow-and-trigger-surfaces.md)                         |

### Public Surfaces and Schemas (021-035)

| #   | Decision                                                                                                              |
| --- | --------------------------------------------------------------------------------------------------------------------- |
| 021 | [Runtime-first workflow hardening](docs/adrs/021-runtime-first-workflow-hardening.md)                                 |
| 022 | [Single public Compozy namespace](docs/adrs/022-single-public-compozy-namespace.md)                                   |
| 023 | [Public API exposure rules](docs/adrs/023-public-api-exposure-rules.md)                                               |
| 024 | [Workflow v2 public schema](docs/adrs/024-workflow-v2-public-schema.md)                                               |
| 025 | [Trigger v2 public schema](docs/adrs/025-trigger-v2-public-schema.md)                                                 |
| 026 | [Runtime execution resource surfaces](docs/adrs/026-runtime-execution-resource-surfaces.md)                           |
| 027 | [Provider-specific agent configuration](docs/adrs/027-provider-specific-agent-configuration.md)                       |
| 028 | [Agent group as primary organization](docs/adrs/028-agent-group-as-primary-organization.md)                           |
| 029 | [Agent definition public schema](docs/adrs/029-agent-definition-public-schema.md)                                     |
| 030 | [Agent API definition and operational surfaces](docs/adrs/030-agent-api-definition-and-operational-surfaces.md)       |
| 031 | [CLI and API as primary control plane](docs/adrs/031-cli-and-api-as-primary-control-plane.md)                         |
| 032 | [Workflow API definition and operational surfaces](docs/adrs/032-workflow-api-definition-and-operational-surfaces.md) |
| 033 | [Trigger API definition and operational surfaces](docs/adrs/033-trigger-api-definition-and-operational-surfaces.md)   |
| 034 | [Canonical control-plane payload conventions](docs/adrs/034-canonical-control-plane-payload-conventions.md)           |
| 035 | [Schedule API surface on typed cron model](docs/adrs/035-schedule-api-surface-on-typed-cron-model.md)                 |

### Infrastructure and Contracts (036-050)

| #   | Decision                                                                                                            |
| --- | ------------------------------------------------------------------------------------------------------------------- |
| 036 | [Explicit watch surfaces for live operations](docs/adrs/036-explicit-watch-surfaces-for-live-operations.md)         |
| 037 | [File-backed definitions and DB ownership](docs/adrs/037-file-backed-definitions-and-db-ownership.md)               |
| 038 | [Validate/compile/dry-run/explain semantics](docs/adrs/038-validate-compile-dry-run-and-explain-semantics.md)       |
| 039 | [Bounded SSE replay and reset semantics](docs/adrs/039-bounded-sse-replay-and-reset-semantics.md)                   |
| 040 | [TOML authoring, JSON transport, IR execution](docs/adrs/040-toml-authoring-json-transport-ir-execution.md)         |
| 041 | [Bounded layered definition validation](docs/adrs/041-bounded-layered-definition-validation.md)                     |
| 042 | [Lightweight definition contract schema](docs/adrs/042-lightweight-definition-contract-schema.md)                   |
| 043 | [Provider layering and constrained request_extra](docs/adrs/043-provider-layering-and-constrained-request-extra.md) |
| 044 | [Versioned packs, explicit upgrades, safe forks](docs/adrs/044-versioned-packs-explicit-upgrades-and-safe-forks.md) |
| 045 | [Task and subtask domain model](docs/adrs/045-task-subtask-domain-model.md)                                         |
| 046 | [Explicit looper execution policy](docs/adrs/046-explicit-looper-execution-policy.md)                               |
| 047 | [Keep task/subtask as public domain names](docs/adrs/047-keep-task-and-subtask-as-public-domain-names.md)           |
| 048 | [Task/subtask control plane surfaces](docs/adrs/048-task-and-subtask-control-plane-surfaces.md)                     |
| 049 | [Phased durable runtime implementation order](docs/adrs/049-phased-durable-runtime-implementation-order.md)         |
| 050 | [Initial dual-database migration slice](docs/adrs/050-initial-dual-database-migration-slice.md)                     |

---

## 16. Open Questions

Areas that do not block the baseline but affect implementation shape:

1. **Workflow runtime** -- How much of existing WorkflowEngine refactored in place? Checkpoint event rows vs snapshots? When is `workflow_step_run` needed?
2. **Definitions and packaging** -- Versioning and pack compatibility details
3. **Tool runtime** -- Registry plug-in without destabilizing tool runner
4. **Domain model** -- Artifact/doc versioning rules
5. **UX and API** -- Which legacy OpenFang import paths worth keeping
6. **Persistence** -- Exact SQL types, indexes, retention policies
7. **Networking** -- OFP/A2A visibility in first Compozy UX
8. **Migration** -- Importer path for OpenFang definitions, upstream vs fork-maintained areas

> Source: [OPEN-QUESTIONS.md](docs/OPEN-QUESTIONS.md)

---

## 17. Explicitly Deferred

- `workflow_step_run` as first-class table (wait for Phase 2-3)
- Full event sourcing (event-like checkpoints initially)
- Fully normalized refs (bounded JSON initially)
- Artifact/doc versioning (Phase 4)
- Aggressive upstream sync
- OFP/A2A as primary product surface

---

## 18. Prior Implementation Reference (`compozy-code/`)

Compozy is a **rewrite** of an existing TypeScript + Rust product. The prior implementation
lives at `~/Dev/compozy/compozy-code/`. Implementers **should consult the old code** for domain context,
naming conventions, and behavioral expectations — especially for tasks that reimplement
existing features in Rust.

### Stack

- **TypeScript** monorepo (pnpm + Turborepo)
- **Backend**: Elysia + Effect-TS + Drizzle ORM + PostgreSQL
- **Desktop**: Tauri v2 (Rust backend + React frontend)
- **AI providers**: Claude Code, Codex, OpenCode (standalone provider packages)
- **Agent execution**: Actor-based "looper" engine in Node.js sidecar

### Key Source Directories

| Old Path                                                                       | Domain                                                                     | Maps To Tasks  |
| ------------------------------------------------------------------------------ | -------------------------------------------------------------------------- | -------------- |
| `~/Dev/compozy/compozy-code/packages/backend/src/db/schema/`                   | DB schemas (tasks, subtasks, artifacts, planning)                          | 28, 32, 37     |
| `~/Dev/compozy/compozy-code/packages/backend/src/modules/tasks/`               | Task CRUD, use cases, routes                                               | 28, 32         |
| `~/Dev/compozy/compozy-code/packages/backend/src/modules/artifacts/`           | Artifact model and repository                                              | 37             |
| `~/Dev/compozy/compozy-code/packages/backend/src/modules/subtasks/`            | Subtask model and repository                                               | 28, 32, 34     |
| `~/Dev/compozy/compozy-code/packages/tools/src/implementations/clarification/` | HITL / clarification tool (pause/resume)                                   | 24, 30         |
| `~/Dev/compozy/compozy-code/packages/tools/src/implementations/`               | Provider tool adapters (Codex, Claude Code)                                | 12, 29         |
| `~/Dev/compozy/compozy-code/packages/types/`                                   | Shared TypeScript types and contracts                                      | 5              |
| `~/Dev/compozy/compozy-code/packages/sdk/src/schemas/`                         | SDK schemas and contract definitions                                       | 5              |
| `~/Dev/compozy/compozy-code/packages/prompts/`                                 | Prompt builder system + built-in prompts                                   | 13, 18         |
| `~/Dev/compozy/compozy-code/packages/tauri/src-node/looper/`                   | Actor-based looper engine (job manager, task stream, execution control)    | 34, 39         |
| `~/Dev/compozy/compozy-code/packages/tauri/src/renderer/systems/`              | 34 domain systems (agent-runtime, hitl, tasks, artifacts, execution, etc.) | 30, 32, 37, 43 |
| `~/Dev/compozy/compozy-code/providers/core/src/`                               | Provider hooks, MCP bridges, tool bridges, token consumption               | 10, 12         |
| `~/Dev/compozy/compozy-code/providers/runtime/src/`                            | OpenResponses protocol, AI SDK bridge, session management                  | 10, 12, 29     |
| `~/Dev/compozy/compozy-code/packages/backend/src/modules/`                     | All backend modules (issues, prds, techspecs, repos, orgs, etc.)           | 32, 43         |
| `~/Dev/compozy/compozy-code/packages/tools/src/integration/`                   | Integration test patterns                                                  | 43             |

### Important Domain Mapping Notes

- **"Issue"** in the old codebase → **"Task"** in Compozy (renamed, see ADR-047)
- **"Clarification"** tool → **"HITL request"** in the new model (ADR-018)
- **Old subtask model** has 3-value status; new model is richer (`depends_on`, `parallelizable`, `assignee`)
- **Old looper** is Node.js actor-based; new looper is a durable Rust executor on the same workflow foundation
- **Old provider layer** uses Vercel AI SDK bridge; new one uses Arky crates with typed provider bindings

---

## Quick Reference: Source Documents

| Document                                                            | What it defines                                           |
| ------------------------------------------------------------------- | --------------------------------------------------------- |
| [DESIGN.md](docs/DESIGN.md)                                         | Architectural stance, product model, all major subsystems |
| [API-SPEC.md](docs/API-SPEC.md)                                     | Complete endpoint/payload contract for `/api/v1` and CLI  |
| [STORAGE-MODEL.md](docs/STORAGE-MODEL.md)                           | Three persistence layers and ownership boundaries         |
| [DATABASE-SCHEMA.md](docs/DATABASE-SCHEMA.md)                       | Table outlines for both databases                         |
| [IMPLEMENTATION-PLAN.md](docs/IMPLEMENTATION-PLAN.md)               | Six-phase delivery roadmap with dependency graph         |
| [INITIAL-RUNTIME-MIGRATIONS.md](docs/INITIAL-RUNTIME-MIGRATIONS.md) | Concrete Phase 0-1 migration specs                        |
| [OPEN-QUESTIONS.md](docs/OPEN-QUESTIONS.md)                         | Deferred decisions that don't block baseline              |
| [adrs/](docs/adrs/)                                                 | 50 Architecture Decision Records                          |
