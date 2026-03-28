## status: pending

<task_context>
<domain>openfang-api/static/js/pages</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>critical</complexity>
<dependencies>task_1,task_2</dependencies>
</task_context>

# Task 10.0: Agents v1 Migration

## Overview

Rebuild the Agents page on the `/api/v1/agents` API. The largest existing page gets migrated to structured `AgentDefinition` forms, validate/compile actions, runtime panel with start/stop/restart, sessions management, skills/MCP assignment, and provider profiles integration.

<critical>
- **ALWAYS READ** @CLAUDE.md before start - **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-ui/techspec.md` and `tasks/prd-ui/analysis_prd_tasks_16_30.md` (tasks 20, 21)
- **YOU CAN ONLY** finish when `make fmt && make lint && make test` pass AND manual browser verification confirms page works
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Migrate from `/api/agents` to `/api/v1/agents`
- List: enabled, group, tags, origin.kind, runtime_status columns
- Agent create/edit: structured `AgentDefinition` form (not raw TOML textarea)
- Validate: `POST /api/v1/agents/validate` with per-field issue display
- Compile: `POST /api/v1/agents/compile` with 3-layer compiled output viewer
- Runtime panel: state, mode, health, active sessions/dispatches
- Runtime actions: start/stop/restart buttons, mode selector dropdown
- Sessions tab: list with activate/reset/compact per session
- Skills assignment: `GET/PUT /api/agents/{id}/skills`
- MCP servers assignment: `GET/PUT /api/agents/{id}/mcp_servers`
</requirements>

## Subtasks

- [ ] 10.1 Migrate agent list to v1 — `OpenFangAPI.v1.agents.list()`, add enabled/group/tags/origin/runtime columns
- [ ] 10.2 Rebuild spawn wizard to create agents via v1 — structured form instead of TOML
- [ ] 10.3 Implement agent edit form — structured `AgentDefinition` fields
- [ ] 10.4 Implement agent delete using v1 endpoint
- [ ] 10.5 Implement validate action with per-field issue display
- [ ] 10.6 Implement compile action with 3-layer output viewer (manifest, binding, metadata)
- [ ] 10.7 Implement runtime panel — state, mode, health from `GET /api/v1/agents/{id}/runtime`
- [ ] 10.8 Implement runtime actions — start/stop/restart with confirmation dialogs
- [ ] 10.9 Implement mode selector dropdown — `PUT /api/v1/agents/{id}/runtime/mode`
- [ ] 10.10 Implement sessions tab — list, create, activate, reset, compact
- [ ] 10.11 Implement skills assignment — list and update agent skills
- [ ] 10.12 Implement MCP servers assignment — list and update agent MCP servers
- [ ] 10.13 Update `index_body.html` agents template section

## Implementation Details

### API Endpoints Used

All endpoints under `OpenFangAPI.v1.agents.*` from the techspec (22 endpoints).

### Relevant Files

- `crates/openfang-api/static/js/pages/agents.js` (REBUILD)
- `crates/openfang-api/static/index_body.html` (MODIFY)
- `crates/openfang-api/static/css/components.css` (MODIFY)

## Deliverables

- Rebuilt `agents.js` on v1 API with structured forms, validate/compile, runtime, sessions
- Complete migration from legacy `/api/agents` to `/api/v1/agents`
- Skills and MCP server assignment UI

## Tests

### Manual Browser Tests (Required)

- [ ] Agent list loads with new columns from v1 API
- [ ] Create agent via structured form — verify agent appears
- [ ] Edit agent — verify changes persist via v1 API
- [ ] Validate agent — verify inline issue display
- [ ] Compile agent — verify 3-layer output viewer
- [ ] Runtime panel — verify state, mode, health display
- [ ] Start/stop/restart — verify actions work with confirmations
- [ ] Sessions tab — verify list, create, activate, reset, compact
- [ ] Inline chat still works after migration

### Verification Commands

- [ ] `make fmt`
- [ ] `make lint`
- [ ] `make test`

## Success Criteria

- All agent operations use v1 API exclusively
- Structured form replaces TOML textarea
- Runtime panel with full lifecycle controls
- Sessions management functional
- Inline chat unaffected by migration
