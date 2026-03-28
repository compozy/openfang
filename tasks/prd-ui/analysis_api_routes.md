# OpenFang API Routes — Full Analysis for UI Integration

Generated from: `crates/openfang-api/src/server.rs`, `routes.rs`, `ws.rs`, `webchat.rs`, `channel_bridge.rs`, `middleware.rs`, `rate_limiter.rs`
Cross-referenced with: `crates/openfang-api/static/js/api.js` and all `static/js/pages/*.js`

---

## Auth Model

- Auth is disabled when `api_key` is empty/whitespace in config (development default).
- When enabled, requests must carry `Authorization: Bearer <api_key>` or `X-API-Key` header.
- WebSocket upgrades authenticate via `Authorization` header OR `?token=` query param.
- SSE endpoints also accept `?token=` for EventSource clients.
- Session cookies (`openfang_session`) are supported when dashboard auth (`auth.enabled`) is active.
- Rate limiting: GCRA, 500 tokens/minute per IP. Costs range from 1 (health) to 100 (run/migrate).

### Public (no-auth) endpoints (GET unless noted)
`/`, `/logo.png`, `/favicon.ico`, `/api/health`, `/api/health/detail`, `/api/status`, `/api/version`,
`/api/agents` (GET only), `/api/profiles`, `/api/config`, `/api/config/schema`, `/api/uploads/*`,
`/api/models`, `/api/models/aliases`, `/api/providers`, `/api/budget`, `/api/budget/agents`,
`/api/budget/agents/*`, `/api/network/status`, `/api/a2a/agents`, `/api/approvals`,
`/api/channels`, `/api/hands`, `/api/hands/active`, `/api/hands/*`, `/api/skills`,
`/api/sessions`, `/api/integrations`, `/api/integrations/available`, `/api/integrations/health`,
`/api/v1/workflows` (GET), `/api/logs/stream`, `/api/cron/*` (GET),
`/api/providers/github-copilot/oauth/*`, `/api/auth/login`, `/api/auth/logout`, `/api/auth/check`,
`/.well-known/agent.json`, `/a2a/*` (GET)

---

## Summary Statistics

| Category | Total Endpoints | Used by UI | NOT used by UI |
|----------|----------------|------------|----------------|
| Static / PWA | 5 | 5 | 0 |
| Health & System | 6 | 5 | 1 |
| Auth | 3 | 3 | 0 |
| Agents (legacy) | 18 | 14 | 4 |
| Agents (v1) | 14 | 0 | 14 |
| Sessions (legacy) | 8 | 7 | 1 |
| Memory / KV | 4 | 4 | 0 |
| Files & Uploads | 4 | 4 | 0 |
| Channels | 7 | 7 | 0 |
| Workflows (legacy) | 6 | 5 | 1 |
| Workflows (v1) | 10 | 0 | 10 |
| Triggers (legacy) | 4 | 2 | 2 |
| Triggers (v1) | 10 | 0 | 10 |
| Schedules (v1) | 9 | 0 | 9 |
| Cron (legacy) | 6 | 5 | 1 |
| Events (v1) | 2 | 0 | 2 |
| Packs (v1) | 8 | 0 | 8 |
| Skills (legacy) | 5 | 4 | 1 |
| Skills (v1) | 2 | 0 | 2 |
| ClawHub | 5 | 5 | 0 |
| Hands | 12 | 11 | 1 |
| MCP | 2 | 1 | 1 |
| Audit | 2 | 2 | 0 |
| Logs (SSE) | 1 | 1 | 0 |
| Peers / Network | 2 | 2 | 0 |
| Comms | 5 | 4 | 1 |
| Tools | 1 | 1 | 0 |
| Config | 4 | 4 | 0 |
| Approvals | 3 | 3 | 0 |
| Usage | 4 | 4 | 0 |
| Budget | 4 | 1 | 3 |
| Models | 5 | 4 | 1 |
| Providers | 6 | 6 | 0 |
| Security | 1 | 1 | 0 |
| Sessions (global) | 3 | 3 | 0 |
| Templates | 2 | 2 | 0 |
| Integrations | 6 | 0 | 6 |
| Pairing | 5 | 0 | 5 |
| Migrate | 3 | 3 | 0 |
| Bindings | 3 | 0 | 3 |
| A2A (inbound protocol) | 5 | 0 | 5 |
| A2A (outbound mgmt) | 4 | 2 | 2 |
| Webhook triggers | 2 | 0 | 2 |
| Commands | 1 | 1 | 0 |
| Shutdown | 1 | 0 | 1 |
| Runs (v1) | 9 | 0 | 9 |
| Dispatches (v1) | 6 | 0 | 6 |
| HITL (v1) | 5 | 0 | 5 |
| Artifacts (v1) | 3 | 0 | 3 |
| Docs (v1) | 3 | 0 | 3 |
| Tasks (v1) | 10 | 0 | 10 |
| Subtasks (v1) | 4 | 0 | 4 |
| Looper Runs (v1) | 8 | 0 | 8 |
| OpenAI-compat | 2 | 0 | 2 |
| MCP HTTP | 1 | 0 | 1 |
| Copilot OAuth | 2 | 2 | 0 |

**Overall: ~230 total endpoints. UI currently uses ~90 of them. ~140 are NOT used by the UI.**

**Critical finding: The entire `/api/v1/` namespace (agents v1, workflows v1, triggers v1, schedules v1, runs, dispatches, HITL, artifacts, docs, tasks, subtasks, looper-runs, packs, skills v1) is completely unused by the UI. The UI still relies on the legacy `/api/agents`, `/api/workflows`, and `/api/triggers` endpoints.**

---

## Domain Groups

---

