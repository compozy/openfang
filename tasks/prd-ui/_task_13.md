## status: completed

<task_context>
<domain>openfang-api/static/js/pages</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task_1,task_10,task_12</dependencies>
</task_context>

# Task 13.0: Arky Provider UI — Profiles, Driver Config, Spawn Wizard

## Overview

Build the Arky provider UI: provider profiles management page, upgrade the agent spawn wizard to use Arky drivers instead of legacy model-catalog providers, add driver-specific config editing, reasoning effort controls, and compiled binding inspector.

<critical>
- **ALWAYS READ** @CLAUDE.md before start - **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-ui/adr-009-arky-providers.md` and `tasks/prd-ui/analysis_arky_providers.md`
- **YOU CAN ONLY** finish when `make fmt && make lint && make test` pass AND manual browser verification confirms all features work
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
- **DASHBOARD / ALPINE** — use `alpine-js` skill for static HTML, Alpine components, and JS under `static/`
</critical>

<requirements>
- Provider Profiles page (in Settings or standalone) with CRUD from `/api/v1/provider-profiles`
- Spawn wizard Step 3 shows 10 Arky drivers instead of model-catalog providers
- Driver-specific config fields shown based on selected driver (codex, claude-code, claude-compatible)
- Profile picker dropdown in spawn wizard
- Reasoning effort selector (None/Low/Medium/High/XHigh) in spawn wizard and agent detail
- max_tokens override field
- Agent detail: "Provider Config" section showing resolved driver, config, defaults
- "View Compiled Binding" expandable calling `GET /api/v1/agents/{id}/compiled`
- Inline MCP server distinction from global MCP servers
</requirements>

## Subtasks

- [x] 13.1 Add provider profiles section in Settings page (or new standalone page)
- [x] 13.2 Implement profile list — name, driver, model, defaults display
- [x] 13.3 Implement profile create form — driver selector -> dynamic config fields per driver
- [x] 13.4 Implement profile edit and delete
- [x] 13.5 Update spawn wizard Step 3 — replace model-catalog dropdown with Arky driver selector (10 drivers grouped by type)
- [x] 13.6 Add driver-specific config fields to spawn wizard — codex fields, claude-code fields, claude-compatible fields
- [x] 13.7 Add profile picker dropdown to spawn wizard (optional selection)
- [x] 13.8 Add reasoning effort selector (None/Low/Medium/High/XHigh) to spawn wizard
- [x] 13.9 Add max_tokens override field to spawn wizard
- [x] 13.10 Add "Provider Config" section to agent detail panel — resolved driver, model, profile, defaults, config
- [x] 13.11 Implement "View Compiled Binding" expandable — calls `GET /api/v1/agents/{id}/compiled`, displays ProviderBinding
- [x] 13.12 Add inline MCP server list in provider config section (distinct from global MCP in Skills page)
- [x] 13.13 Update `index_body.html` for all new UI sections

## Implementation Details

### Arky Driver Groups

```
Direct Providers:
  - codex (OpenAI Codex CLI)
  - claude-code (Claude Code CLI)

Gateway Providers:
  - openrouter
  - bedrock (AWS)
  - vertex (GCP)
  - ollama (Local)
  - zai
  - vercel
  - moonshot
  - minimax
```

### Driver-Specific Config Fields

When driver = `codex`:
- sandbox_mode: select (safe/unsafe)
- web_search: toggle
- reasoning_summary: select (off/brief/full)

When driver = `claude-code` or compatible:
- allowed_tools: multi-select or text list
- disallowed_tools: multi-select or text list
- max_budget_usd: number input
- fallback_model: text input
- mcp_servers: list of inline MCP configs

When driver = `bedrock`/`vertex`/compatible:
- selected_model: text (provider-specific model ID)
- region: text (e.g., us-east-1)
- project_id: text (Vertex only)

### API Endpoints Used

- `OpenFangAPI.v1.providerProfiles.*` (5 CRUD endpoints from task 12)
- `OpenFangAPI.v1.agents.compiled(id)` (compiled binding)
- Existing agent spawn/edit endpoints

### Relevant Files

- `crates/openfang-api/static/js/pages/agents.js` (MODIFY — spawn wizard, detail panel)
- `crates/openfang-api/static/js/pages/settings.js` (MODIFY — profiles section)
- `crates/openfang-api/static/index_body.html` (MODIFY)
- `crates/openfang-api/static/css/components.css` (MODIFY)

## Deliverables

- Provider profiles CRUD in Settings
- Spawn wizard with Arky drivers, driver-specific config, profile picker, reasoning effort
- Agent detail with provider config section and compiled binding inspector
- Inline MCP distinction from global MCP

## Tests

### Manual Browser Tests (Required)

- [ ] Provider profiles — create, list, edit, delete
- [ ] Spawn wizard — select each driver type, verify correct config fields appear
- [ ] Spawn wizard — select a profile, verify fields pre-fill
- [ ] Spawn wizard — set reasoning effort, verify it's included in agent definition
- [ ] Agent detail — verify Provider Config section displays resolved config
- [ ] Agent detail — expand "View Compiled Binding", verify data loads
- [ ] Verify inline MCP servers are distinct from global MCP servers

### Verification Commands

- [ ] `make fmt`
- [ ] `make lint`
- [ ] `make test`

## Success Criteria

- Provider profiles CRUD functional
- Spawn wizard uses Arky drivers, not legacy providers
- Driver-specific config fields work for all 3 namespaces
- Compiled binding inspector shows resolved provider data
