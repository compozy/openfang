## markdown

## status: completed

<task_context>
<domain>openfang-cli</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>low</complexity>
<dependencies>task_45</dependencies>
</task_context>

# Task 46.0: CLI Commands — A2A, Peers, Budget

## Overview

Add three missing CLI command groups that have full API implementations but no CLI bindings: Agent-to-Agent (A2A) communication, Peer/Network management, and Budget management. All follow the same pattern: define a clap subcommand enum, implement handlers that make HTTP requests to the running daemon.

<critical>
- **ALWAYS READ** @CLAUDE.md before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-gaps/techspec.md` (Gaps 1, 2, 3) before start
- **YOU CAN ONLY** finish when `make fmt`, `make lint`, and `make test` pass at 100%
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Add `A2aCommands` subcommand group with: list, discover, send, status
- Add `PeersCommands` subcommand group with: list, status
- Add `BudgetCommands` subcommand group with: status, update, agents, agent
- All commands must follow existing CLI patterns (HTTP client calls to daemon, JSON output with `--json` flag)
- All commands must handle daemon-not-running errors gracefully
</requirements>

## Subtasks

- [x] 46.1 Study existing CLI command patterns in `main.rs` (e.g., `SkillCommands`, `HandCommands`)
- [x] 46.2 Implement `A2aCommands` enum and register as `A2a(A2aCommands)` in `Commands`
- [x] 46.3 Implement `cmd_a2a_list()` — GET `/api/a2a/agents`
- [x] 46.4 Implement `cmd_a2a_discover(url)` — POST `/api/a2a/discover`
- [x] 46.5 Implement `cmd_a2a_send(url, message, session_id)` — POST `/api/a2a/send`
- [x] 46.6 Implement `cmd_a2a_status(id)` — GET `/api/a2a/tasks/{id}/status`
- [x] 46.7 Implement `PeersCommands` enum and register as `Peers(PeersCommands)` in `Commands`
- [x] 46.8 Implement `cmd_peers_list()` — GET `/api/peers`
- [x] 46.9 Implement `cmd_peers_status()` — GET `/api/network/status`
- [x] 46.10 Implement `BudgetCommands` enum and register as `Budget(BudgetCommands)` in `Commands`
- [x] 46.11 Implement `cmd_budget_status()` — GET `/api/budget`
- [x] 46.12 Implement `cmd_budget_update(hourly, daily, monthly)` — PUT `/api/budget`
- [x] 46.13 Implement `cmd_budget_agents()` — GET `/api/budget/agents`
- [x] 46.14 Implement `cmd_budget_agent(id)` — GET `/api/budget/agents/{id}`
- [x] 46.15 Run `make fmt && make lint && make test` — all must pass

## Implementation Details

### API Endpoints Consumed

| CLI Command | HTTP Method | API Endpoint | Response |
|-------------|-----------|--------------|----------|
| `openfang a2a list` | GET | `/api/a2a/agents` | JSON array of agent cards |
| `openfang a2a discover <url>` | POST | `/api/a2a/discover` | Agent card JSON |
| `openfang a2a send <url> <msg>` | POST | `/api/a2a/send` | Task response JSON |
| `openfang a2a status <id>` | GET | `/api/a2a/tasks/{id}/status` | Task status JSON |
| `openfang peers list` | GET | `/api/peers` | JSON array of peers |
| `openfang peers status` | GET | `/api/network/status` | Network status JSON |
| `openfang budget status` | GET | `/api/budget` | Budget summary JSON |
| `openfang budget update` | PUT | `/api/budget` | Updated budget JSON |
| `openfang budget agents` | GET | `/api/budget/agents` | Per-agent ranking JSON |
| `openfang budget agent <id>` | GET | `/api/budget/agents/{id}` | Agent budget detail JSON |

### Existing Pattern Reference

Follow the same HTTP client pattern used by existing commands. Look at handlers for `skill`, `hand`, or `cron` commands as reference for:
- HTTP client construction (`reqwest`)
- Error handling (daemon not running, connection refused)
- Output formatting (table for human, JSON for `--json`)
- API base URL resolution from config

### Relevant Files

- `crates/openfang-cli/src/main.rs` — All changes go here

### Dependent Files

- `crates/openfang-api/src/routes.rs` — API handlers (read-only reference, no changes)
- `crates/openfang-api/src/server.rs` — Route registration (read-only reference, no changes)

## Deliverables

- Three new CLI command groups: `a2a`, `peers`, `budget`
- 10 new handler functions matching the API endpoints
- All commands produce both human-readable and JSON output
- Help text for all commands and arguments

## Tests

### Unit Tests (Required)

- [x] `A2aCommands` enum parses all subcommands correctly from CLI args
- [x] `PeersCommands` enum parses all subcommands correctly
- [x] `BudgetCommands` enum parses all subcommands correctly
- [x] Budget update command validates numeric arguments

### Integration Tests (Required)

- [x] Smoke test: `openfang a2a list` returns valid output or clean error when daemon is down
- [x] Smoke test: `openfang peers list` returns valid output or clean error
- [x] Smoke test: `openfang budget status` returns valid output or clean error

### Regression and Anti-Pattern Guards

- [x] Existing CLI tests still pass (no regressions in command parsing)
- [x] No test-only production APIs introduced
- [x] Error handling follows existing daemon-not-running pattern

### Verification Commands

- [x] `make fmt`
- [x] `make lint`
- [x] `make test`

## Success Criteria

- All three command groups accessible via `openfang a2a`, `openfang peers`, `openfang budget`
- Commands produce correct output when daemon is running
- Commands produce helpful error messages when daemon is not running
- `openfang --help` shows the three new command groups