### 1. Static / PWA

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/` | Dashboard SPA (index_body.html) | Public | — | YES |
| GET | `/logo.png` | Logo image | Public | — | YES |
| GET | `/favicon.ico` | Favicon | Public | — | YES |
| GET | `/manifest.json` | PWA manifest | Public | — | YES |
| GET | `/sw.js` | PWA service worker | Public | — | YES |

**None unused.**

---

### 2. Health & System

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/health` | Simple health check → `{"status":"ok"}` | Public | — | YES (overview) |
| GET | `/api/health/detail` | Detailed health with subsystem status | Public | — | NO |
| GET | `/api/status` | Daemon status with uptime, agent counts, memory, version | Public | — | YES (app.js, settings, chat, runtime, agents) |
| GET | `/api/version` | Version string | Public | — | YES (settings) |
| GET | `/api/metrics` | Prometheus metrics endpoint | Required | — | NO |
| POST | `/api/shutdown` | Graceful shutdown (loopback-only, no auth check) | Loopback | — | NO |

**NOT used by UI: `/api/health/detail`, `/api/metrics`, `/api/shutdown`**

---

### 3. Authentication

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| POST | `/api/auth/login` | Login with username/password → session cookie | Public | — | YES (app.js) |
| POST | `/api/auth/logout` | Invalidate session | Public | — | YES (app.js) |
| GET | `/api/auth/check` | Check if session/key is valid | Public | — | YES (app.js) |

**None unused.**

---

### 4. Agents — Legacy API (`/api/agents`)

The UI uses these legacy endpoints. They coexist with the newer `/api/v1/agents` namespace.

#### Request/Response Notes

- `POST /api/agents`: body `{ manifest_toml: string }` → `{ agent_id, name, state }`
- `GET /api/agents/{id}`: → `AgentEntry` with id, name, state, manifest, session info
- `PATCH /api/agents/{id}`: body `{ name?, system_prompt?, model? }` → updated agent
- `DELETE /api/agents/{id}` / `POST /api/agents/{id}/stop`: stop/kill agent
- `POST /api/agents/{id}/message`: body `{ message: string }` → `{ response, input_tokens, output_tokens, ... }`
- `POST /api/agents/{id}/message/stream`: SSE stream of token deltas
- `GET /api/agents/{id}/ws`: WebSocket upgrade for real-time bidirectional chat

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/agents` | List all agents | Public | — | YES (app.js, runtime, workflow-builder) |
| POST | `/api/agents` | Spawn new agent from TOML manifest | Required | — | YES (agents.js, wizard.js) |
| GET | `/api/agents/{id}` | Get agent details | Required | — | YES (agents.js) |
| DELETE | `/api/agents/{id}` | Kill agent | Required | — | YES (agents.js, chat.js) |
| PATCH | `/api/agents/{id}` | Patch agent (name/prompt/model) | Required | — | NO |
| PUT | `/api/agents/{id}/mode` | Set agent mode (auto/manual/paused) | Required | — | YES (agents.js) |
| POST | `/api/agents/{id}/restart` | Restart agent | Required | — | NO |
| POST | `/api/agents/{id}/start` | Alias for restart | Required | — | NO |
| POST | `/api/agents/{id}/message` | Send message (blocking) | Required | — | YES (wizard.js, chat.js HTTP fallback) |
| POST | `/api/agents/{id}/message/stream` | Send message with SSE streaming | Required | SSE | NO |
| GET | `/api/agents/{id}/session` | Get active session info | Required | — | YES (chat.js) |
| POST | `/api/agents/{id}/session/reset` | Reset/clear session history | Required | — | YES (chat.js) |
| POST | `/api/agents/{id}/session/compact` | Compact session context | Required | — | YES (chat.js) |
| DELETE | `/api/agents/{id}/history` | Clear conversation history | Required | — | YES (agents.js) |
| POST | `/api/agents/{id}/stop` | Cancel active run | Required | — | YES (chat.js) |
| PUT | `/api/agents/{id}/model` | Switch model | Required | — | YES (agents.js, chat.js) |
| GET | `/api/agents/{id}/tools` | Get tool filter list | Required | — | YES (agents.js) |
| PUT | `/api/agents/{id}/tools` | Set tool filter list | Required | — | YES (agents.js) |
| GET | `/api/agents/{id}/skills` | Get agent skills | Required | — | NO |
| PUT | `/api/agents/{id}/skills` | Set agent skills | Required | — | NO |
| GET | `/api/agents/{id}/mcp_servers` | Get MCP servers for agent | Required | — | NO |
| PUT | `/api/agents/{id}/mcp_servers` | Set MCP servers for agent | Required | — | NO |
| PATCH | `/api/agents/{id}/identity` | Update agent identity (name, avatar, etc.) | Required | — | NO |
| PATCH | `/api/agents/{id}/config` | Patch agent config fields | Required | — | YES (agents.js) |
| POST | `/api/agents/{id}/clone` | Clone agent | Required | — | YES (agents.js) |
| GET | `/api/agents/{id}/files` | List agent workspace files | Required | — | YES (agents.js) |
| GET | `/api/agents/{id}/files/{filename}` | Read a workspace file | Required | — | YES (agents.js) |
| PUT | `/api/agents/{id}/files/{filename}` | Write a workspace file | Required | — | YES (agents.js) |
| GET | `/api/agents/{id}/deliveries` | Get delivery receipts | Required | — | NO |
| POST | `/api/agents/{id}/upload` | Upload file attachment | Required | — | YES (chat.js, agents.js) |
| GET | `/api/agents/{id}/ws` | WebSocket upgrade for real-time chat | Required (Bearer or ?token=) | **WebSocket** | YES (chat.js) |
| GET | `/api/uploads/{file_id}` | Serve uploaded file | Public | — | YES (implicitly via chat) |
| GET | `/api/profiles` | List available agent profiles | Public | — | YES (agents.js) |
| PUT | `/api/agents/{id}/update` | Legacy update agent | Required | — | NO |

**NOT used by UI:** `PATCH /api/agents/{id}` (use `PATCH /config` instead), `POST /api/agents/{id}/restart`, `POST /api/agents/{id}/start`, `POST /api/agents/{id}/message/stream` (uses WS instead), `GET/PUT /api/agents/{id}/skills`, `GET/PUT /api/agents/{id}/mcp_servers`, `PATCH /api/agents/{id}/identity`, `GET /api/agents/{id}/deliveries`, `PUT /api/agents/{id}/update`

**WebSocket protocol** (`/api/agents/{id}/ws`):
- Client → Server: `{"type":"message","content":"...","attachments":[{"file_id":"..."}]}`
- Client → Server: `{"type":"command","command":"new|reset|compact|stop|model|usage|context|verbose|queue|budget|peers|a2a","args":"..."}`
- Client → Server: `{"type":"ping"}`
- Server → Client: `{"type":"connected","agent_id":"..."}`, `{"type":"typing","state":"start|tool|stop","tool":"..."}`, `{"type":"text_delta","content":"..."}`, `{"type":"tool_start","id":"...","tool":"..."}`, `{"type":"tool_end","id":"...","tool":"...","input":"..."}`, `{"type":"tool_result","id":"...","tool":"...","result":"...","is_error":bool}`, `{"type":"response","content":"...","input_tokens":N,"output_tokens":N,"iterations":N,"cost_usd":null,"context_pressure":"low|medium|high|critical"}`, `{"type":"phase","phase":"...","detail":"..."}`, `{"type":"silent_complete","input_tokens":N,"output_tokens":N}`, `{"type":"canvas","canvas_id":"...","html":"...","title":"..."}`, `{"type":"error","content":"..."}`, `{"type":"agents_updated","agents":[...]}`, `{"type":"command_result","command":"...","message":"..."}`, `{"type":"pong"}`
- Max 5 concurrent WS connections per IP. 30 min idle timeout. 10 msg/min rate limit. 64 KB max message.

---

### 5. Agents — v1 API (`/api/v1/agents`)

These are ALL unused by the UI. They implement a richer control plane with compiled definitions and per-session management.

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| POST | `/api/v1/agents/validate` | Validate agent definition TOML | Required | — | **NO** |
| POST | `/api/v1/agents/compile` | Compile agent definition to IR | Required | — | **NO** |
| GET | `/api/v1/agents` | List agents (v1 format) | Required | — | **NO** |
| POST | `/api/v1/agents` | Create agent (v1 format) | Required | — | **NO** |
| GET | `/api/v1/agents/{id}` | Get agent (v1 format) | Required | — | **NO** |
| PUT | `/api/v1/agents/{id}` | Update agent (v1 format) | Required | — | **NO** |
| DELETE | `/api/v1/agents/{id}` | Delete agent (v1 format) | Required | — | **NO** |
| GET | `/api/v1/agents/{id}/compiled` | Get compiled agent IR | Required | — | **NO** |
| GET | `/api/v1/agents/{id}/runtime` | Get agent runtime status | Required | — | **NO** |
| POST | `/api/v1/agents/{id}/runtime/start` | Start agent runtime | Required | — | **NO** |
| POST | `/api/v1/agents/{id}/runtime/stop` | Stop agent runtime | Required | — | **NO** |
| POST | `/api/v1/agents/{id}/runtime/restart` | Restart agent runtime | Required | — | **NO** |
| PUT | `/api/v1/agents/{id}/runtime/mode` | Set agent runtime mode | Required | — | **NO** |
| GET | `/api/v1/agents/{id}/sessions` | List sessions (v1) | Required | — | **NO** |
| POST | `/api/v1/agents/{id}/sessions` | Create session (v1) | Required | — | **NO** |
| GET | `/api/v1/agents/{id}/sessions/{session_id}` | Get session (v1) | Required | — | **NO** |
| POST | `/api/v1/agents/{id}/sessions/{session_id}/activate` | Activate session | Required | — | **NO** |
| POST | `/api/v1/agents/{id}/sessions/{session_id}/reset` | Reset session | Required | — | **NO** |
| POST | `/api/v1/agents/{id}/sessions/{session_id}/compact` | Compact session | Required | — | **NO** |
| POST | `/api/v1/agents/{id}/messages` | Submit message (v1) | Required | — | **NO** |
| POST | `/api/v1/agents/{id}/messages/stream` | Stream message (v1) | Required | SSE | **NO** |
| POST | `/api/v1/agents/{id}/messages/dry-run` | Dry-run message (v1) | Required | — | **NO** |

---

### 6. Sessions — Legacy (`/api/agents/{id}/sessions`)

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/agents/{id}/sessions` | List sessions for agent | Required | — | YES (chat.js) |
| POST | `/api/agents/{id}/sessions` | Create new session | Required | — | YES (chat.js) |
| POST | `/api/agents/{id}/sessions/{session_id}/switch` | Switch active session | Required | — | YES (chat.js) |

