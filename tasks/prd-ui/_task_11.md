## status: pending

<task_context>
<domain>openfang-api/static/js/pages</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>medium</complexity>
<dependencies>task_1,task_10</dependencies>
</task_context>

# Task 11.0: Chat SSE Streaming + Dry-Run Mode

## Overview

Update the chat page to support v1 SSE streaming as an alternative to WebSocket, and add a dry-run mode that previews message effects without execution.

<critical>
- **ALWAYS READ** @CLAUDE.md before start - **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-ui/techspec.md` and `tasks/prd-ui/analysis_prd_tasks_16_30.md` (task 22)
- **YOU CAN ONLY** finish when `make fmt && make lint && make test` pass AND manual browser verification confirms chat works
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Add SSE streaming option via `POST /api/v1/agents/{id}/messages/stream`
- SSE events: `message.delta`, `message.completed`, `tool.started`, `tool.completed`, `error`, `keepalive`
- Dry-run mode toggle: `POST /api/v1/agents/{id}/messages/dry-run`
- Dry-run shows: would_execute, resolved, effects, explanation
- Keep WebSocket as primary for bidirectional commands
</requirements>

## Subtasks

- [ ] 11.1 Add SSE streaming path in `chat.js` — use `POST /api/v1/agents/{id}/messages/stream` as fallback when WS unavailable
- [ ] 11.2 Handle SSE event types: `message.delta`, `message.completed`, `tool.started`, `tool.completed`, `error`
- [ ] 11.3 Add dry-run mode toggle UI — button or toggle in chat input area
- [ ] 11.4 Implement dry-run request — `POST /api/v1/agents/{id}/messages/dry-run`
- [ ] 11.5 Display dry-run results — would_execute, resolved provider, effects list, explanation text
- [ ] 11.6 Update `index_body.html` chat template for dry-run toggle and result display

## Implementation Details

### SSE Stream Events

| Event | Data |
|-------|------|
| `message.delta` | `{ content: "..." }` — append to response |
| `message.completed` | `{ content, input_tokens, output_tokens, cost_usd }` — final response |
| `tool.started` | `{ id, tool, input }` — tool call begin |
| `tool.completed` | `{ id, tool, result, is_error }` — tool call end |
| `error` | `{ message }` — error during processing |
| `keepalive` | empty — connection alive signal |

### Relevant Files

- `crates/openfang-api/static/js/pages/chat.js` (MODIFY)
- `crates/openfang-api/static/index_body.html` (MODIFY)

## Deliverables

- SSE streaming as WS fallback in chat
- Dry-run mode with preview display
- Chat functionality preserved for all existing features

## Tests

### Manual Browser Tests (Required)

- [ ] Chat via WS still works (primary path)
- [ ] Disable WS (close connection) — verify SSE fallback works
- [ ] Toggle dry-run mode — send message — verify preview without execution
- [ ] Dry-run result shows would_execute, effects, explanation

### Verification Commands

- [ ] `make fmt`
- [ ] `make lint`
- [ ] `make test`

## Success Criteria

- Chat works via both WS (primary) and SSE (fallback)
- Dry-run mode previews without executing
- No regressions in existing chat features
