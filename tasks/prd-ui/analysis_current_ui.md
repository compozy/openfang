# OpenFang Web Dashboard — Current UI Analysis

> Research-only analysis of the existing Alpine.js SPA dashboard.
> Generated from reading all source files in `crates/openfang-api/static/`.

---

## Table of Contents

1. [Overall UI Architecture](#1-overall-ui-architecture)
2. [API Client Capabilities](#2-api-client-capabilities)
3. [CSS & Theming Approach](#3-css--theming-approach)
4. [Page-by-Page Analysis](#4-page-by-page-analysis)
   - [Overview](#41-overview)
   - [Agents (+ inline Chat)](#42-agents--inline-chat)
   - [Sessions & Memory](#43-sessions--memory)
   - [Approvals](#44-approvals)
   - [Comms (Agent Topology)](#45-comms-agent-topology)
   - [Workflows](#46-workflows)
   - [Visual Workflow Builder](#47-visual-workflow-builder)
   - [Scheduler](#48-scheduler)
   - [Channels](#49-channels)
   - [Skills](#410-skills)
   - [Hands](#411-hands)
   - [Logs](#412-logs)
   - [Runtime](#413-runtime)
   - [Settings](#414-settings)
   - [Analytics / Usage](#415-analytics--usage)
   - [Setup Wizard](#416-setup-wizard)
5. [Shared UI Patterns & Components](#5-shared-ui-patterns--components)
6. [Known Bugs & Limitations](#6-known-bugs--limitations)
7. [Complete API Endpoint Inventory](#7-complete-api-endpoint-inventory)

---

## 1. Overall UI Architecture

### SPA Framework

The dashboard is a single-page application (SPA) built with **Alpine.js v3** and **vanilla JavaScript**. There is no build step — all JS files are served directly by the Rust backend. External CDN dependencies:

- Alpine.js v3 (reactivity, stores, component data)
- marked.js (Markdown rendering)
- highlight.js (code block syntax highlighting)
- Chart.js (used only in Hands trader dashboard)
- MathJax or similar (LaTeX rendering via `renderLatex()`)

### Routing

Navigation is **hash-based**: `window.location.hash` is parsed on page load and on `hashchange` events. The main `app()` function maintains a `page` property which all `x-if`/`x-show` directives key off.

**Valid page names:**
```
overview, agents, sessions, approvals, comms, workflows, scheduler,
channels, skills, hands, analytics, logs, runtime, settings, wizard
```

**Redirect aliases** (old or alternate hash → canonical page):
```
chat        → agents
usage       → analytics
audit       → logs
memory      → sessions
extensions  → skills
mcps        → skills
marketplace → skills
```

### Global Alpine Store (`Alpine.store('app')`)

Defined in `app.js`. All pages access this store via `$store.app`.

**State fields:**
- `agents[]` — full agent list, refreshed every 5s and on demand
- `connected: true` — whether the daemon is reachable
- `booting: false` — initial boot flag
- `wsConnected: false` — WebSocket connection status
- `connectionState: 'connected'` — tracks `connected`/`reconnecting`/`disconnected`
- `pendingApprovalCount: 0` — approval badge counter
- `focusMode: false` — hides sidebar chrome
- `showOnboarding: true` — whether to show onboarding banner
- `showAuthPrompt: false` — triggers auth overlay
- `authMode: 'none'` — `'none'` / `'session'` / `'apikey'`
- `sessionUser: null` — logged-in username (session mode)

**Store methods / API calls:**
| Method | Endpoint | Purpose |
|--------|----------|---------|
| `refreshAgents()` | `GET /api/agents` | Reload agent list |
| `refreshApprovals()` | `GET /api/approvals` | Reload + badge count + toast on new |
| `checkStatus()` | `GET /api/status` | Daemon health/connection |
| `checkOnboarding()` | `GET /api/config` | Whether to show onboarding banner |
| `checkAuth()` | `GET /api/auth/check` then `GET /api/tools` | Auth mode detection |
| `sessionLogin()` | `POST /api/auth/login` | Username/password login |
| `sessionLogout()` | `POST /api/auth/logout` | Session logout |

### Global Poll (every 5s)

The main `app()` component runs `pollStatus()` + `refreshApprovals()` on a 5-second interval using `setInterval`. This keeps the connection status indicator and approval badge up to date without user interaction.

### Keyboard Shortcuts

| Keys | Action |
|------|--------|
| `Ctrl+K` | Navigate to Agents page |
| `Ctrl+N` | Open new agent spawn modal |
| `Ctrl+Shift+F` | Toggle focus mode |
| `Escape` | Close mobile sidebar menu |

### Component Registration Pattern

Most pages register a plain function returning a data object:
```js
function agentsPage() { return { ... }; }
```

These are referenced in HTML as `x-data="agentsPage()"`.

**Exception:** `runtimePage` is registered via `Alpine.data('runtimePage', () => ({ ... }))` — the only page using the `Alpine.data()` registration pattern.

### Sidebar Navigation

Fixed left sidebar with collapsible sections:

- **Chat** → `agents`
- **Monitor**: Overview, Analytics, Logs
- **Agents**: Sessions, Approvals, Comms
- **Automation**: Workflows, Scheduler
- **Extensions**: Channels, Skills, Hands
- **System**: Runtime, Settings

The approval badge appears next to "Approvals" when `$store.app.pendingApprovalCount > 0`.

---

## 2. API Client Capabilities

### `OpenFangAPI` Singleton (`api.js`)

IIFE-based singleton exposing:

```js
OpenFangAPI.get(path)
OpenFangAPI.post(path, body)
OpenFangAPI.put(path, body)
OpenFangAPI.patch(path, body)
OpenFangAPI.del(path)           // also exposed as .delete
OpenFangAPI.upload(agentId, file)
OpenFangAPI.setAuthToken(token)
OpenFangAPI.getToken()
OpenFangAPI.wsConnect(agentId, callbacks)
OpenFangAPI.wsDisconnect()
OpenFangAPI.wsSend(data)
OpenFangAPI.isWsConnected()
OpenFangAPI.getConnectionState()
OpenFangAPI.onConnectionChange(fn)
```

**Notable properties NOT exposed** (relevant for bug below):
- `OpenFangAPI.baseUrl` — does NOT exist
- `OpenFangAPI.apiKey` — does NOT exist

### Auth Injection

All requests include `Authorization: Bearer {token}` if a token is set. On `401` responses, the token is cleared from `localStorage`, `_authToken` is reset, and `store.showAuthPrompt = true` triggers the auth overlay.

### Connection State Tracking

Three states: `connected`, `reconnecting`, `disconnected`. Transitions are broadcast to registered listeners via `onConnectionChange(fn)`. The connection indicator in the sidebar reflects this state.

### WebSocket Manager

- One active socket at a time per `wsConnect()` call
- URL pattern: `ws(s)://{host}/api/agents/{id}/ws?token={token}`
- `MAX_RECONNECT = 5` attempts with exponential backoff: `min(1000 * 2^n, 10000)` ms
- Guard pattern: socket reference checked on `onclose`/`onerror` to prevent superseded sockets from corrupting state
- Reconnect toasts: "Connection lost, reconnecting..." on first drop; "Connection lost — switched to HTTP mode" after max attempts; "Reconnected" on recovery

### WebSocket Message Types (chat)

`text_delta`, `tool_start`, `tool_end`, `tool_result`, `response`, `typing`, `phase`, `canvas`, `command_result`, `connected`, `thinking`, `silent_complete`, `error`, `agents_updated`, `pong`

### File Upload

`upload(agentId, file)`: multipart FormData POST to `/api/agents/{id}/upload`. Includes `Authorization` header. Returns parsed JSON.

### Toast System (`OpenFangToast`)

```js
OpenFangToast.success(msg, duration?)
OpenFangToast.error(msg, duration?)     // default 6000ms
OpenFangToast.warn(msg, duration?)      // default 5000ms
OpenFangToast.info(msg, duration?)      // default 4000ms
OpenFangToast.confirm(title, msg, onConfirm)  // styled modal
```

Container auto-created as `#toast-container`. Toasts auto-dismiss with CSS transition. The `confirm()` method creates a styled modal overlay (not native `window.confirm()`).

---

## 3. CSS & Theming Approach

### Theme System (`theme.css`)

**Design tokens** via CSS custom properties on `:root` (light) and `[data-theme="dark"]`:

- **Accent color**: `#FF5C00` (orange) — used for primary buttons, focus rings, highlights
- **Background**: layered (`--bg-base`, `--bg-surface`, `--bg-elevated`, `--bg-overlay`)
- **Text**: 4 levels (`--text-primary`, `--text-secondary`, `--text-muted`, `--text-dim`)
- **Border**: `--border-subtle`, `--border-default`, `--border-strong`
- **Semantic colors**: `--success`, `--warn`, `--error`, `--info` with `-muted` and `-text` variants
- **Shadow**: 6-level system (`--shadow-xs` through `--shadow-2xl`)
- **Radius**: 5 sizes (`--radius-sm` through `--radius-2xl`)
- **Easing**: spring curves (`--ease-spring`, `--ease-bounce`)

**Fonts**: Inter (body, UI), Geist Mono (code, IDs, monospace fields) — loaded via Google Fonts.

**Theme switching**: `data-theme` attribute on `<body>`, toggled from the sidebar switcher (light/system/dark). `system` mode reads `prefers-color-scheme` media query.

**Animations defined:**
- `fadeIn`, `slideUp`, `slideDown`, `scaleIn` — page transitions
- `shimmer` — skeleton loading state
- `pulse-ring` — status indicators
- `spin` — loading spinners
- `cardEntry` — staggered card entrance

**Stagger helpers**: `.stagger-1` through `.stagger-6` apply `animation-delay` for sequential card reveals.

**Skeleton states**: `.skeleton` class with shimmer animation for loading placeholders.

**Accessibility**: `prefers-reduced-motion` media query disables animations. Print styles strip sidebar/chrome.

### Component Library (`components.css`)

#### Buttons
`.btn` base + modifiers: `.btn-primary` (orange), `.btn-ghost` (transparent), `.btn-danger` (red), `.btn-success` (green), `.btn-sm` (smaller), `.btn-block` (full-width).

#### Cards
`.card` with hover lift (`translateY(-1px)`), focus ring, `.card-header`, `.card-grid`, `.card-flex`. `.card-glow` adds radial gradient halo on hover.

#### Badges
`.badge` + 10+ semantic variants: `running` (green), `suspended` (yellow), `terminated`/`crashed` (red), `created` (blue), `connected` (green), `success`, `warn`, `error`, `muted`, `info`, `dim`.

#### Tables
`.table-wrap` (overflow scroll) + `table` styles: `th` (muted text, small caps), `td`, row hover highlight.

#### Forms
`.form-group`, `.form-label`, `.form-input`, `.form-select`, `.form-textarea` — all use `--bg-surface`, `--border-default`, focus ring with `--accent`.

### Additional Stylesheets

- `layout.css` — sidebar, main content, responsive layout, modal overlays, split pane
- `components.css` — all UI components (buttons, cards, forms, tables, badges, toasts, modals)
- `theme.css` — design tokens, animations, skeleton states

---

## 4. Page-by-Page Analysis

### 4.1 Overview

**Nav path:** `#overview`
**File:** `pages/overview.js` — `overviewPage()`

**Data displayed:**
- Agent count, active agents, total messages, total cost
- Setup checklist (5 items): provider configured, agent created, first message sent, channel connected, skill installed
- Provider health badges (configured/cooldown/open circuit breaker)
- Recent audit activity feed (last 8 events) with relative timestamps
- Security systems panel (9 hardcoded feature badges)
- Onboarding banner (shown until all checklist items done)

**API endpoints called:**
| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/api/health` | GET | Daemon health |
| `/api/status` | GET | Version, uptime, agent counts |
| `/api/usage` | GET | Total tokens/cost |
| `/api/audit/recent?n=8` | GET | Recent activity feed |
| `/api/channels` | GET | Channel health badges |
| `/api/providers` | GET | Provider health indicators |
| `/api/mcp/servers` | GET | MCP server count |
| `/api/skills` | GET | Skill count |

**Functionality:**
- 30-second auto-refresh via `setInterval`
- Setup checklist tracks completion in `localStorage`
- Provider badges show circuit breaker/cooldown states
- Audit feed: action icons, friendly labels, relative timestamps (`timeAgo()`)

**Alpine.js data:**
```js
healthData, statusData, usageData, auditItems, channels, providers, mcpServers, skills
checklist: { provider, agent, message, channel, skill }
loading, error
```

**Limitations:**
- Security systems panel is entirely hardcoded — not fetched from API
- Checklist completion relies on `localStorage` flags, not server-side state
- No refresh button; relies solely on 30s auto-poll

---

### 4.2 Agents (+ inline Chat)

**Nav path:** `#agents`
**Files:** `pages/agents.js` — `agentsPage()` + `pages/chat.js` — `chatPage()`

#### Agents Sub-page

**Data displayed:**
- Agent cards (name, emoji, model, status badge, last active)
- Templates tab (built-in + server-fetched)
- Active inline chat panel (when agent selected)
- Spawn wizard (5-step modal)

**API endpoints — agents:**
| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/api/agents` | GET | List all agents |
| `/api/agents` | POST | Create agent (with `manifest_toml`) |
| `/api/agents/{id}` | DELETE | Delete agent |
| `/api/agents/{id}/config` | PATCH | Update agent config |
| `/api/agents/{id}/mode` | PUT | Set agent mode |
| `/api/agents/{id}/model` | PUT | Switch model |
| `/api/agents/{id}/clone` | POST | Clone agent |
| `/api/agents/{id}/history` | DELETE | Clear chat history |
| `/api/agents/{id}/files` | GET | List agent files |
| `/api/agents/{id}/files/{name}` | GET | Read agent file |
| `/api/agents/{id}/files/{name}` | PUT | Write agent file |
| `/api/agents/{id}/tools` | GET | Get tool filter list |
| `/api/agents/{id}/tools` | PUT | Update tool filter |
| `/api/templates` | GET | Server-side templates |
| `/api/providers` | GET | Available providers |
| `/api/profiles` | GET | Tool profiles (for preview) |

**Spawn wizard steps:**
1. **Identity** — name, emoji (picker), color, archetype (assistant/coder/analyst/creative/support/researcher)
2. **Personality** — preset styles (professional/friendly/technical/creative/concise/mentor)
3. **Provider & Model** — provider dropdown → model dropdown (filtered)
4. **Profile** — tool access profile with preview
5. **Review** — TOML manifest preview before creation

**TOML generation helpers:** `tomlMultilineEscape()`, `tomlBasicEscape()` — properly escape multiline strings and special characters for TOML manifest format.

**Fallback chain:** Configured via `PATCH /api/agents/{id}/config` with `fallback_models[]` array.

**Templates:** 6 built-in templates hardcoded in JS + additional from `GET /api/templates`. Filtered by category.

#### Chat Sub-page (inline panel)

**Real-time:** WebSocket at `/api/agents/{id}/ws` for streaming.

**API endpoints — chat:**
| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/api/agents/{id}/ws` | WS | Streaming chat |
| `/api/agents/{id}/stop` | POST | Stop generation |
| `/api/agents/{id}/sessions` | GET | List sessions |
| `/api/agents/{id}/sessions` | POST | Create session |
| `/api/agents/{id}/sessions/{sid}/switch` | POST | Switch active session |
| `/api/agents/{id}/session/reset` | POST | Clear current session |
| `/api/agents/{id}/session/compact` | POST | Compact/summarize context |
| `/api/agents/{id}/upload` | POST | File upload (multipart) |
| `/api/models` | GET | Model list for switcher |
| `/api/commands` | GET | Dynamic slash commands |

**Features:**
- Markdown rendering with syntax highlighting and copy buttons
- LaTeX rendering (`renderLatex()` called post-render)
- Tool call display with icons, collapsible results
- Canvas rendering: agent-generated HTML in sandboxed `iframe`
- Voice recording via `MediaRecorder` API, upload + transcription
- File attachments: drag-drop, up to 10MB, image preview
- Slash commands: 14 built-in + dynamic from API, `/help` shows list
- Context pressure indicator: `critical`/`high`/`medium`/`low` color-coded
- Message queue for messages sent while response is streaming
- `sanitizeToolText()`: strips leaked function-call JSON (Llama/Groq models)
- Model switcher: grouped by provider, filterable

**Limitations:**
- Inline chat only; no dedicated full-screen chat page
- No persistent file attachment list per session
- Voice transcription depends on server-side handling of upload response

---

### 4.3 Sessions & Memory

**Nav path:** `#sessions`
**File:** `pages/sessions.js` — `sessionsPage()`

**Data displayed:**
- Sessions tab: all chat sessions (agent name, session ID, message count, last active)
- Memory tab: per-agent key-value memory store (keys, values, inline edit)

**API endpoints:**
| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/api/sessions` | GET | List all sessions |
| `/api/sessions/{id}` | DELETE | Delete session |
| `/api/memory/agents/{id}/kv` | GET | Agent KV memory |
| `/api/memory/agents/{id}/kv/{key}` | PUT | Update memory key |
| `/api/memory/agents/{id}/kv/{key}` | DELETE | Delete memory key |

**Functionality:**
- Client-side search filter on sessions (name/ID)
- "Open in Chat" button sets `pendingAgent` on store and navigates to `#agents`
- Memory KV: inline edit with JSON parse validation, add/delete keys
- Agent selector for memory tab

**Limitations:**
- No session export or bulk delete
- Memory search/filter not implemented
- No pagination for large session counts

---

### 4.4 Approvals

**Nav path:** `#approvals`
**File:** `pages/approvals.js` — `approvalsPage()`

**Data displayed:**
- List of pending/resolved Human-in-the-Loop (HITL) approval requests
- Tool name, agent, requested input, status, timestamp

**API endpoints:**
| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/api/approvals` | GET | List approvals |
| `/api/approvals/{id}/approve` | POST | Approve request |
| `/api/approvals/{id}/reject` | POST | Reject with optional reason |

**Functionality:**
- 5-second polling for new requests (same as global store poll)
- Filter by status: all/pending/approved/rejected
- `pendingCount` computed property drives sidebar badge
- Approve/reject with inline confirmation

**Limitations:**
- No WebSocket/SSE — relies on polling only
- No bulk approve/reject
- No approval history export
- Rejection reason is optional with no structured format

---

### 4.5 Comms (Agent Topology)

**Nav path:** `#comms`
**File:** `pages/comms.js` — `commsPage()`

**Data displayed:**
- Agent topology tree (parent-child hierarchy, peer relationships)
- Live event feed with SSE streaming

**API endpoints:**
| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/api/comms/topology` | GET | Agent graph (nodes + edges) |
| `/api/comms/events?limit=200` | GET | Recent events |
| `/api/comms/events/stream` | GET (SSE) | Live event stream |
| `/api/comms/send` | POST | Send inter-agent message |
| `/api/comms/task` | POST | Dispatch task to agent |

**Functionality:**
- Tree rendering: `rootNodes()`, `childrenOf(id)`, `peersOf(id)` using edge `kind` (`parent_child`/`peer`)
- SSE event feed with event type filtering
- Send message modal, task dispatch modal
- Event auto-scroll to latest

**Known Bug:**
`commsPage.startSSE()` references `OpenFangAPI.baseUrl` and `OpenFangAPI.apiKey` — neither property exists on the `OpenFangAPI` object. The SSE connection will silently fail (URL will be malformed). This is a latent bug.

**Limitations:**
- Topology is static snapshot (no live graph updates)
- No visual graph rendering (tree-list only, no SVG/canvas)
- SSE setup broken due to non-existent API properties

---

### 4.6 Workflows

**Nav path:** `#workflows`
**File:** `pages/workflows.js` — `workflowsPage()`

**Data displayed:**
- Workflow list (name, steps count, last run, status)
- Run history for selected workflow
- Run result JSON output

**API endpoints:**
| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/api/workflows` | GET | List workflows |
| `/api/workflows` | POST | Create workflow |
| `/api/workflows/{id}` | PUT | Update workflow |
| `/api/workflows/{id}` | DELETE | Delete workflow |
| `/api/workflows/{id}` | GET | Get workflow detail |
| `/api/workflows/{id}/run` | POST | Trigger run |
| `/api/workflows/{id}/runs` | GET | Run history |

**Functionality:**
- CRUD for workflow definitions
- Trigger manual run, view result JSON in textarea
- Run history list

**Limitations:**
- Delete uses native `window.confirm()` instead of `OpenFangToast.confirm()` — inconsistent UX
- No workflow step editor in this page (use Visual Builder instead)
- Run result shown raw as JSON in `<textarea>` — no formatted display
- No run status polling (run result shown once, not updated for async runs)

---

### 4.7 Visual Workflow Builder

**Nav path:** `#workflows` → "Open Builder" button (not a separate hash route)
**File:** `pages/workflow-builder.js` — `workflowBuilder()`

**Data displayed:**
- SVG canvas with draggable nodes and bezier connections
- Node palette (agent, parallel, condition, loop, collect, start, end)
- Node property panel
- Generated TOML preview

**Functionality:**
- Drag nodes from palette onto canvas
- Drag existing nodes to reposition
- Click to select, double-click to edit (with manual timestamp debounce)
- Draw connections between node ports
- Canvas pan (drag empty area) and zoom (0.3×–2× via scroll)
- TOML generation from node graph structure
- Save workflow to `/api/workflows`

**Architecture Note:**
Uses manual `document.createElementNS` DOM rendering rather than Alpine's `x-for` — this is intentional because Alpine.js `x-for` cannot create SVG elements in the correct namespace (`http://www.w3.org/2000/svg`). Renders are scheduled via `requestAnimationFrame` for batching.

Double-click detection is also manual (comparing timestamps) because native `dblclick` events fire after the node re-renders and lose their target.

**API endpoints:**
| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/api/workflows` | POST | Save new workflow |
| `/api/workflows/{id}` | PUT | Update existing |

**Limitations:**
- No undo/redo
- No copy/paste nodes
- No alignment or snap-to-grid
- No multi-select
- TOML generation covers basic step types but complex conditions may be incomplete
- No live validation of the workflow graph (e.g., disconnected nodes)

---

### 4.8 Scheduler

**Nav path:** `#scheduler`
**File:** `pages/scheduler.js` — `schedulerPage()`

**Data displayed:**
- Scheduled Jobs tab: cron jobs list (name, schedule, agent, status, last/next run)
- Event Triggers tab: trigger definitions (pattern, enabled, fire count)
- History tab: synthetic history built from `last_run` + `fire_count` fields

**API endpoints:**
| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/api/cron/jobs` | GET | List scheduled jobs |
| `/api/cron/jobs` | POST | Create cron job |
| `/api/cron/jobs/{id}/enable` | PUT | Enable/disable job |
| `/api/cron/jobs/{id}` | DELETE | Delete job |
| `/api/cron/jobs/{id}/run` | POST | Trigger immediate run |
| `/api/triggers` | GET | List event triggers |
| `/api/triggers/{id}` | PUT | Update trigger (enable/disable) |
| `/api/triggers/{id}` | DELETE | Delete trigger |

**Functionality:**
- Create job with form: name, cron expression, agent, message
- 11 cron presets (quick-select)
- Human-readable cron description (`describeCron()`) with 20+ known patterns + dynamic parsing
- Enable/disable toggle for jobs and triggers
- "Run Now" triggers immediate job execution
- Trigger type labels: Lifecycle, Agent Spawned/Terminated, System, Memory Update, etc.

**Limitations:**
- History tab is synthetic (not a real API) — only shows jobs that ran and triggers that fired
- No trigger creation UI (triggers must be created programmatically)
- No cron next-run preview
- Run Now doesn't update `last_run` in UI (correctly noted in comment: "runs asynchronously in the background")

---

### 4.9 Channels

**Nav path:** `#channels`
**File:** `pages/channels.js` — `channelsPage()`

**Data displayed:**
- Channel cards (name, type, status, description, difficulty)
- 3-step setup flow per channel: Configure → Verify → Ready
- WhatsApp QR code flow

**API endpoints:**
| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/api/channels` | GET | List configured channels |
| `/api/channels/{name}/configure` | POST | Configure channel |
| `/api/channels/{name}/test` | POST | Test connection |
| `/api/channels/{name}/configure` | DELETE | Remove configuration |
| `/api/channels/whatsapp/qr/start` | POST | Start WhatsApp QR session |
| `/api/channels/whatsapp/qr/status?session_id=` | GET | Poll QR scan status (3s interval) |

**Functionality:**
- Category filter: messaging, social, enterprise, developer, notifications
- Search by name
- Difficulty badge: Easy/Medium/Advanced
- Advanced field toggle (hides rarely-needed fields by default)
- WhatsApp: QR code display with 3s polling until scan confirmed
- Status polling every 15s for all channels
- Step wizard with back navigation

**Limitations:**
- No channel-specific log view
- No webhook URL display (relevant for inbound channels)
- No multi-agent routing configuration per channel

---

### 4.10 Skills

**Nav path:** `#skills`
**File:** `pages/skills.js` — `skillsPage()`

**Data displayed:**
- Installed tab: locally installed skills
- ClawHub tab: external skill registry browse/search
- MCP tab: Model Context Protocol server list

**API endpoints:**
| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/api/skills` | GET | List installed skills |
| `/api/skills/uninstall` | POST | Uninstall skill |
| `/api/skills/create` | POST | Create custom skill |
| `/api/clawhub/search?q=` | GET | Search ClawHub registry |
| `/api/clawhub/browse?sort=` | GET | Browse ClawHub (cached 60s) |
| `/api/clawhub/skill/{slug}` | GET | Skill detail |
| `/api/clawhub/skill/{slug}/code` | GET | Skill source code |
| `/api/clawhub/install` | POST | Install from ClawHub |
| `/api/mcp/servers` | GET | List MCP servers |

**Functionality:**
- ClawHub browse with 60-second client-side cache
- Debounced search (350ms delay)
- 18 category definitions for filtering
- Runtime badges: PY / JS / WASM / PROMPT
- Skill detail modal with code preview
- 4 quick-start prompt-only skills (hardcoded in JS)
- Custom skill creation form
- MCP server status indicators

**Limitations:**
- No MCP server CRUD (add/remove servers)
- No skill update mechanism (install = fresh install only)
- ClawHub search is server-proxied but no offline fallback

---

### 4.11 Hands

**Nav path:** `#hands`
**File:** `pages/hands.js` — `handsPage()`

**Data displayed:**
- Available Hand packages (name, description, category, deps status)
- Active instances (running Hand instances)
- Trader dashboard (for trader Hand: equity curve, P&L bars, signal radar)
- Browser viewer (for browser Hand: screenshot polling)

**API endpoints:**
| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/api/hands` | GET | List available Hands |
| `/api/hands/active` | GET | List active instances |
| `/api/hands/{id}` | GET | Hand detail |
| `/api/hands/{id}/activate` | POST | Start Hand instance |
| `/api/hands/{id}/install-deps` | POST | Install dependencies |
| `/api/hands/{id}/check-deps` | POST | Check dep status |
| `/api/hands/instances/{id}` | DELETE | Stop/remove instance |
| `/api/hands/instances/{id}/pause` | POST | Pause instance |
| `/api/hands/instances/{id}/resume` | POST | Resume instance |
| `/api/hands/instances/{id}/stats` | GET | Instance stats |
| `/api/hands/instances/{id}/browser` | GET | Browser screenshot |

**Functionality:**
- 3-step activation wizard: deps check → settings form → confirm
- Auto-install deps with progress tracking
- Platform detection (macOS/Windows/Linux) for install commands display
- Browser Hand: polls screenshot every 3s, displays as `<img src="data:..."`
- Trader Hand dashboard: Chart.js charts (equity curve line, daily P&L bar, signal radar)
  - Data sourced from agent memory KV (`trader_hand_*` keys)

**Limitations:**
- Trader dashboard charts only appear for the specific "trader" Hand
- Browser viewer is polling-based (3s interval), not live stream
- No dep version pinning in the UI
- Settings form fields are Hand-specific, defined in Hand manifest

---

### 4.12 Logs

**Nav path:** `#logs`
**File:** `pages/logs.js` — `logsPage()`

**Data displayed:**
- Live Logs tab: real-time log stream (level, timestamp, message, component)
- Audit tab: audit log chain with verify capability

**API endpoints:**
| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/api/logs/stream` | GET (SSE) | Live log stream |
| `/api/audit/recent?n=200` | GET | Fallback poll (2s) |
| `/api/audit/verify` | GET | Verify audit chain integrity |

**Functionality:**
- SSE-based live streaming, falls back to 2s polling if SSE unavailable
- Pause/resume stream toggle
- Level filter: ALL / DEBUG / INFO / WARN / ERROR
- Text filter (client-side substring match)
- Auto-scroll to latest (disables when user scrolls up)
- Export: downloads full log buffer as `.txt` blob
- Audit chain verification with pass/fail display
- Cap at 500 log entries (circular buffer behavior)

**Limitations:**
- No log persistence — buffer lost on page navigation
- No log search by timestamp range
- SSE stream carries no session state (no "catch up" on reconnect)

---

### 4.13 Runtime

**Nav path:** `#runtime`
**File:** `pages/runtime.js` — `runtimePage()`

**Registration:** Uses `Alpine.data('runtimePage', ...)` — the only page using this pattern.

**Data displayed:**
- Version, platform, architecture, uptime
- Default model, API listen address, home directory
- Log level, network enabled flag
- Provider list with status

**API endpoints:**
| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/api/status` | GET | Version, uptime, platform |
| `/api/version` | GET | Detailed version info |
| `/api/providers` | GET | Provider list |
| `/api/agents` | GET | Agent count |

**Functionality:**
- Read-only display of runtime internals
- No controls or configuration

**Limitations:**
- Entirely read-only; cannot change log level, network mode, etc. from this page
- No restart/reload capability
- No memory usage or CPU metrics

---

### 4.14 Settings

**Nav path:** `#settings`
**File:** `pages/settings.js` — `settingsPage()`

**Tabs:** Providers, Models, Config, Tools, Security, Network/Peers, Migration

**API endpoints:**
| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/api/providers` | GET | List providers |
| `/api/providers/{id}/key` | POST | Set API key |
| `/api/providers/{id}/key` | DELETE | Remove API key |
| `/api/providers/{id}/test` | POST | Test connectivity |
| `/api/providers/{id}/url` | PUT | Set custom base URL |
| `/api/providers/{name}/url` | PUT | Custom provider URL |
| `/api/providers/github-copilot/oauth/start` | POST | Start OAuth flow |
| `/api/providers/github-copilot/oauth/poll/{id}` | GET | Poll OAuth completion |
| `/api/models` | GET | List all models |
| `/api/models/custom` | POST | Add custom model |
| `/api/models/custom/{id}` | DELETE | Remove custom model |
| `/api/config` | GET | Current config |
| `/api/config/schema` | GET | Config JSON schema |
| `/api/config/set` | POST | Update config field |
| `/api/security` | GET | Security feature status |
| `/api/audit/verify` | GET | Audit chain verify |
| `/api/peers` | GET | Connected peers (15s poll) |
| `/api/migrate/detect` | GET | Detect migrations |
| `/api/migrate/scan` | POST | Scan for migration targets |
| `/api/migrate` | POST | Run migration (dry-run supported) |

**Functionality:**
- **Providers**: Key management, connectivity test, custom URL, GitHub Copilot OAuth (device flow polling)
- **Models**: Browse by provider, add/remove custom models with metadata
- **Config**: Form-driven config editor backed by JSON schema
- **Security**: Feature status display; hardcoded documentation for 8 core + 4 configurable + 3 monitoring features
- **Peers**: OFP network peer list, 15s auto-refresh
- **Migration**: Detect/scan/run data migrations from other AI tools with dry-run option

**Limitations:**
- Security features panel is largely hardcoded (badges not dynamically from API)
- No config field validation against schema in the browser
- Peer management is read-only (no manual peer add/remove)
- No provider ordering/priority settings

---

### 4.15 Analytics / Usage

**Nav path:** `#analytics` (also `#usage` redirects here)
**File:** `pages/usage.js` — `analyticsPage()`

**Data displayed:**
- Summary tab: total tokens, cost, messages, avg cost/message, top model, provider donut chart
- Cost tab: per-agent costs, daily bar chart (last 7 days), cost projection

**API endpoints:**
| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/api/usage/summary` | GET | Aggregate usage stats |
| `/api/usage/by-model` | GET | Usage grouped by model |
| `/api/usage` | GET | All usage records |
| `/api/usage/daily` | GET | Daily usage breakdown |

**Functionality:**
- Pure CSS/SVG donut chart (stroke-dasharray math, no Chart.js)
- SVG bar chart for 7-day daily cost
- Provider extraction from model names via `_extractProvider()` heuristic (string prefix matching)
- Cost projection: simple linear extrapolation from daily average
- Color coding per provider (hardcoded color map)

**Limitations:**
- No date range selector
- No per-agent breakdown in summary tab
- No export (CSV/JSON)
- Provider extraction is heuristic-based (may misidentify custom model names)
- No real-time updates (manual refresh button only)

---

### 4.16 Setup Wizard

**Nav path:** `#wizard`
**File:** `pages/wizard.js` — `wizardPage()`

**Data displayed:**
- 6-step onboarding flow: Welcome → Provider → Agent → Try It → Channel → Done

**API endpoints:**
| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/api/providers` | GET | List available providers |
| `/api/agents` | POST | Create agent (via TOML manifest) |
| `/api/agents/{id}/ws` | WS | Inline chat in Try It step |
| `/api/agents/{id}/stop` | POST | Stop response in Try It |

**Functionality:**
- Step 1 (Welcome): introduction, feature overview
- Step 2 (Provider): select provider, enter API key (links to docs for 12 providers)
- Step 3 (Agent): select from 10 built-in templates (categorized), customize name
- Step 4 (Try It): inline WS chat to test the new agent
- Step 5 (Channel): optional Telegram/Discord/Slack setup
- Step 6 (Done): summary, navigate to agents

`finish()` method: sets `pendingAgent` on global store + navigates to `#agents` (which picks up `pendingAgent` to open chat inline).

`defaultModelForProvider()`: maps provider IDs to default model IDs for wizard-created agents.

**Limitations:**
- Only 3 channels offered in wizard (Telegram, Discord, Slack) — not all supported channels
- No ability to go back to wizard after completion (no "restart onboarding" option in UI)
- Template customization is minimal (name only, no persona adjustment in wizard)

---

## 5. Shared UI Patterns & Components

### Modal Pattern

Modals use `x-show` with a fixed overlay div and `z-index` layering. `OpenFangToast.confirm()` creates a programmatic modal for destructive actions. Page-specific modals (spawn wizard, setup flow) are full-page overlays with step navigation.

### Loading States

All data fetches show skeleton placeholders (`.skeleton` shimmer class) or spinner icons during load. Error states show inline error messages with retry actions.

### Polling Intervals

| Page / Feature | Interval | Mechanism |
|---------------|----------|-----------|
| Global store (status + approvals) | 5s | `setInterval` in `app()` |
| Overview auto-refresh | 30s | `setInterval` in `overviewPage()` |
| Approvals page | 5s | (same as global) |
| Channel status | 15s | `setInterval` in `channelsPage()` |
| Peers (Settings) | 15s | `setInterval` in `settingsPage()` |
| WhatsApp QR | 3s | `setInterval` until confirmed |
| Browser Hand screenshot | 3s | `setInterval` while viewing |
| Logs fallback poll | 2s | `setInterval` when SSE fails |
| GitHub Copilot OAuth poll | 3s | `setInterval` until done |

### Real-time Patterns

| Pattern | Used in |
|---------|---------|
| WebSocket (streaming) | Chat, Wizard Try It |
| SSE | Logs, Comms |
| Short polling | Approvals, Channels, Peers, OAuth, WhatsApp QR |

### Relative Time Formatting

Multiple pages implement their own `timeAgo()` / `relativeTime()` / `formatTime()` helpers — these are not shared utilities but per-page implementations with similar logic.

### Markdown Rendering

`renderMarkdown(text)` in `app.js`:
1. Protects LaTeX (`$...$`, `$$...$$`, `\[...\]`, `\(...\)`) before passing to `marked.js`
2. Runs `marked.parse()`
3. Applies `highlight.js` to code blocks
4. Adds copy buttons to each code block
5. Opens external links in new tab
6. Restores LaTeX
7. Calls `renderLatex()` for math rendering

### Tool Call Display

`toolIcon(toolName)` returns inline SVG based on tool name prefix (file operations, network, code, memory, search, browser, etc.). Tool calls show in chat with collapsible result panels.

---

## 6. Known Bugs & Limitations

### Confirmed Bugs

1. **`comms.js` — Non-existent API properties**: `commsPage.startSSE()` references `OpenFangAPI.baseUrl` and `OpenFangAPI.apiKey`. The `OpenFangAPI` singleton does not expose these properties. The SSE event stream in the Comms page will fail silently — the URL constructed for the EventSource will be malformed.

2. **`workflows.js` — Native `confirm()` used for delete**: Workflow deletion uses `window.confirm()` instead of `OpenFangToast.confirm()`. This breaks the styled UX (native browser dialog appears instead of the custom modal). All other deletion flows use `OpenFangToast.confirm()`.

### Design Limitations

3. **History tab (Scheduler) is synthetic**: The Run History tab in the Scheduler page is not backed by a real API endpoint. It is assembled client-side from `job.last_run` timestamps and `trigger.fire_count` values. This means no actual history records, no timestamps for trigger fires beyond `created_at`, and history is lost on page refresh.

4. **Security panel hardcoded**: Both the Overview security systems panel and the Settings → Security tab display hardcoded feature lists that do not reflect real-time server feature flags.

5. **Comms topology is static**: The topology graph (agent parent-child tree) is fetched once on tab switch with no live updates.

6. **No shared time-formatting utility**: `timeAgo` / `relativeTime` / `formatTime` are independently implemented in multiple page files with slight behavioral differences.

7. **Runtime page unique registration pattern**: `runtimePage` uses `Alpine.data()` while all other pages use plain function declarations. This inconsistency could confuse future developers.

8. **Analytics provider extraction is heuristic**: `_extractProvider()` uses string prefix matching on model IDs to infer provider. Custom model names or renamed models may be misattributed.

9. **Workflow builder double-click**: Native `dblclick` events don't work due to SVG re-renders; manual timestamp comparison is used instead. This is a workaround that may break if render timing changes.

10. **No shared error boundary**: Each page handles its own errors independently. There is no global error handler or centralized error display pattern.

---

## 7. Complete API Endpoint Inventory

All endpoints observed in the frontend codebase, grouped by domain:

### Health & Status
- `GET /api/health`
- `GET /api/status`
- `GET /api/version`

### Auth
- `GET /api/auth/check`
- `POST /api/auth/login`
- `POST /api/auth/logout`

### Agents
- `GET /api/agents`
- `POST /api/agents`
- `DELETE /api/agents/{id}`
- `PATCH /api/agents/{id}/config`
- `PUT /api/agents/{id}/mode`
- `PUT /api/agents/{id}/model`
- `POST /api/agents/{id}/clone`
- `DELETE /api/agents/{id}/history`
- `GET /api/agents/{id}/files`
- `GET /api/agents/{id}/files/{name}`
- `PUT /api/agents/{id}/files/{name}`
- `GET /api/agents/{id}/tools`
- `PUT /api/agents/{id}/tools`
- `POST /api/agents/{id}/stop`
- `POST /api/agents/{id}/upload` (multipart)
- `WS /api/agents/{id}/ws`

### Sessions
- `GET /api/agents/{id}/sessions`
- `POST /api/agents/{id}/sessions`
- `POST /api/agents/{id}/sessions/{sid}/switch`
- `POST /api/agents/{id}/session/reset`
- `POST /api/agents/{id}/session/compact`
- `GET /api/sessions`
- `DELETE /api/sessions/{id}`

### Memory
- `GET /api/memory/agents/{id}/kv`
- `PUT /api/memory/agents/{id}/kv/{key}`
- `DELETE /api/memory/agents/{id}/kv/{key}`

### Approvals
- `GET /api/approvals`
- `POST /api/approvals/{id}/approve`
- `POST /api/approvals/{id}/reject`

### Models & Providers
- `GET /api/models`
- `POST /api/models/custom`
- `DELETE /api/models/custom/{id}`
- `GET /api/providers`
- `POST /api/providers/{id}/key`
- `DELETE /api/providers/{id}/key`
- `POST /api/providers/{id}/test`
- `PUT /api/providers/{id}/url`
- `POST /api/providers/github-copilot/oauth/start`
- `GET /api/providers/github-copilot/oauth/poll/{id}`

### Config
- `GET /api/config`
- `GET /api/config/schema`
- `POST /api/config/set`

### Templates & Profiles
- `GET /api/templates`
- `GET /api/profiles`

### Commands & Tools
- `GET /api/commands`
- `GET /api/tools`

### Usage & Analytics
- `GET /api/usage`
- `GET /api/usage/summary`
- `GET /api/usage/by-model`
- `GET /api/usage/daily`

### Audit & Logs
- `GET /api/audit/recent?n={n}`
- `GET /api/audit/verify`
- `GET /api/logs/stream` (SSE)

### Cron / Scheduler
- `GET /api/cron/jobs`
- `POST /api/cron/jobs`
- `PUT /api/cron/jobs/{id}/enable`
- `DELETE /api/cron/jobs/{id}`
- `POST /api/cron/jobs/{id}/run`

### Event Triggers
- `GET /api/triggers`
- `PUT /api/triggers/{id}`
- `DELETE /api/triggers/{id}`

### Workflows
- `GET /api/workflows`
- `POST /api/workflows`
- `PUT /api/workflows/{id}`
- `DELETE /api/workflows/{id}`
- `GET /api/workflows/{id}`
- `POST /api/workflows/{id}/run`
- `GET /api/workflows/{id}/runs`

### Channels
- `GET /api/channels`
- `POST /api/channels/{name}/configure`
- `POST /api/channels/{name}/test`
- `DELETE /api/channels/{name}/configure`
- `POST /api/channels/whatsapp/qr/start`
- `GET /api/channels/whatsapp/qr/status?session_id={id}`

### Skills & ClawHub
- `GET /api/skills`
- `POST /api/skills/uninstall`
- `POST /api/skills/create`
- `GET /api/clawhub/search?q={q}`
- `GET /api/clawhub/browse?sort={sort}`
- `GET /api/clawhub/skill/{slug}`
- `GET /api/clawhub/skill/{slug}/code`
- `POST /api/clawhub/install`

### MCP Servers
- `GET /api/mcp/servers`

### Hands
- `GET /api/hands`
- `GET /api/hands/active`
- `GET /api/hands/{id}`
- `POST /api/hands/{id}/activate`
- `POST /api/hands/{id}/install-deps`
- `POST /api/hands/{id}/check-deps`
- `DELETE /api/hands/instances/{id}`
- `POST /api/hands/instances/{id}/pause`
- `POST /api/hands/instances/{id}/resume`
- `GET /api/hands/instances/{id}/stats`
- `GET /api/hands/instances/{id}/browser`

### Comms / Agent Topology
- `GET /api/comms/topology`
- `GET /api/comms/events?limit={n}`
- `GET /api/comms/events/stream` (SSE)
- `POST /api/comms/send`
- `POST /api/comms/task`

### Peers & Network
- `GET /api/peers`

### Security
- `GET /api/security`

### Migration
- `GET /api/migrate/detect`
- `POST /api/migrate/scan`
- `POST /api/migrate`

---

*Total distinct endpoints observed: 90+*
*Total distinct pages/tabs: 16*
*Real-time mechanisms: WebSocket (chat streaming), SSE (logs, comms), polling (approvals, channels, peers, OAuth)*