**Global Sessions:**

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/sessions` | List all sessions across all agents | Public | — | YES (sessions.js) |
| DELETE | `/api/sessions/{id}` | Delete a session | Required | — | YES (sessions.js) |
| PUT | `/api/sessions/{id}/label` | Set a label on a session | Required | — | NO |
| GET | `/api/agents/{id}/sessions/by-label/{label}` | Find session by label | Required | — | NO |

**NOT used by UI:** `PUT /api/sessions/{id}/label`, `GET /api/agents/{id}/sessions/by-label/{label}`

---

### 7. Memory / KV Store

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/memory/agents/{id}/kv` | Get all KV entries for agent | Required | — | YES (sessions.js, hands.js) |
| GET | `/api/memory/agents/{id}/kv/{key}` | Get single KV entry | Required | — | YES (hands.js) |
| PUT | `/api/memory/agents/{id}/kv/{key}` | Set KV entry | Required | — | YES (sessions.js) |
| DELETE | `/api/memory/agents/{id}/kv/{key}` | Delete KV entry | Required | — | YES (sessions.js) |

**None unused.**

---

### 8. Channels

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/channels` | List all channel adapters and their status | Public | — | YES (channels.js, overview.js) |
| POST | `/api/channels/{name}/configure` | Configure a channel adapter | Required | — | YES (channels.js, wizard.js) |
| DELETE | `/api/channels/{name}/configure` | Remove channel config | Required | — | YES (channels.js) |
| POST | `/api/channels/{name}/test` | Test channel connection | Required | — | YES (channels.js) |
| POST | `/api/channels/reload` | Hot-reload all channel bridges | Required | — | NO |
| POST | `/api/channels/whatsapp/qr/start` | Start WhatsApp QR login flow | Required | — | YES (channels.js) |
| GET | `/api/channels/whatsapp/qr/status` | Poll WhatsApp QR login status | Required | — | YES (channels.js) |

**NOT used by UI:** `POST /api/channels/reload`

---

### 9. Templates

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/templates` | List available agent templates | Required | — | YES (agents.js) |
| GET | `/api/templates/{name}` | Get template details | Required | — | YES (agents.js) |

