## status: completed

<task_context>
<domain>openfang-api/static/js/pages</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>critical</complexity>
<dependencies>task_1,task_2</dependencies>
</task_context>

# Task 7.0: Workflows v2 — Full Rebuild on v1 API

## Overview

Rebuild the Workflows page on the `/api/v1/workflows` API. This is the most complex single page: list with runtime status, full editor supporting all 8 step kinds, flow mode picker, input/output contract editor, validate/compile actions, fork, run trigger with dry-run preview, and Visual Builder updates for v2 step types.

<critical>
- **ALWAYS READ** @CLAUDE.md before start - **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-ui/techspec.md`, `tasks/prd-ui/analysis_prd_tasks_1_15.md` (tasks 13-15), and `tasks/prd-ui/analysis_prd_tasks_16_30.md` (task 25)
- **YOU CAN ONLY** finish when `make fmt && make lint && make test` pass AND manual browser verification confirms page works
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
- **DASHBOARD / ALPINE** — use `alpine-js` skill for static HTML, Alpine components, and JS under `static/`
</critical>

<requirements>
- Migrate from `/api/workflows` to `/api/v1/workflows`
- List: steps count, enabled toggle, runtime status (active_runs, last_run_at), origin badge
- Editor supports all 8 step kinds: Agent, Primitive, Workflow, WaitSignal, StartLooper, EmitEvent, Collect, Noop
- Flow mode picker: Sequential, FanOut, Conditional, Loop with mode-specific fields
- `save_as` symbol binding per step
- `input`/`output` contract editor
- Validate: `POST /api/v1/workflows/validate` with inline per-step issue display
- Compile: `POST /api/v1/workflows/compile` with IR viewer
- Fork: `POST /api/v1/workflows/{id}/fork`
- Run trigger form: `POST /api/v1/workflows/{id}/runs` with input fields
- Dry-run preview: `POST /api/v1/workflows/{id}/runs/dry-run`
- Visual Builder updated for v2 step types
</requirements>

## Subtasks

- [x] 7.1 Migrate workflow list to v1 API — `OpenFangAPI.v1.workflows.list()`, add enabled toggle, runtime status columns, origin badge
- [x] 7.2 Implement workflow CRUD — create, update, delete using v1 endpoints
- [x] 7.3 Build step editor — support all 8 step kinds with kind-specific form fields
- [x] 7.4 Implement flow mode picker — Sequential (default), FanOut, Conditional (`when` field), Loop (`until` + `max_iterations`)
- [x] 7.5 Implement `save_as` symbol binding per step
- [x] 7.6 Implement `input`/`output` contract editor using ContractKind vocabulary
- [x] 7.7 Implement validate action — call `POST /api/v1/workflows/validate`, display issues inline with severity + path highlighting
- [x] 7.8 Implement compile action — call `POST /api/v1/workflows/compile`, display compiled IR summary
- [x] 7.9 Implement fork action — call `POST /api/v1/workflows/{id}/fork`, navigate to new workflow
- [x] 7.10 Implement run trigger form — input fields based on workflow input contract, submit to `POST /api/v1/workflows/{id}/runs`
- [x] 7.11 Implement dry-run preview — `POST /api/v1/workflows/{id}/runs/dry-run`, show would_execute/effects
- [x] 7.12 Implement runtime status panel — active_runs count, last_run_at, healthy indicator from `GET /api/v1/workflows/{id}/runtime`
- [x] 7.13 Implement runs sub-list per workflow — from `GET /api/v1/workflows/{id}/runs`
- [x] 7.14 Update Visual Builder (`workflow-builder.js`) node palette for v2 step types
- [x] 7.15 Remove or deprecate old `scheduler.js` workflows tab references
- [x] 7.16 Update `index_body.html` workflows template section

## Implementation Details

### 8 Step Kinds

| Kind | Fields |
|------|--------|
| Agent | agent_id, instructions (optional), provider_override |
| Primitive | action, params |
| Workflow | workflow_id |
| WaitSignal | signal_name, timeout_secs |
| StartLooper | task_id, execution_policy |
| EmitEvent | event, source, payload |
| Collect | from_step, aggregation |
| Noop | (none) |

### Flow Modes

| Mode | Required Fields |
|------|----------------|
| Sequential | (none — default) |
| FanOut | items (expression) |
| Conditional | when (expression) |
| Loop | until (expression), max_iterations |

### API Endpoints Used

All 13 endpoints under `OpenFangAPI.v1.workflows.*` from the techspec.

### Relevant Files

- `crates/openfang-api/static/js/pages/workflows.js` (REBUILD)
- `crates/openfang-api/static/js/pages/workflow-builder.js` (MODIFY)
- `crates/openfang-api/static/index_body.html` (MODIFY)
- `crates/openfang-api/static/css/components.css` (MODIFY)

## Deliverables

- Rebuilt `workflows.js` on v1 API with full editor, validate/compile, fork, run trigger
- Updated `workflow-builder.js` with v2 step types
- Complete migration from legacy `/api/workflows` to `/api/v1/workflows`

## Tests

### Manual Browser Tests (Required)

- [ ] Verify workflow list loads with new columns (enabled, runtime status, origin)
- [ ] Create workflow with each step kind — verify all 8 kinds have correct fields
- [ ] Set flow mode — verify mode-specific fields appear
- [ ] Validate workflow — verify inline issue display with severity colors
- [ ] Compile workflow — verify IR viewer shows compiled output
- [ ] Fork workflow — verify new copy created
- [ ] Trigger run — verify run starts (check Runs page)
- [ ] Dry-run — verify preview without actual execution
- [ ] Visual Builder — verify v2 node types in palette

### Verification Commands

- [ ] `make fmt`
- [ ] `make lint`
- [ ] `make test`

## Success Criteria

- All workflow operations use v1 API exclusively
- Editor supports all 8 step kinds and 4 flow modes
- Validate/compile/fork/run/dry-run all functional
- Visual Builder updated for v2 types
