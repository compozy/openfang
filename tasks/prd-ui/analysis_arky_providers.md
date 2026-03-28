# Arky Provider System — UI Gap Analysis

> Generated: 2026-03-27
> Source: `crates/arky-*`, `crates/openfang-provider-binding/`, `crates/openfang-api/src/routes.rs`

---

## What the Arky Subsystem Adds

The Arky subsystem (10 crates under `crates/arky-*`) is a layered provider SDK embedded inside OpenFang. It replaces the flat model-catalog provider system with:

### 10 Arky Drivers

| Driver | Type | Description |
|--------|------|-------------|
| `codex` | Direct | OpenAI Codex CLI provider |
| `claude-code` | Direct | Claude Code CLI provider (default) |
| `openrouter` | Claude-compatible | OpenRouter gateway |
| `bedrock` | Claude-compatible | AWS Bedrock gateway |
| `vertex` | Claude-compatible | Google Vertex AI gateway |
| `ollama` | Claude-compatible | Ollama local provider |
| `zai` | Claude-compatible | Zai gateway |
| `vercel` | Claude-compatible | Vercel AI gateway |
| `moonshot` | Claude-compatible | Moonshot gateway |
| `minimax` | Claude-compatible | MiniMax gateway |

### 3-Tier Config Merging

```
Tier 1 (Workspace):  ProviderConfig       — binary path, env vars, timeouts, runtime dirs
Tier 2 (Profile):    ProviderProfileConfig — driver, model, defaults, behavior config
Tier 3 (Agent):      AgentConfig           — per-agent overrides of all above
```

Resolution: workspace < profile < agent (later tiers override earlier).

### Driver-Specific Typed Configs

**Codex** (`codex` driver):
- `sandbox_mode` — execution sandboxing
- `sandbox_network_access` — network in sandbox
- `include_plan_tool` — planning tool availability
- `resume_last` — resume previous session
- `web_search` — web search capability
- `rmcp_client` — RMCP client config
- `reasoning_summary` — reasoning trace mode
- `model_verbosity` — output verbosity level

**Claude Code** (`claude-code` driver):
- `continue_conversation` — continue existing session
- `fork_session` — fork from existing session
- `additional_directories` — extra working directories
- `enable_file_checkpointing` — file state snapshots
- `allowed_tools` — tool whitelist
- `disallowed_tools` — tool blacklist
- `mcp_servers` — inline MCP server configs (per-agent, passed to subprocess)
- `max_budget_usd` — per-agent budget cap
- `fallback_model` — fallback on primary failure

**Claude-Compatible** (bedrock, vertex, etc.):
- Inherits all `claude_code` fields, plus:
- `selected_model` — provider-specific model ID (e.g., `us.anthropic.claude-sonnet-4-20250514-v1:0`)
- `region` — cloud region (e.g., `us-east-1`)
- `project_id` — GCP project ID (Vertex only)

### ProviderRequestDefaults

- `max_tokens` — max output tokens override
- `reasoning_effort` — enum: None, Low, Medium, High, XHigh

---

## What the UI Currently Shows

The UI uses the **legacy model-catalog provider system**, not Arky:

| UI Location | What It Shows | Source |
|-------------|---------------|--------|
| Spawn wizard (Step 3) | Provider dropdown: groq, openai, anthropic, etc. | `GET /api/providers` (model-catalog) |
| Agent detail | `model_provider` string (e.g., "groq") | Legacy `AgentEntry` from `/api/agents` |
| Agent detail | `profile` string (e.g., "full", "coding") | Legacy tool profile, NOT Arky profile |
| Settings > Providers | API key management per model-catalog provider | `GET /api/providers` |

**None of the following are visible in the UI:**
- Arky driver selection
- Provider profiles (ProviderProfileConfig)
- Driver-specific config (sandbox_mode, mcp_servers, region, etc.)
- Reasoning effort / max_tokens overrides
- Compiled ProviderBinding
- Inline MCP servers vs global MCP servers distinction

---

## What's Missing from the Backend API

| Gap | Description |
|-----|-------------|
| `/api/v1/provider-profiles` CRUD | No endpoint exists for managing Arky provider profiles. The profile system lives in config files only. |
| `ValidationContext.known_profiles` | Always empty in `routes.rs` agent validation. Agent definitions referencing profiles pass validation silently without checking if the profile exists. |

---

## UI Features Needed

### 1. Provider Profiles Management

**Location**: Settings page (new tab) or standalone page
**Backend prerequisite**: `/api/v1/provider-profiles` CRUD endpoint

- List profiles: name, driver, model, defaults
- Create/edit: driver selector -> dynamic config fields based on driver
- Profile detail: full config display
- Delete with confirmation
- Used as reference in agent spawn wizard (profile picker dropdown)

### 2. Spawn Wizard Upgrade

**Location**: Agents page spawn modal (Step 3)

Current Step 3 shows model-catalog providers. Replace with:

- **Driver selector**: 10 Arky drivers grouped by type (Direct: codex, claude-code; Gateway: openrouter, bedrock, vertex, ollama, etc.)
- **Profile picker**: optional, from provider profiles
- **Model field**: aware of dual-model concept (display model vs provider_model_id for claude-compatible)
- **Reasoning effort**: dropdown (None/Low/Medium/High/XHigh)
- **max_tokens**: optional number input
- **Driver-specific config**: collapsible section showing fields for the selected driver

### 3. Agent Detail: Provider Config Section

**Location**: Agent detail panel (new tab or expandable section)

- **Resolved provider**: driver, model, profile reference
- **Defaults**: reasoning_effort badge, max_tokens value
- **Driver config**: read-only or editable fields for the resolved driver namespace
- **Compiled binding**: expandable "Debug" panel calling `GET /api/v1/agents/{id}/compiled` showing the resolved ProviderBinding
- **Inline MCP**: list of MCP servers from `provider.config.claude_code.mcp_servers` (distinct from global MCP in Skills page)

### 4. Provider Config in Workflow Steps

**Location**: Workflow v2 editor (Phase 3)

When editing an Agent step, the `provider_override` field allows per-step provider config. The workflow editor should support:
- Optional driver/model/profile override per step
- Reasoning effort override per step

---

## Relationship to Existing Phases

| Phase | Impact |
|-------|--------|
| Phase 3 (Workflows v2) | Agent steps may reference provider overrides — the step editor needs awareness of drivers and profiles |
| Phase 4 (Agents v1) | Agent list shows `AgentProviderSummary { driver, model, profile }` from v1 API — needs new columns |
| Phase 4.5 (NEW) | Full Arky provider UI: profiles management, spawn wizard upgrade, agent detail provider section, compiled binding inspector |

---

## Priority Assessment

| Feature | Priority | Rationale |
|---------|----------|-----------|
| Spawn wizard driver selector | High | Without it, users create agents with wrong provider abstraction |
| Reasoning effort control | High | Directly affects output quality and cost |
| Provider profiles management | Medium | Profiles can be managed via config files as fallback |
| Driver-specific config editor | Medium | Power-user feature, config files work as fallback |
| Compiled binding inspector | Low | Debugging tool, useful but not essential |
| Inline MCP distinction | Low | Edge case for advanced users |