**None unused.**

---

### 10. Workflows — Legacy API (`/api/workflows`)

The UI uses these non-v1 endpoints. They are registered in routes.rs but the path is inferred from the handler doc comments.

**Note:** The legacy `/api/workflows` routes ARE in `routes.rs` handlers but their registration in `server.rs` was not found, meaning the UI calls to `/api/workflows` (in workflows.js) may be hitting unregistered routes. The v1 equivalents at `/api/v1/workflows` ARE registered.

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/workflows` | List all workflows | Required | — | YES (workflows.js) — but path may not be registered |
| POST | `/api/workflows` | Create workflow | Required | — | YES (workflows.js, workflow-builder.js) |
| GET | `/api/workflows/{id}` | Get workflow | Required | — | YES (workflows.js) |
| PUT | `/api/workflows/{id}` | Update workflow | Required | — | YES (workflows.js) |
| DELETE | `/api/workflows/{id}` | Delete workflow | Required | — | YES (workflows.js) |
| POST | `/api/workflows/{id}/run` | Execute workflow | Required | — | YES (workflows.js) |
| GET | `/api/workflows/{id}/runs` | List runs for workflow | Required | — | YES (workflows.js) |

**NOT used by UI via these legacy paths:** none (all used), but **the entire v1 workflow API is unused.**

---

### 11. Workflows — v1 API (`/api/v1/workflows`)

All unused by UI.

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/v1/workflows` | List workflow definitions | Public | — | **NO** |
| POST | `/api/v1/workflows` | Create workflow definition | Required | — | **NO** |
| GET | `/api/v1/workflows/{id}` | Get workflow definition | Required | — | **NO** |
| PUT | `/api/v1/workflows/{id}` | Update workflow definition | Required | — | **NO** |
| DELETE | `/api/v1/workflows/{id}` | Delete workflow definition | Required | — | **NO** |
| POST | `/api/v1/workflows/validate` | Validate workflow TOML | Required | — | **NO** |
| POST | `/api/v1/workflows/compile` | Compile workflow to IR | Required | — | **NO** |
| GET | `/api/v1/workflows/{id}/compiled` | Get compiled workflow IR | Required | — | **NO** |
| POST | `/api/v1/workflows/{id}/fork` | Fork workflow definition | Required | — | **NO** |
| GET | `/api/v1/workflows/{id}/runs` | List workflow runs (v1) | Required | — | **NO** |
| POST | `/api/v1/workflows/{id}/runs` | Start workflow run (v1) | Required | — | **NO** |
| POST | `/api/v1/workflows/{id}/runs/dry-run` | Dry-run workflow run | Required | — | **NO** |
| GET | `/api/v1/workflows/{id}/runtime` | Get workflow runtime status | Required | — | **NO** |

---

### 12. Triggers — Legacy API (`/api/triggers`)

The scheduler.js calls `/api/triggers` (GET) and `/api/triggers/{id}` (PUT, DELETE). The server.rs does NOT have a registered route for `/api/triggers` — only `/api/v1/triggers` is registered. This appears to be a UI bug where the scheduler page calls non-existent routes.

| Method | Path | Description | Auth | WS/SSE | UI Uses? | Registered? |
|--------|------|-------------|------|--------|----------|-------------|
| GET | `/api/triggers` | List triggers | Required | — | YES (scheduler.js) | **NO — NOT REGISTERED** |
| POST | `/api/triggers` | Create trigger | Required | — | NO | **NO — NOT REGISTERED** |
| DELETE | `/api/triggers/{id}` | Delete trigger | Required | — | YES (scheduler.js) | **NO — NOT REGISTERED** |
| PUT | `/api/triggers/{id}` | Update trigger (enable/disable) | Required | — | YES (scheduler.js) | **NO — NOT REGISTERED** |

**BUG: The scheduler.js UI calls `/api/triggers` routes that are not registered. These will return 404.**

---

### 13. Triggers — v1 API (`/api/v1/triggers`)

All unused by UI.

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/v1/triggers` | List trigger definitions | Required | — | **NO** |
| POST | `/api/v1/triggers` | Create trigger definition | Required | — | **NO** |
| GET | `/api/v1/triggers/{id}` | Get trigger definition | Required | — | **NO** |
| PUT | `/api/v1/triggers/{id}` | Update trigger definition | Required | — | **NO** |
| DELETE | `/api/v1/triggers/{id}` | Delete trigger definition | Required | — | **NO** |
| POST | `/api/v1/triggers/validate` | Validate trigger definition | Required | — | **NO** |
| POST | `/api/v1/triggers/compile` | Compile trigger definition | Required | — | **NO** |
| GET | `/api/v1/triggers/{id}/compiled` | Get compiled trigger IR | Required | — | **NO** |
| POST | `/api/v1/triggers/{id}/fork` | Fork trigger definition | Required | — | **NO** |
| GET | `/api/v1/triggers/{id}/runtime` | Get trigger runtime status | Required | — | **NO** |
| POST | `/api/v1/triggers/{id}/enable` | Enable trigger | Required | — | **NO** |
| POST | `/api/v1/triggers/{id}/disable` | Disable trigger | Required | — | **NO** |
| POST | `/api/v1/triggers/{id}/test` | Test trigger firing | Required | — | **NO** |

---

### 14. Schedules — v1 API (`/api/v1/schedules`)

All unused by UI.

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/v1/schedules` | List schedule definitions | Required | — | **NO** |
| POST | `/api/v1/schedules` | Create schedule definition | Required | — | **NO** |
| GET | `/api/v1/schedules/{id}` | Get schedule definition | Required | — | **NO** |
| PUT | `/api/v1/schedules/{id}` | Update schedule definition | Required | — | **NO** |
| DELETE | `/api/v1/schedules/{id}` | Delete schedule definition | Required | — | **NO** |
| POST | `/api/v1/schedules/validate` | Validate schedule definition | Required | — | **NO** |
| POST | `/api/v1/schedules/{id}/fork` | Fork schedule definition | Required | — | **NO** |
| GET | `/api/v1/schedules/{id}/runtime` | Get schedule runtime status | Required | — | **NO** |
| POST | `/api/v1/schedules/{id}/enable` | Enable schedule | Required | — | **NO** |
| POST | `/api/v1/schedules/{id}/disable` | Disable schedule | Required | — | **NO** |
| POST | `/api/v1/schedules/{id}/run-now` | Run schedule immediately | Required | — | **NO** |
| POST | `/api/v1/schedules/{id}/run-now/dry-run` | Dry-run schedule | Required | — | **NO** |

---

### 15. Cron Jobs — Legacy API (`/api/cron`)

The scheduler.js uses these extensively.

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/cron/jobs` | List all cron jobs | Public | — | YES (scheduler.js) |
| POST | `/api/cron/jobs` | Create cron job | Required | — | YES (scheduler.js) |
| DELETE | `/api/cron/jobs/{id}` | Delete cron job | Required | — | YES (scheduler.js) |
| PUT | `/api/cron/jobs/{id}/enable` | Enable/disable cron job | Required | — | YES (scheduler.js) |
| GET | `/api/cron/jobs/{id}/status` | Get cron job status | Public | — | NO |
| POST | `/api/cron/jobs/{id}/run` | Run cron job immediately | Required | — | YES (scheduler.js) |

**NOT used by UI:** `GET /api/cron/jobs/{id}/status`

---

### 16. Events — v1 API

All unused by UI.

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| POST | `/api/v1/events` | Post event for trigger evaluation | Required | — | **NO** |
| POST | `/api/v1/events/dry-run` | Dry-run event evaluation | Required | — | **NO** |

---

### 17. Packs — v1 API (`/api/v1/packs`)

All unused by UI.

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/v1/packs` | List installed packs | Required | — | **NO** |
| POST | `/api/v1/packs/install` | Install a pack | Required | — | **NO** |
| GET | `/api/v1/packs/{id}` | Get pack details | Required | — | **NO** |
| GET | `/api/v1/packs/{id}/objects` | List pack objects | Required | — | **NO** |
| POST | `/api/v1/packs/{id}/upgrade` | Upgrade pack | Required | — | **NO** |
| POST | `/api/v1/packs/{id}/upgrade/dry-run` | Dry-run pack upgrade | Required | — | **NO** |
| POST | `/api/v1/packs/{id}/uninstall` | Uninstall pack | Required | — | **NO** |
| POST | `/api/v1/packs/{id}/fork` | Fork pack object | Required | — | **NO** |

---

### 18. Skills — Legacy API (`/api/skills`)

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/skills` | List installed skills | Public | — | YES (skills.js, overview.js, app.js) |
| POST | `/api/skills/install` | Install skill from file | Required | — | NO |
| POST | `/api/skills/uninstall` | Uninstall skill by name | Required | — | YES (skills.js) |
| POST | `/api/skills/reload` | Reload all skills | Required | — | NO |
| POST | `/api/skills/create` | Create new skill (from UI) | Required | — | YES (skills.js) |
| GET | `/api/marketplace/search` | Search marketplace (deprecated) | Required | — | NO |

**NOT used by UI:** `POST /api/skills/install`, `POST /api/skills/reload`, `GET /api/marketplace/search`

---

### 19. Skills — v1 API (`/api/v1/skills`)

All unused by UI.

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/v1/skills` | List skills (v1 format) | Required | — | **NO** |
| GET | `/api/v1/skills/{id}` | Get skill (v1 format) | Required | — | **NO** |

---

### 20. ClawHub (OpenClaw Marketplace)

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/clawhub/search?q=&limit=` | Search ClawHub registry | Required | — | YES (skills.js) |
| GET | `/api/clawhub/browse?sort=&limit=&cursor=` | Browse ClawHub skills | Required | — | YES (skills.js) |
| GET | `/api/clawhub/skill/{slug}` | Get skill detail from ClawHub | Required | — | YES (skills.js) |
| GET | `/api/clawhub/skill/{slug}/code` | Get skill source code | Required | — | YES (skills.js) |
| POST | `/api/clawhub/install` | Install skill from ClawHub | Required | — | YES (skills.js) |

**None unused.**

---

### 21. Hands (Integrations/Automations)

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/hands` | List all hands (definitions) | Public | — | YES (hands.js) |
| POST | `/api/hands/install` | Install hand from file | Required | — | NO |
| POST | `/api/hands/upsert` | Upsert hand definition | Required | — | NO |
| GET | `/api/hands/active` | List active hand instances | Public | — | YES (hands.js) |
| GET | `/api/hands/{hand_id}` | Get hand definition details | Public | — | YES (hands.js) |
| POST | `/api/hands/{hand_id}/activate` | Activate hand (create instance) | Required | — | YES (hands.js) |
| POST | `/api/hands/{hand_id}/check-deps` | Check hand dependencies | Required | — | YES (hands.js) |
| POST | `/api/hands/{hand_id}/install-deps` | Install hand dependencies | Required | — | YES (hands.js) |
| GET | `/api/hands/{hand_id}/settings` | Get hand settings | Required | — | NO |
| PUT | `/api/hands/{hand_id}/settings` | Update hand settings | Required | — | NO |
| POST | `/api/hands/instances/{id}/pause` | Pause hand instance | Required | — | YES (hands.js) |
| POST | `/api/hands/instances/{id}/resume` | Resume hand instance | Required | — | YES (hands.js) |
| DELETE | `/api/hands/instances/{id}` | Deactivate hand instance | Required | — | YES (hands.js) |
| GET | `/api/hands/instances/{id}/stats` | Get hand instance stats | Required | — | YES (hands.js) |
| GET | `/api/hands/instances/{id}/browser` | Get browser instance URL/info | Required | — | YES (hands.js) |

**NOT used by UI:** `POST /api/hands/install`, `POST /api/hands/upsert`, `GET/PUT /api/hands/{hand_id}/settings`

---

### 22. MCP Servers

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/mcp/servers` | List MCP servers for all agents | Required | — | YES (skills.js, overview.js) |
| POST | `/mcp` | MCP protocol endpoint (HTTP transport) | Required | — | **NO** |

**NOT used by UI:** `POST /mcp`

---

### 23. Audit

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/audit/recent?n=` | Recent audit events (default 20, max 200) | Required | — | YES (overview.js, logs.js) |
| GET | `/api/audit/verify` | Verify audit log integrity | Required | — | YES (settings.js, logs.js) |

**None unused.**

---

### 24. Logs (SSE)

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/logs/stream` | SSE stream of live daemon logs | Public (auth via ?token=) | **SSE** | YES (logs.js) |

**None unused.**

---

### 25. Peers / Network

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/peers` | List OFP peer nodes | Required | — | YES (settings.js) |
| GET | `/api/network/status` | OFP network status | Public | — | YES (chat.js) |

**None unused.**

---

### 26. Agent Comms (Multi-Agent Communication)

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/comms/topology` | Get agent communication topology graph | Required | — | YES (comms.js) |
| GET | `/api/comms/events?limit=` | Get recent inter-agent events | Required | — | YES (comms.js) |
| GET | `/api/comms/events/stream` | SSE stream of inter-agent events | Required | **SSE** | NO |
| POST | `/api/comms/send` | Send message to another agent | Required | — | YES (comms.js) |
| POST | `/api/comms/task` | Assign task to another agent | Required | — | YES (comms.js) |

**NOT used by UI:** `GET /api/comms/events/stream` (SSE stream, uses polling instead)

---

### 27. Tools

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/tools` | List all available built-in tools | Required | — | YES (settings.js, app.js) |

---

### 28. Config

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/config` | Get full config (redacted secrets) | Public | — | YES (settings.js, app.js) |
| GET | `/api/config/schema` | Get config JSON schema | Public | — | YES (settings.js) |
| POST | `/api/config/set` | Set a config field by JSON path | Required | — | YES (settings.js) |
| POST | `/api/config/reload` | Reload config from disk | Required | — | NO |

**NOT used by UI:** `POST /api/config/reload` (redundant with auto-reload)

---

### 29. Approvals

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/approvals` | List pending approvals | Public | — | YES (approvals.js, app.js) |
| POST | `/api/approvals` | Create approval request (internal) | Required | — | NO |
| POST | `/api/approvals/{id}/approve` | Approve a request | Required | — | YES (approvals.js) |
| POST | `/api/approvals/{id}/reject` | Reject a request | Required | — | YES (approvals.js) |

**NOT used by UI:** `POST /api/approvals` (created internally by the runtime, not the UI)

---

### 30. Usage Statistics

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/usage` | Overall token usage stats | Required | — | YES (settings.js, overview.js, usage.js) |
| GET | `/api/usage/summary` | Usage summary (totals) | Required | — | YES (usage.js) |
| GET | `/api/usage/by-model` | Usage breakdown by model | Required | — | YES (usage.js) |
| GET | `/api/usage/daily` | Daily usage over time | Required | — | YES (usage.js) |

**None unused.**

---

### 31. Budget

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/budget` | Get budget status (hourly/daily/monthly spend vs limits) | Public | — | YES (chat.js `/budget` command) |
| PUT | `/api/budget` | Update budget limits | Required | — | **NO** |
| GET | `/api/budget/agents` | Per-agent budget ranking | Public | — | **NO** |
| GET | `/api/budget/agents/{id}` | Single agent budget detail | Public | — | **NO** |
| PUT | `/api/budget/agents/{id}` | Update agent budget limits | Required | — | **NO** |

**NOT used by UI:** `PUT /api/budget`, `GET /api/budget/agents`, `GET /api/budget/agents/{id}`, `PUT /api/budget/agents/{id}`. The `/api/budget` endpoint is only called from within a chat slash-command handler, not from a dedicated UI section.

---

### 32. Models & Providers

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/models` | List all available models with capabilities and pricing | Public | — | YES (settings.js, agents.js, chat.js) |
| GET | `/api/models/aliases` | List model aliases | Public | — | NO |
| POST | `/api/models/custom` | Add custom model definition | Required | — | YES (settings.js) |
| DELETE | `/api/models/custom/{id}` | Remove custom model | Required | — | YES (settings.js) |
| GET | `/api/models/{id}` | Get single model details | Required | — | NO |
| GET | `/api/providers` | List providers with auth status | Public | — | YES (settings.js, agents.js, wizard.js, runtime.js, overview.js) |
| POST | `/api/providers/{name}/key` | Set provider API key | Required | — | YES (settings.js, wizard.js) |
| DELETE | `/api/providers/{name}/key` | Remove provider API key | Required | — | YES (settings.js) |
| POST | `/api/providers/{name}/test` | Test provider connection | Required | — | YES (settings.js, wizard.js) |
| PUT | `/api/providers/{name}/url` | Set provider base URL (Ollama, OpenAI-compat) | Required | — | YES (settings.js) |
| POST | `/api/providers/github-copilot/oauth/start` | Start Copilot OAuth flow | Public | — | YES (settings.js, wizard.js) |
| GET | `/api/providers/github-copilot/oauth/poll/{poll_id}` | Poll Copilot OAuth status | Public | — | YES (settings.js) |

**NOT used by UI:** `GET /api/models/aliases`, `GET /api/models/{id}`

---

### 33. Security Dashboard

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/security` | Security status overview (CORS, auth, key status) | Required | — | YES (settings.js) |

---

### 34. Integrations

All unused by UI.

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/integrations` | List active integrations | Public | — | **NO** |
| GET | `/api/integrations/available` | List available integration types | Public | — | **NO** |
| POST | `/api/integrations/add` | Add integration | Required | — | **NO** |
| DELETE | `/api/integrations/{id}` | Remove integration | Required | — | **NO** |
| POST | `/api/integrations/{id}/reconnect` | Reconnect integration | Required | — | **NO** |
| GET | `/api/integrations/health` | Integration health check | Public | — | **NO** |
| POST | `/api/integrations/reload` | Reload integrations | Required | — | **NO** |

---

### 35. Device Pairing

All unused by UI.

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| POST | `/api/pairing/request` | Request device pairing | Required | — | **NO** |
| POST | `/api/pairing/complete` | Complete pairing handshake | Required | — | **NO** |
| GET | `/api/pairing/devices` | List paired devices | Required | — | **NO** |
| DELETE | `/api/pairing/devices/{id}` | Remove paired device | Required | — | **NO** |
| POST | `/api/pairing/notify` | Send notification to paired device | Required | — | **NO** |

---

### 36. Migration

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/migrate/detect` | Detect migratable agents/configs | Required | — | YES (settings.js) |
| POST | `/api/migrate/scan` | Scan a path for agents to migrate | Required | — | YES (settings.js) |
| POST | `/api/migrate` | Execute migration | Required | — | YES (settings.js) |

**None unused.**

---

### 37. Bindings

All unused by UI.

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/bindings` | List agent-channel bindings | Required | — | **NO** |
| POST | `/api/bindings` | Add agent-channel binding | Required | — | **NO** |
| DELETE | `/api/bindings/{index}` | Remove binding by index | Required | — | **NO** |

---

### 38. A2A (Agent-to-Agent Protocol) — Inbound

These expose this node as an A2A server for external agents to call. All unused by UI.

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/.well-known/agent.json` | A2A agent card (capability advertisement) | Public | — | **NO** |
| GET | `/a2a/agents` | List local agents (A2A protocol) | Public | — | **NO** |
| POST | `/a2a/tasks/send` | Accept task from external A2A agent | Public | — | **NO** |
| GET | `/a2a/tasks/{id}` | Get task status (A2A protocol) | Public | — | **NO** |
| POST | `/a2a/tasks/{id}/cancel` | Cancel task (A2A protocol) | Public | — | **NO** |

---

### 39. A2A Management — Outbound

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/a2a/agents` | List discovered external A2A agents | Public | — | YES (chat.js `/a2a` command) |
| POST | `/api/a2a/discover` | Discover A2A agent at URL | Required | — | **NO** |
| POST | `/api/a2a/send` | Send task to external A2A agent | Required | — | **NO** |
| GET | `/api/a2a/tasks/{id}/status` | Check external task status | Required | — | **NO** |

**NOT used by UI:** `POST /api/a2a/discover`, `POST /api/a2a/send`, `GET /api/a2a/tasks/{id}/status`. The `/api/a2a/agents` endpoint is only used within a chat slash-command, not in a dedicated UI panel.

---

### 40. Webhook Triggers (External Event Injection)

All unused by UI.

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| POST | `/hooks/wake` | Wake agent from external webhook | Required | — | **NO** |
| POST | `/hooks/agent` | Send message to agent via webhook | Required | — | **NO** |

---

### 41. Chat Commands

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/commands` | List available chat slash commands | Required | — | YES (chat.js) |

---

### 42. Runs — v1 API (`/api/v1/runs`)

All unused by UI. These are workflow execution run records with SSE streaming.

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/v1/runs` | List workflow runs | Required | — | **NO** |
| GET | `/api/v1/runs/{id}` | Get run details | Required | — | **NO** |
| GET | `/api/v1/runs/{id}/events` | SSE stream of run events | Required | **SSE** | **NO** |
| GET | `/api/v1/runs/{id}/checkpoints` | Get run checkpoints | Required | — | **NO** |
| GET | `/api/v1/runs/{id}/dispatches` | Get run dispatches | Required | — | **NO** |
| GET | `/api/v1/runs/{id}/hitl-requests` | Get HITL requests for run | Required | — | **NO** |
| GET | `/api/v1/runs/{id}/signals` | Get run signals | Required | — | **NO** |
| POST | `/api/v1/runs/{id}/signals` | Post signal to run | Required | — | **NO** |
| POST | `/api/v1/runs/{id}/pause` | Pause a run | Required | — | **NO** |
| POST | `/api/v1/runs/{id}/resume` | Resume a paused run | Required | — | **NO** |
| POST | `/api/v1/runs/{id}/cancel` | Cancel a run | Required | — | **NO** |

---

### 43. Dispatches — v1 API (`/api/v1/dispatches`)

All unused by UI.

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/v1/dispatches` | List dispatches | Required | — | **NO** |
| GET | `/api/v1/dispatches/{id}` | Get dispatch details | Required | — | **NO** |
| GET | `/api/v1/dispatches/{id}/children` | Get child dispatches | Required | — | **NO** |
| POST | `/api/v1/dispatches/{id}/retry` | Retry dispatch | Required | — | **NO** |
| POST | `/api/v1/dispatches/{id}/cancel` | Cancel dispatch | Required | — | **NO** |
| GET | `/api/v1/dispatches/{id}/events` | SSE stream of dispatch events | Required | **SSE** | **NO** |

---

### 44. HITL (Human-in-the-Loop) — v1 API (`/api/v1/hitl-requests`)

All unused by UI.

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/v1/hitl-requests` | List HITL requests | Required | — | **NO** |
| GET | `/api/v1/hitl-requests/stream` | SSE stream of HITL requests | Required | **SSE** | **NO** |
| GET | `/api/v1/hitl-requests/{id}` | Get HITL request details | Required | — | **NO** |
| POST | `/api/v1/hitl-requests/{id}/answer` | Answer a HITL request | Required | — | **NO** |
| POST | `/api/v1/hitl-requests/{id}/cancel` | Cancel a HITL request | Required | — | **NO** |

---

### 45. Artifacts — v1 API (`/api/v1/artifacts`)

All unused by UI.

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/v1/artifacts` | List artifacts | Required | — | **NO** |
| GET | `/api/v1/artifacts/{id}` | Get artifact details | Required | — | **NO** |
| GET | `/api/v1/artifacts/{id}/versions` | List artifact versions | Required | — | **NO** |

---

### 46. Docs — v1 API (`/api/v1/docs`)

All unused by UI.

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/v1/docs` | List docs | Required | — | **NO** |
| GET | `/api/v1/docs/{id}` | Get doc details | Required | — | **NO** |
| GET | `/api/v1/docs/{id}/versions` | List doc versions | Required | — | **NO** |

---

### 47. Tasks — v1 API (`/api/v1/tasks`)

All unused by UI.

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/v1/tasks` | List tasks | Required | — | **NO** |
| POST | `/api/v1/tasks` | Create task | Required | — | **NO** |
| GET | `/api/v1/tasks/{id}` | Get task | Required | — | **NO** |
| PUT | `/api/v1/tasks/{id}` | Update task | Required | — | **NO** |
| DELETE | `/api/v1/tasks/{id}` | Delete task | Required | — | **NO** |
| GET | `/api/v1/tasks/{id}/subtasks` | List subtasks | Required | — | **NO** |
| POST | `/api/v1/tasks/{id}/subtasks` | Create subtask | Required | — | **NO** |
| POST | `/api/v1/tasks/{id}/replan` | Replan task | Required | — | **NO** |
| GET | `/api/v1/tasks/{id}/artifacts` | Get task artifacts | Required | — | **NO** |
| GET | `/api/v1/tasks/{id}/docs` | Get task docs | Required | — | **NO** |
| GET | `/api/v1/tasks/{id}/files` | Get task files | Required | — | **NO** |

---

### 48. Subtasks — v1 API (`/api/v1/subtasks`)

All unused by UI.

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/v1/subtasks` | List all subtasks | Required | — | **NO** |
| GET | `/api/v1/subtasks/{id}` | Get subtask | Required | — | **NO** |
| PUT | `/api/v1/subtasks/{id}` | Update subtask | Required | — | **NO** |
| DELETE | `/api/v1/subtasks/{id}` | Delete subtask | Required | — | **NO** |

---

### 49. Looper Runs — v1 API (`/api/v1/looper-runs`)

All unused by UI.

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| GET | `/api/v1/looper-runs` | List looper runs | Required | — | **NO** |
| POST | `/api/v1/looper-runs` | Create looper run | Required | — | **NO** |
| GET | `/api/v1/looper-runs/{id}` | Get looper run | Required | — | **NO** |
| GET | `/api/v1/looper-runs/{id}/subtasks` | List looper subtasks | Required | — | **NO** |
| POST | `/api/v1/looper-runs/{id}/pause` | Pause looper run | Required | — | **NO** |
| POST | `/api/v1/looper-runs/{id}/resume` | Resume looper run | Required | — | **NO** |
| POST | `/api/v1/looper-runs/{id}/cancel` | Cancel looper run | Required | — | **NO** |
| GET | `/api/v1/looper-runs/{id}/events` | SSE stream of looper events | Required | **SSE** | **NO** |

---

### 50. OpenAI-Compatible API

All unused by UI.

| Method | Path | Description | Auth | WS/SSE | UI Uses? |
|--------|------|-------------|------|--------|----------|
| POST | `/v1/chat/completions` | OpenAI-compatible chat completions | Required | — | **NO** |
| GET | `/v1/models` | OpenAI-compatible model list | Required | — | **NO** |

---

## Key Findings & Gaps

### Critical Bug: Unregistered Trigger Routes

The `scheduler.js` page calls three routes that are NOT registered in `server.rs`:
- `GET /api/triggers` — handler exists (`list_triggers`) but not routed
- `DELETE /api/triggers/{id}` — handler exists (`delete_trigger`) but not routed
- `PUT /api/triggers/{id}` — handler exists (`update_trigger`) but not routed

The server only has the v1 triggers registered at `/api/v1/triggers`. The scheduler UI trigger tab will return 404 for all operations.

### The UI Operates Entirely on Legacy APIs

The entire `/api/v1/` namespace — representing the richer, more capable second-generation API for workflows, triggers, schedules, runs, dispatches, HITL, packs, skills, tasks, artifacts, docs, and looper runs — is **completely unused by the UI**. The UI still uses the older `/api/agents`, `/api/workflows`, and `/api/cron` endpoints.

### Significant Feature Gaps

The following major backend capabilities have **no UI representation at all**:

1. **Workflow Runs (v1)** — `/api/v1/runs` with SSE streaming, pause/resume/cancel, signals, checkpoints
2. **HITL (Human-in-the-Loop)** — `/api/v1/hitl-requests` with SSE stream for real-time review queue
3. **Dispatches** — `/api/v1/dispatches` sub-execution tracking with SSE
4. **Tasks & Subtasks** — `/api/v1/tasks` full task management system
5. **Looper Runs** — `/api/v1/looper-runs` with streaming and lifecycle control
6. **Packs** — `/api/v1/packs` complete pack install/upgrade/uninstall system
7. **Artifacts & Docs** — `/api/v1/artifacts` and `/api/v1/docs` output management
8. **Schedules v1** — `/api/v1/schedules` (UI uses legacy `/api/cron/jobs` instead)
9. **Triggers v1** — `/api/v1/triggers` (UI has broken calls to legacy `/api/triggers`)
10. **Integrations** — `/api/integrations` full integration system has no UI panel
11. **Device Pairing** — `/api/pairing/*` completely hidden
12. **Bindings** — `/api/bindings` channel-agent bindings not exposed in UI
13. **Budget Management** — Only GET read, no UI for setting limits or viewing per-agent breakdown
14. **A2A Discovery** — Discover/send/status calls not exposed in a dedicated UI section
15. **Agent Skills/MCP assignment** — `GET/PUT /api/agents/{id}/skills` and `GET/PUT /api/agents/{id}/mcp_servers` unused
16. **Comms SSE stream** — `GET /api/comms/events/stream` uses polling instead of SSE

### SSE Endpoints Available but Unused by UI

| Endpoint | Description |
|----------|-------------|
| `POST /api/agents/{id}/message/stream` | Per-message SSE token stream (uses WS instead) |
| `GET /api/logs/stream` | Live log stream (IS used in logs.js) |
| `GET /api/v1/runs/{id}/events` | Run event stream |
| `GET /api/v1/dispatches/{id}/events` | Dispatch event stream |
| `GET /api/v1/hitl-requests/stream` | HITL request queue stream |
| `GET /api/v1/looper-runs/{id}/events` | Looper run event stream |
| `GET /api/comms/events/stream` | Agent comms event stream (polled instead) |
