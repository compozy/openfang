# Compozy Control Plane API Spec

**Status:** Current public control-plane baseline
**Date:** 2026-03-21

This document is the canonical public contract for the Compozy control plane.
It defines the target payloads and command grammar for:

- `/api/v1`
- `compozy ...`

It is written as the target product contract, not as an implementation-phase
subset.

## 1. Scope

This spec currently freezes payload conventions and public resource shapes for:

- `agents`
- `workflows`
- `triggers`
- `schedules`
- `skills`
- `packs`
- `tasks`
- `subtasks`
- `artifacts`
- `docs`
- `events`
- `runs`
- `dispatches`
- `hitl-requests`
- `looper-runs`

## 2. Common Conventions

### Resource Identity

- definition resources use stable user-controlled IDs such as `prd-writer`,
  `sdlc`, or `issue-created-start-sdlc`
- runtime resources use opaque IDs such as `run_123`, `dispatch_456`, or
  `hitl_789`

### Timestamps

- timestamps are serialized as RFC 3339 UTC strings
- examples:
  - `2026-03-21T14:05:00Z`
  - `2026-03-21T14:05:00.125Z`

### List Responses

All list endpoints return:

```json
{
  "items": [],
  "next_cursor": null
}
```

Common query parameters:

- `limit`
- `cursor`
- `sort`
- `order`
- `q` when full-text or fuzzy search is meaningful

Default rules:

- default `limit = 50`
- maximum `limit = 200`
- `next_cursor = null` means no next page

### Validation Requests

Validation endpoints use:

```json
{
  "definition": {},
  "strict": true,
  "context": {}
}
```

Validation responses use:

```json
{
  "valid": true,
  "issues": [],
  "normalized": {}
}
```

Issue objects use:

```json
{
  "severity": "error",
  "code": "missing_field",
  "path": "steps[1].uses.agent",
  "message": "agent is required for kind=agent"
}
```

### Compilation Requests

Compilation endpoints use:

```json
{
  "definition": {},
  "context": {}
}
```

Compilation responses use:

```json
{
  "definition_id": "example",
  "normalized": {},
  "compiled": {}
}
```

### Dry-Run Requests

Dry-run endpoints mirror the request shape of the real side-effecting endpoint.

Dry-run responses use:

```json
{
  "would_execute": true,
  "resolved": {},
  "effects": {},
  "explanation": {}
}
```

Design rule:

- `validate` checks whether a definition is acceptable
- `compile` returns the normalized and compiled internal form of a definition
- `dry-run` simulates a side-effecting operation without executing it
- `explanation` is an optional structured section inside `compile`, `dry-run`,
  or `test` responses rather than a universal separate endpoint

### Definition Mutation Responses

- `POST /api/v1/<resource>`
- `PUT /api/v1/<resource>/{id}`

return the full resulting resource object.

### Definition Origin Metadata

Definition resources may include origin metadata:

```json
{
  "origin": {
    "kind": "user"
  },
  "forked_from": null
}
```

Pack-managed resources may instead return:

```json
{
  "origin": {
    "kind": "pack",
    "pack_id": "sdlc",
    "pack_version": "1.2.0",
    "source": "bundled"
  },
  "forked_from": null
}
```

Forked resources remain user-owned and include upstream provenance:

```json
{
  "origin": {
    "kind": "user"
  },
  "forked_from": {
    "kind": "pack",
    "pack_id": "sdlc",
    "pack_version": "1.2.0",
    "resource_type": "workflow",
    "resource_id": "sdlc"
  }
}
```

Normal create operations must not silently shadow managed pack objects. Same-ID
overrides are created only through explicit fork operations.

### Operational Action Responses

Operational action endpoints return:

```json
{
  "accepted": true,
  "resource_id": "example",
  "status": "accepted"
}
```

When useful, action responses may include:

- `run_id`
- `session_id`
- `message_id`
- `event_id`
- `runtime`
- `warnings`

### Error Responses

Errors use:

```json
{
  "error": {
    "code": "validation_error",
    "message": "workflow definition is invalid",
    "details": []
  }
}
```

### Streaming

Streaming endpoints use server-sent events.

Common SSE event names:

- `stream.snapshot`
- `stream.reset`
- `keepalive`
- `message.delta`
- `message.completed`
- `tool.started`
- `tool.completed`
- `dispatch.updated`
- `hitl.requested`
- `run.updated`
- `error`

### Watch And Subscription Policy

The public control plane uses explicit SSE sub-resources for live operational
state.

It does **not** use a generic `watch=true` parameter across every endpoint.

Design rules:

- definition resources remain request/response by default
- live operational resources expose explicit SSE sub-resources when needed
- SSE endpoints should support `Last-Event-ID` when practical
- watch endpoints should prefer resource-local event streams over one giant
  global multiplexed stream

### Definition Contract Schema

Definition resources use one shared lightweight contract language for:

- `agent.input`
- `agent.output`
- `workflow.input`
- `workflow.output`

The canonical contract node shape is:

```json
{
  "kind": "object",
  "description": "optional",
  "nullable": false,
  "fields": {},
  "required": [],
  "open": false,
  "items": {}
}
```

Only the fields relevant to the chosen `kind` are present.

Canonical structural kinds:

- `string`
- `integer`
- `number`
- `boolean`
- `object`
- `array`
- `any`

Canonical semantic kinds:

- `artifact_ref`
- `doc_ref`
- `issue_ref`
- `task_ref`
- `task_list`
- `run_ref`

Kind-specific metadata may be added for semantic kinds. Examples:

- `artifact_type` on `artifact_ref`
- `doc_type` on `doc_ref`

Design rules:

- `object` uses `fields`, `required`, and `open`
- `array` uses `items`
- object contracts default to `open = false`
- `kind = "any"` is the explicit escape hatch for arbitrary JSON
- full JSON Schema is not the canonical authoring format for definitions

Normalization may accept a few convenience aliases:

- `text` normalizes to `string`
- `json` normalizes to `any`

Example contract:

```json
{
  "kind": "object",
  "required": ["issue_id"],
  "open": false,
  "fields": {
    "issue_id": {
      "kind": "string",
      "description": "Issue identifier"
    },
    "context": {
      "kind": "object",
      "open": true
    }
  }
}
```
- replay is bounded, not infinite
- durable history should come from normal resource endpoints, not from treating
  SSE as a full history API

Replay rules:

- `Last-Event-ID` provides best-effort resume within a bounded replay window
- if the requested event is no longer available, the server should emit
  `stream.reset`, then `stream.snapshot`, then continue live events
- unbounded backfill is out of scope for watch endpoints

Initial watch surfaces:

- `POST /api/v1/agents/{id}/messages/stream`
- `GET /api/v1/runs/{id}/events`
- `GET /api/v1/dispatches/{id}/events`
- `GET /api/v1/hitl-requests/stream`
- `GET /api/v1/looper-runs/{id}/events`

## 3. Agents

### Resource Shape

The canonical agent resource is the public `agent_definition` plus read-only
metadata.

Installation and workspace provider plumbing is intentionally **not** part of
the agent resource.

The agent resource may include:

- `provider.driver`
- `provider.model`
- `provider.profile`
- `provider.defaults`
- `provider.config`
- optional `provider.request_extra`

`provider.request_extra` is an advanced request-level escape hatch. It is not a
general place for credentials, environment variables, provider bootstrap, or
transport infrastructure.

```json
{
  "id": "prd-writer",
  "name": "PRD Writer",
  "version": "1.0.0",
  "description": "Writes and iterates on product requirement documents",
  "enabled": true,
  "group": "sdlc",
  "tags": ["docs", "prd", "planning"],
  "provider": {
    "driver": "claude_code",
    "model": "sonnet",
    "profile": "default",
    "defaults": {
      "reasoning_effort": "high",
      "max_tokens": 8000
    },
    "config": {
      "continue_conversation": true,
      "fork_session": false,
      "allowed_tools": ["Read", "Write", "Bash"],
      "disallowed_tools": [],
      "additional_directories": ["./docs"],
      "max_budget_usd": 5.0,
      "fallback_model": "sonnet"
    }
  },
  "prompt": {
    "system": "You are a senior product writer.",
    "instructions": "Write clear, implementation-ready PRDs.",
    "skills": ["writing", "prd"]
  },
  "capabilities": {
    "tools": ["*"],
    "primitives": ["issue.read", "artifact.*", "doc.*", "hitl.*"],
    "delegation": ["call", "send"],
    "workspace": "none",
    "network": true
  },
  "runtime": {
    "autonomous": true,
    "memory_policy": "session",
    "hitl": "explicit_only"
  },
  "input": {
    "kind": "object"
  },
  "output": {
    "kind": "artifact_ref",
    "artifact_type": "prd"
  },
  "origin": {
    "kind": "user"
  },
  "forked_from": null,
  "created_at": "2026-03-21T12:00:00Z",
  "updated_at": "2026-03-21T14:00:00Z"
}
```

### Endpoints

- `GET /api/v1/agents`
- `POST /api/v1/agents`
- `POST /api/v1/agents/validate`
- `POST /api/v1/agents/compile`
- `GET /api/v1/agents/{id}`
- `PUT /api/v1/agents/{id}`
- `DELETE /api/v1/agents/{id}`
- `POST /api/v1/agents/{id}/fork`
- `GET /api/v1/agents/{id}/compiled`
- `GET /api/v1/agents/{id}/runtime`
- `POST /api/v1/agents/{id}/runtime/start`
- `POST /api/v1/agents/{id}/runtime/stop`
- `POST /api/v1/agents/{id}/runtime/restart`
- `PUT /api/v1/agents/{id}/runtime/mode`
- `GET /api/v1/agents/{id}/sessions`
- `POST /api/v1/agents/{id}/sessions`
- `GET /api/v1/agents/{id}/sessions/{session_id}`
- `POST /api/v1/agents/{id}/sessions/{session_id}/activate`
- `POST /api/v1/agents/{id}/sessions/{session_id}/reset`
- `POST /api/v1/agents/{id}/sessions/{session_id}/compact`
- `POST /api/v1/agents/{id}/messages`
- `POST /api/v1/agents/{id}/messages/stream`
- `POST /api/v1/agents/{id}/messages/dry-run`

### List Filters

- `group`
- `tag`
- `enabled`
- `provider_driver`
- `q`

### List Item Shape

```json
{
  "id": "prd-writer",
  "name": "PRD Writer",
  "description": "Writes and iterates on PRDs",
  "enabled": true,
  "group": "sdlc",
  "tags": ["docs", "prd", "planning"],
  "provider": {
    "driver": "claude_code",
    "model": "sonnet",
    "profile": "default"
  },
  "origin": {
    "kind": "pack",
    "pack_id": "sdlc"
  },
  "runtime_status": {
    "loaded": true,
    "healthy": true,
    "active_sessions": 2,
    "active_dispatches": 1
  },
  "updated_at": "2026-03-21T14:00:00Z"
}
```

### Compiled Response

`GET /api/v1/agents/{id}/compiled` returns:

```json
{
  "definition_id": "prd-writer",
  "normalized": {},
  "compiled": {
    "agent_manifest": {},
    "provider_binding": {},
    "product_metadata": {}
  }
}
```

### Runtime Resource

`GET /api/v1/agents/{id}/runtime` returns:

```json
{
  "agent_id": "prd-writer",
  "loaded": true,
  "state": "running",
  "mode": "normal",
  "healthy": true,
  "active_session_id": "session_main",
  "active_sessions": 2,
  "active_dispatches": 1,
  "last_active_at": "2026-03-21T14:05:00Z"
}
```

### Sessions

List item:

```json
{
  "id": "session_main",
  "label": "Main",
  "active": true,
  "message_count": 24,
  "dispatch_count": 4,
  "updated_at": "2026-03-21T14:05:00Z"
}
```

Create request:

```json
{
  "label": "New Session"
}
```

Detail shape:

```json
{
  "id": "session_main",
  "label": "Main",
  "active": true,
  "message_count": 24,
  "dispatch_count": 4,
  "created_at": "2026-03-21T12:05:00Z",
  "updated_at": "2026-03-21T14:05:00Z"
}
```

### Message Submission

`POST /api/v1/agents/{id}/messages` request:

```json
{
  "session_id": "session_main",
  "input": {
    "items": [
      {
        "type": "text",
        "text": "Create a PRD for issue ISSUE-123"
      }
    ]
  },
  "metadata": {
    "source": "api"
  }
}
```

Response:

```json
{
  "accepted": true,
  "resource_id": "prd-writer",
  "status": "accepted",
  "session_id": "session_main",
  "message_id": "msg_123"
}
```

`POST /api/v1/agents/{id}/messages/stream` uses the same request body and
returns SSE.

`POST /api/v1/agents/{id}/messages/dry-run` uses the same request body and
returns:

```json
{
  "would_execute": true,
  "resolved": {
    "agent_id": "prd-writer",
    "session_id": "session_main",
    "provider": {
      "driver": "claude_code",
      "model": "sonnet"
    }
  },
  "effects": {
    "message_submit": true
  },
  "explanation": {
    "skills": ["writing", "prd"],
    "capabilities": {
      "network": true,
      "workspace": "none"
    }
  }
}
```

## 4. Workflows

### Resource Shape

`input` describes the accepted workflow input contract.

`output` describes the final result contract exposed by the workflow.

`outputs` maps runtime symbols into that final result shape.

```json
{
  "id": "sdlc",
  "name": "SDLC",
  "version": "1.0.0",
  "description": "First-party SDLC workflow",
  "enabled": true,
  "tags": ["sdlc", "planning"],
  "input": {
    "kind": "object",
    "required": ["issue_id"],
    "open": false,
    "fields": {
      "issue_id": {
        "kind": "string"
      }
    }
  },
  "output": {
    "kind": "object",
    "required": ["prd"],
    "open": false,
    "fields": {
      "prd": {
        "kind": "artifact_ref",
        "artifact_type": "prd"
      }
    }
  },
  "defaults": {
    "timeout_secs": 120,
    "error_mode": "fail"
  },
  "steps": [
    {
      "id": "load-issue",
      "name": "Load Issue",
      "kind": "primitive",
      "uses": {
        "primitive": "issue.read"
      },
      "with": {
        "issue_id": "{{ input.issue_id }}"
      },
      "save_as": "issue",
      "flow": {
        "mode": "sequential"
      }
    },
    {
      "id": "write-prd",
      "name": "Write PRD",
      "kind": "agent",
      "uses": {
        "agent": "prd-writer"
      },
      "with": {
        "issue": "{{ vars.issue }}"
      },
      "save_as": "prd",
      "flow": {
        "mode": "sequential"
      },
      "runtime": {
        "timeout_secs": 300,
        "error_mode": "fail"
      }
    }
  ],
  "outputs": {
    "prd": "{{ vars.prd }}"
  },
  "origin": {
    "kind": "pack",
    "pack_id": "sdlc",
    "pack_version": "1.2.0",
    "source": "bundled"
  },
  "forked_from": null,
  "created_at": "2026-03-21T12:00:00Z",
  "updated_at": "2026-03-21T14:00:00Z"
}
```

### Endpoints

- `GET /api/v1/workflows`
- `POST /api/v1/workflows`
- `POST /api/v1/workflows/validate`
- `POST /api/v1/workflows/compile`
- `GET /api/v1/workflows/{id}`
- `PUT /api/v1/workflows/{id}`
- `DELETE /api/v1/workflows/{id}`
- `POST /api/v1/workflows/{id}/fork`
- `GET /api/v1/workflows/{id}/compiled`
- `GET /api/v1/workflows/{id}/runtime`
- `POST /api/v1/workflows/{id}/runs`
- `POST /api/v1/workflows/{id}/runs/dry-run`
- `GET /api/v1/workflows/{id}/runs`

### List Filters

- `enabled`
- `tag`
- `q`

### List Item Shape

```json
{
  "id": "sdlc",
  "name": "SDLC",
  "description": "First-party SDLC workflow",
  "enabled": true,
  "tags": ["sdlc", "planning"],
  "steps": 4,
  "origin": {
    "kind": "pack",
    "pack_id": "sdlc",
    "pack_version": "1.2.0",
    "source": "bundled"
  },
  "runtime_status": {
    "loaded": true,
    "active_runs": 3,
    "waiting_runs": 1
  },
  "updated_at": "2026-03-21T14:00:00Z"
}
```

### Compiled Response

`GET /api/v1/workflows/{id}/compiled` returns:

```json
{
  "definition_id": "sdlc",
  "normalized": {},
  "compiled": {
    "workflow_ir": {}
  }
}
```

### Runtime Resource

`GET /api/v1/workflows/{id}/runtime` returns:

```json
{
  "workflow_id": "sdlc",
  "loaded": true,
  "healthy": true,
  "active_runs": 3,
  "waiting_runs": 1,
  "last_run_at": "2026-03-21T14:05:00Z"
}
```

### Run Creation

`POST /api/v1/workflows/{id}/runs` request:

```json
{
  "input": {
    "issue_id": "ISSUE-123"
  },
  "labels": ["manual"],
  "metadata": {
    "source": "api"
  }
}
```

Response:

```json
{
  "accepted": true,
  "resource_id": "sdlc",
  "status": "accepted",
  "run_id": "run_123"
}
```

`POST /api/v1/workflows/{id}/runs/dry-run` uses the same request body and
returns:

```json
{
  "would_execute": true,
  "resolved": {
    "workflow_id": "sdlc",
    "workflow_version": "1.0.0",
    "initial_step_id": "load-issue"
  },
  "effects": {
    "run_create": true,
    "initial_dispatches": 0
  },
  "explanation": {
    "input_contract": {
      "kind": "object",
      "required": ["issue_id"],
      "open": false,
      "fields": {
        "issue_id": {
          "kind": "string"
        }
      }
    },
    "output_contract": {
      "kind": "object",
      "required": ["prd"],
      "open": false,
      "fields": {
        "prd": {
          "kind": "artifact_ref",
          "artifact_type": "prd"
        }
      }
    }
  }
}
```

### Workflow-Scoped Run Summary

`GET /api/v1/workflows/{id}/runs` returns list items like:

```json
{
  "id": "run_123",
  "status": "running",
  "current_step_id": "write-prd",
  "started_at": "2026-03-21T14:05:00Z",
  "updated_at": "2026-03-21T14:06:00Z"
}
```

## 5. Triggers

### Resource Shape

```json
{
  "id": "issue-created-start-sdlc",
  "name": "Issue Created Starts SDLC",
  "description": "Starts SDLC when an issue is created",
  "enabled": true,
  "max_fires": 0,
  "cooldown_secs": 0,
  "match": {
    "event": "issue.created"
  },
  "target": {
    "kind": "workflow_start",
    "workflow": "sdlc",
    "input": {
      "issue_id": "{{ event.issue_id }}"
    }
  },
  "created_at": "2026-03-21T12:00:00Z",
  "updated_at": "2026-03-21T14:00:00Z"
}
```

### Endpoints

- `GET /api/v1/triggers`
- `POST /api/v1/triggers`
- `POST /api/v1/triggers/validate`
- `POST /api/v1/triggers/compile`
- `GET /api/v1/triggers/{id}`
- `PUT /api/v1/triggers/{id}`
- `DELETE /api/v1/triggers/{id}`
- `POST /api/v1/triggers/{id}/fork`
- `GET /api/v1/triggers/{id}/compiled`
- `GET /api/v1/triggers/{id}/runtime`
- `POST /api/v1/triggers/{id}/enable`
- `POST /api/v1/triggers/{id}/disable`
- `POST /api/v1/triggers/{id}/test`

### List Filters

- `enabled`
- `event`
- `target_kind`
- `q`

### List Item Shape

```json
{
  "id": "issue-created-start-sdlc",
  "name": "Issue Created Starts SDLC",
  "enabled": true,
  "match": {
    "event": "issue.created"
  },
  "target": {
    "kind": "workflow_start",
    "workflow": "sdlc"
  },
  "runtime_status": {
    "enabled": true,
    "fire_count": 12,
    "last_fired_at": "2026-03-21T14:00:00Z"
  },
  "updated_at": "2026-03-21T14:00:00Z"
}
```

### Compiled Response

`GET /api/v1/triggers/{id}/compiled` returns:

```json
{
  "definition_id": "issue-created-start-sdlc",
  "normalized": {},
  "compiled": {
    "trigger_ir": {}
  }
}
```

### Runtime Resource

`GET /api/v1/triggers/{id}/runtime` returns:

```json
{
  "trigger_id": "issue-created-start-sdlc",
  "enabled": true,
  "fire_count": 12,
  "max_fires": 0,
  "cooldown_secs": 0,
  "last_fired_at": "2026-03-21T14:00:00Z"
}
```

### Trigger Test

`POST /api/v1/triggers/{id}/test` request:

```json
{
  "event": {
    "event": "issue.created",
    "source": "api",
    "payload": {
      "issue_id": "ISSUE-123"
    }
  }
}
```

Response:

```json
{
  "matched": true,
  "resolved_target": {
    "kind": "workflow_start",
    "workflow": "sdlc",
    "input": {
      "issue_id": "ISSUE-123"
    }
  },
  "would_dispatch": true,
  "explanation": {
    "match": "event matched by exact event name",
    "target_kind": "workflow_start"
  }
}
```

## 6. Schedules

### Resource Shape

The schedule surface remains closely aligned with the OpenFang typed cron
model.

`schedule` and `delivery` stay near-canonical.

`action` keeps the same basic action family, but the action payloads align with
the rest of the Compozy control plane.

```json
{
  "id": "sched_123",
  "agent": "prd-writer",
  "name": "Nightly Repo Review",
  "enabled": true,
  "schedule": {
    "kind": "cron",
    "expr": "0 2 * * *",
    "tz": "America/Sao_Paulo"
  },
  "action": {
    "kind": "workflow_run",
    "workflow_id": "repo-review",
    "input": {
      "scope": "open_prs"
    },
    "timeout_secs": 300
  },
  "delivery": {
    "kind": "none"
  },
  "created_at": "2026-03-21T12:00:00Z",
  "runtime_status": {
    "last_run": "2026-03-21T02:00:00Z",
    "next_run": "2026-03-22T02:00:00Z",
    "last_status": "ok",
    "consecutive_errors": 0,
    "one_shot": false
  }
}
```

### Endpoints

- `GET /api/v1/schedules`
- `POST /api/v1/schedules`
- `POST /api/v1/schedules/validate`
- `GET /api/v1/schedules/{id}`
- `PUT /api/v1/schedules/{id}`
- `DELETE /api/v1/schedules/{id}`
- `POST /api/v1/schedules/{id}/fork`
- `GET /api/v1/schedules/{id}/runtime`
- `POST /api/v1/schedules/{id}/enable`
- `POST /api/v1/schedules/{id}/disable`
- `POST /api/v1/schedules/{id}/run-now`
- `POST /api/v1/schedules/{id}/run-now/dry-run`

### List Filters

- `agent`
- `enabled`
- `schedule_kind`
- `action_kind`
- `q`

### List Item Shape

```json
{
  "id": "sched_123",
  "agent": "prd-writer",
  "name": "Nightly Repo Review",
  "enabled": true,
  "schedule": {
    "kind": "cron",
    "expr": "0 2 * * *",
    "tz": "America/Sao_Paulo"
  },
  "action": {
    "kind": "workflow_run",
    "workflow_id": "repo-review"
  },
  "runtime_status": {
    "next_run": "2026-03-22T02:00:00Z",
    "last_status": "ok"
  },
  "updated_at": "2026-03-21T14:00:00Z"
}
```

### Action Kinds

Supported action kinds:

- `system_event`
- `agent_turn`
- `workflow_run`
- `workflow_signal`

#### `system_event`

```json
{
  "kind": "system_event",
  "event": "nightly.review.requested",
  "payload": {
    "scope": "open_prs"
  }
}
```

#### `agent_turn`

This reuses the same input item model as agent messaging.

```json
{
  "kind": "agent_turn",
  "input": {
    "items": [
      {
        "type": "text",
        "text": "Review the open PRs for regressions"
      }
    ]
  },
  "model_override": null,
  "timeout_secs": 180
}
```

#### `workflow_run`

```json
{
  "kind": "workflow_run",
  "workflow_id": "repo-review",
  "input": {
    "scope": "open_prs"
  },
  "timeout_secs": 300
}
```

#### `workflow_signal`

```json
{
  "kind": "workflow_signal",
  "signal": "deadline_reached",
  "selector": {
    "workflow_id": "release-prep"
  },
  "payload": {
    "reason": "scheduled_check"
  }
}
```

### Delivery Kinds

Supported delivery kinds:

- `none`
- `channel`
- `last_channel`
- `webhook`

### Validation

Schedules intentionally expose validation but not a separate compilation
surface, because they map closely to the typed scheduler model and do not
currently need a separate public compilation artifact.

### Runtime Resource

`GET /api/v1/schedules/{id}/runtime` returns:

```json
{
  "schedule_id": "sched_123",
  "enabled": true,
  "last_run": "2026-03-21T02:00:00Z",
  "next_run": "2026-03-22T02:00:00Z",
  "last_status": "ok",
  "consecutive_errors": 0,
  "one_shot": false
}
```

### Run Now

`POST /api/v1/schedules/{id}/run-now` request:

```json
{
  "metadata": {
    "source": "api"
  }
}
```

Response:

```json
{
  "accepted": true,
  "resource_id": "sched_123",
  "status": "accepted"
}
```

`POST /api/v1/schedules/{id}/run-now/dry-run` uses the same request body and
returns:

```json
{
  "would_execute": true,
  "resolved": {
    "schedule_id": "sched_123",
    "action": {
      "kind": "workflow_run",
      "workflow_id": "repo-review"
    }
  },
  "effects": {
    "schedule_fire": true
  },
  "explanation": {
    "delivery": {
      "kind": "none"
    }
  }
}
```

## 7. Packs

### Resource Shape

```json
{
  "id": "sdlc",
  "name": "SDLC",
  "version": "1.2.0",
  "source": {
    "kind": "bundled"
  },
  "installed": true,
  "managed": true,
  "objects": {
    "agents": 5,
    "workflows": 2,
    "triggers": 3,
    "schedules": 1,
    "templates": 4
  },
  "updated_at": "2026-03-21T14:00:00Z"
}
```

### Endpoints

- `GET /api/v1/packs`
- `GET /api/v1/packs/{id}`
- `GET /api/v1/packs/{id}/objects`
- `POST /api/v1/packs/install`
- `POST /api/v1/packs/{id}/upgrade`
- `POST /api/v1/packs/{id}/upgrade/dry-run`
- `POST /api/v1/packs/{id}/uninstall`
- `POST /api/v1/packs/{id}/fork`

### Pack Fork

`POST /api/v1/packs/{id}/fork` forks a managed pack object to user-owned.

Request:

```json
{
  "resource_type": "workflow",
  "resource_id": "sdlc-main"
}
```

Response:

```json
{
  "accepted": true,
  "resource_id": "sdlc-main",
  "forked_from": {
    "pack_id": "sdlc",
    "version": "1.2.0"
  }
}
```

### Pack Design Rules

- packs are versioned distribution units
- installations pin exact pack versions
- upgrades are explicit, not automatic
- bundled first-party packs use the same model as other managed packs
- pack-managed definitions are immutable in place
- customization happens through explicit fork operations on managed
  definitions, not by editing managed pack content directly

### Install Request

`POST /api/v1/packs/install` request:

```json
{
  "source": {
    "kind": "bundled",
    "pack_id": "sdlc",
    "version": "1.2.0"
  }
}
```

### Upgrade Dry-Run

`POST /api/v1/packs/{id}/upgrade/dry-run` request:

```json
{
  "target_version": "1.3.0"
}
```

Response:

```json
{
  "would_execute": true,
  "resolved": {
    "pack_id": "sdlc",
    "from_version": "1.2.0",
    "to_version": "1.3.0"
  },
  "effects": {
    "managed_objects_added": 1,
    "managed_objects_updated": 3,
    "managed_objects_removed": 0,
    "forks_untouched": 2
  },
  "explanation": {
    "managed_objects_only": true,
    "forks_remain_detached": true
  }
}
```

### Definition Fork Request

Definition resources that may come from packs expose an explicit fork endpoint.

`POST /api/v1/workflows/{id}/fork` request:

```json
{
  "mode": "shadow"
}
```

Response:

```json
{
  "id": "sdlc",
  "origin": {
    "kind": "user"
  },
  "forked_from": {
    "kind": "pack",
    "pack_id": "sdlc",
    "pack_version": "1.2.0",
    "resource_type": "workflow",
    "resource_id": "sdlc"
  }
}
```

## 8. Event Ingress

### Endpoint

- `POST /api/v1/events`
- `POST /api/v1/events/dry-run`

### Request Shape

```json
{
  "event": "issue.created",
  "source": "api",
  "payload": {
    "issue_id": "ISSUE-123"
  },
  "idempotency_key": "issue-created-ISSUE-123",
  "occurred_at": "2026-03-21T14:10:00Z",
  "metadata": {
    "actor": "system"
  }
}
```

### Response Shape

```json
{
  "accepted": true,
  "resource_id": "evt_123",
  "status": "accepted",
  "event_id": "evt_123",
  "matched_triggers": ["issue-created-start-sdlc"],
  "effects": {
    "workflow_starts": 1,
    "workflow_signals": 0,
    "agent_messages": 0
  }
}
```

`POST /api/v1/events/dry-run` uses the same request body and returns:

```json
{
  "would_execute": true,
  "resolved": {
    "event": "issue.created",
    "source": "api"
  },
  "effects": {
    "matched_triggers": ["issue-created-start-sdlc"],
    "workflow_starts": 1,
    "workflow_signals": 0,
    "agent_messages": 0
  },
  "explanation": {
    "matching_mode": "trigger_engine"
  }
}
```

## 9. Runs

### Endpoints

- `GET /api/v1/runs`
- `GET /api/v1/runs/{id}`
- `GET /api/v1/runs/{id}/checkpoints`
- `GET /api/v1/runs/{id}/dispatches`
- `GET /api/v1/runs/{id}/hitl-requests`
- `GET /api/v1/runs/{id}/signals`
- `POST /api/v1/runs/{id}/signals`
- `POST /api/v1/runs/{id}/pause`
- `POST /api/v1/runs/{id}/resume`
- `POST /api/v1/runs/{id}/cancel`

### List Filters

- `workflow_id`
- `status`
- `waiting_kind`
- `label`
- `q`

### Run Detail Shape

```json
{
  "id": "run_123",
  "workflow_id": "sdlc",
  "workflow_version": "1.0.0",
  "status": "running",
  "input": {
    "issue_id": "ISSUE-123"
  },
  "vars": {
    "issue": {
      "id": "ISSUE-123"
    }
  },
  "current_step_id": "write-prd",
  "waiting_kind": null,
  "waiting_ref": null,
  "active_dispatch_id": "dispatch_456",
  "active_hitl_request_id": null,
  "labels": ["manual"],
  "metadata": {
    "source": "api"
  },
  "error": null,
  "started_at": "2026-03-21T14:05:00Z",
  "updated_at": "2026-03-21T14:06:00Z",
  "completed_at": null
}
```

### Run Summary Shape

```json
{
  "id": "run_123",
  "workflow_id": "sdlc",
  "status": "running",
  "current_step_id": "write-prd",
  "waiting_kind": null,
  "started_at": "2026-03-21T14:05:00Z",
  "updated_at": "2026-03-21T14:06:00Z"
}
```

### Signal Submission

`POST /api/v1/runs/{id}/signals` request:

```json
{
  "name": "artifact_approved",
  "payload": {
    "artifact_id": "artifact_001"
  },
  "source": "api",
  "idempotency_key": "artifact_approved_artifact_001"
}
```

Signal detail shape:

```json
{
  "id": "signal_654",
  "run_id": "run_123",
  "name": "artifact_approved",
  "payload": {
    "artifact_id": "artifact_001"
  },
  "source": "api",
  "consumed": true,
  "created_at": "2026-03-21T14:15:00Z",
  "consumed_at": "2026-03-21T14:15:01Z"
}
```

### Checkpoint Shape

```json
{
  "id": "chk_123",
  "run_id": "run_123",
  "step_id": "write-prd",
  "kind": "dispatch_created",
  "data": {
    "dispatch_id": "dispatch_456"
  },
  "created_at": "2026-03-21T14:05:10Z"
}
```

## 10. Dispatches

### Endpoints

- `GET /api/v1/dispatches`
- `GET /api/v1/dispatches/{id}`
- `GET /api/v1/dispatches/{id}/children`
- `POST /api/v1/dispatches/{id}/retry`
- `POST /api/v1/dispatches/{id}/cancel`

### List Filters

- `run_id`
- `status`
- `target_agent`
- `step_id`

### Dispatch Detail Shape

```json
{
  "id": "dispatch_456",
  "run_id": "run_123",
  "step_id": "write-prd",
  "kind": "call",
  "target_agent": "prd-writer",
  "status": "waiting_hitl",
  "input": {
    "issue": {
      "id": "ISSUE-123"
    }
  },
  "result": null,
  "error": null,
  "attempt": 1,
  "parent_dispatch_id": null,
  "spawned_agent_id": null,
  "started_at": "2026-03-21T14:05:10Z",
  "updated_at": "2026-03-21T14:06:00Z",
  "completed_at": null
}
```

### Dispatch Summary Shape

```json
{
  "id": "dispatch_456",
  "run_id": "run_123",
  "step_id": "write-prd",
  "kind": "call",
  "target_agent": "prd-writer",
  "status": "waiting_hitl",
  "updated_at": "2026-03-21T14:06:00Z"
}
```

## 11. HITL Requests

### Endpoints

- `GET /api/v1/hitl-requests`
- `GET /api/v1/hitl-requests/{id}`
- `POST /api/v1/hitl-requests/{id}/answer`
- `POST /api/v1/hitl-requests/{id}/cancel`

### List Filters

- `run_id`
- `dispatch_id`
- `status`
- `kind`

### HITL Detail Shape

```json
{
  "id": "hitl_789",
  "run_id": "run_123",
  "step_id": "write-prd",
  "dispatch_id": "dispatch_456",
  "kind": "clarification",
  "status": "pending",
  "question": "Should the PRD prioritize B2B admins or end users first?",
  "context": {
    "artifact_type": "prd",
    "artifact_id": "artifact_001"
  },
  "response": null,
  "sequence_no": 1,
  "created_at": "2026-03-21T14:05:45Z",
  "answered_at": null,
  "timeout_at": null
}
```

### HITL Answer Request

```json
{
  "response": {
    "type": "choice",
    "value": "b2b_admins_first"
  },
  "metadata": {
    "source": "api"
  }
}
```

## 12. Tasks And Subtasks

### Task Resource Shape

Tasks are durable domain objects, not temporary queue items.

They anchor the durable context of work, including linked artifacts, docs,
files, repositories, and labels when relevant.

```json
{
  "id": "task_001",
  "slug": "onboarding-revamp-prd",
  "title": "Prepare PRD for onboarding revamp",
  "description": "Define the new onboarding flow and acceptance criteria",
  "status": "planned",
  "priority": "high",
  "complexity": "medium",
  "position": 1,
  "source": {
    "kind": "workflow",
    "workflow_id": "sdlc",
    "run_id": "run_123"
  },
  "owner": {
    "kind": "agent_group",
    "ref": "sdlc"
  },
  "created_by": {
    "kind": "agent",
    "ref": "planner"
  },
  "repository_refs": [
    {
      "repository_id": "repo_main",
      "role": "primary"
    }
  ],
  "label_refs": ["planning", "prd"],
  "artifact_refs": [
    {
      "artifact_id": "artifact_001",
      "type": "prd"
    }
  ],
  "doc_refs": [
    {
      "doc_id": "doc_001",
      "type": "brief"
    }
  ],
  "file_refs": [
    {
      "path": "docs/prd.md",
      "kind": "workspace",
      "description": "Current PRD draft"
    }
  ],
  "metadata": {},
  "created_at": "2026-03-21T14:00:00Z",
  "updated_at": "2026-03-21T14:05:00Z",
  "completed_at": null
}
```

### Task Endpoints

- `GET /api/v1/tasks`
- `POST /api/v1/tasks`
- `GET /api/v1/tasks/{id}`
- `PUT /api/v1/tasks/{id}`
- `DELETE /api/v1/tasks/{id}`
- `POST /api/v1/tasks/{id}/replan`
- `GET /api/v1/tasks/{id}/subtasks`
- `POST /api/v1/tasks/{id}/subtasks`
- `GET /api/v1/tasks/{id}/artifacts`
- `GET /api/v1/tasks/{id}/docs`
- `GET /api/v1/tasks/{id}/files`
- `GET /api/v1/subtasks`
- `GET /api/v1/subtasks/{id}`
- `PUT /api/v1/subtasks/{id}`
- `DELETE /api/v1/subtasks/{id}`

### Task List Filters

- `status`
- `priority`
- `created_by`
- `source_kind`
- `label`
- `repository`
- `q`

### Task Summary Shape

```json
{
  "id": "task_001",
  "slug": "onboarding-revamp-prd",
  "title": "Prepare PRD for onboarding revamp",
  "status": "in_progress",
  "priority": "high",
  "position": 1,
  "source": {
    "kind": "workflow",
    "run_id": "run_123"
  },
  "updated_at": "2026-03-21T14:05:00Z"
}
```

### Subtask Detail Shape

Subtasks are executable child units inside a task.

They carry local execution context, targeting, dependencies, and results.

```json
{
  "id": "subtask_001",
  "task_id": "task_001",
  "title": "Draft problem statement",
  "description": "Write the initial problem statement for the PRD",
  "kind": "doc_change",
  "status": "ready",
  "complexity": "medium",
  "position": 1,
  "assignee": {
    "kind": "agent",
    "ref": "prd-writer"
  },
  "depends_on": [],
  "parallelizable": false,
  "input": {
    "artifact_id": "artifact_001"
  },
  "result": null,
  "metadata": {},
  "created_at": "2026-03-21T14:01:00Z",
  "updated_at": "2026-03-21T14:05:00Z",
  "completed_at": null
}
```

### Subtask List Filters

For `GET /api/v1/tasks/{id}/subtasks` and `GET /api/v1/subtasks`:

- `task_id`
- `status`
- `assignee_kind`
- `assignee_ref`
- `kind`
- `ready`
- `blocked`

### Subtask Summary Shape

```json
{
  "id": "subtask_001",
  "task_id": "task_001",
  "title": "Draft problem statement",
  "status": "ready",
  "assignee": {
    "kind": "agent",
    "ref": "prd-writer"
  },
  "updated_at": "2026-03-21T14:05:00Z"
}
```

### Replan Request

`replan` explicitly changes the subtask plan of an existing task without
replacing the task identity itself.

```json
{
  "reason": "Split the implementation work into smaller review-driven subtasks",
  "operations": [
    {
      "op": "cancel_subtasks",
      "subtask_ids": ["subtask_003"]
    },
    {
      "op": "create_subtasks",
      "items": [
        {
          "title": "Review clarity issues",
          "description": "Resolve review comments related to clarity",
          "kind": "review_item",
          "position": 4,
          "assignee": {
            "kind": "agent",
            "ref": "prd-writer"
          },
          "depends_on": ["subtask_001"],
          "parallelizable": true,
          "input": {}
        }
      ]
    },
    {
      "op": "update_subtasks",
      "items": [
        {
          "id": "subtask_004",
          "depends_on": ["subtask_001"]
        }
      ]
    }
  ],
  "metadata": {
    "source": "agent"
  }
}
```

### Replan Response

```json
{
  "accepted": true,
  "resource_id": "task_001",
  "status": "accepted",
  "effects": {
    "created_subtasks": 1,
    "updated_subtasks": 1,
    "cancelled_subtasks": 1
  }
}
```

### Linked Context Shapes

`GET /api/v1/tasks/{id}/artifacts`:

```json
{
  "items": [
    {
      "artifact_id": "artifact_001",
      "type": "prd",
      "current_version_id": "artifact_v3"
    }
  ],
  "next_cursor": null
}
```

`GET /api/v1/tasks/{id}/docs`:

```json
{
  "items": [
    {
      "doc_id": "doc_001",
      "type": "brief",
      "current_version_id": "doc_v2"
    }
  ],
  "next_cursor": null
}
```

`GET /api/v1/tasks/{id}/files`:

```json
{
  "items": [
    {
      "path": "docs/prd.md",
      "kind": "workspace",
      "description": "Current PRD draft"
    }
  ],
  "next_cursor": null
}
```

## 13. Looper Runs

### Endpoints

- `POST /api/v1/looper-runs`
- `GET /api/v1/looper-runs`
- `GET /api/v1/looper-runs/{id}`
- `GET /api/v1/looper-runs/{id}/subtasks`
- `POST /api/v1/looper-runs/{id}/pause`
- `POST /api/v1/looper-runs/{id}/resume`
- `POST /api/v1/looper-runs/{id}/cancel`

### List Filters

- `task_id`
- `source_run_id`
- `status`
- `execution_mode`

### Looper Run Creation

Creating a looper run is the canonical way to start looper execution for a
task.

```json
{
  "task_id": "task_001",
  "subtask_ids": null,
  "execution_policy": {
    "mode": "parallel",
    "max_parallelism": 4,
    "selection": "priority"
  },
  "metadata": {
    "source": "api"
  }
}
```

### Looper Run Detail Shape

```json
{
  "id": "loop_321",
  "task_id": "task_001",
  "source_run_id": "run_123",
  "status": "running",
  "execution_policy": {
    "mode": "parallel",
    "max_parallelism": 4,
    "selection": "priority"
  },
  "current_subtask_id": "subtask_001",
  "progress": {
    "total": 12,
    "completed": 3,
    "failed": 1
  },
  "error": null,
  "started_at": "2026-03-21T14:08:00Z",
  "updated_at": "2026-03-21T14:10:00Z",
  "completed_at": null
}
```

### Subtask Summary Shape

```json
{
  "id": "subtask_001",
  "title": "Implement repository sync guard",
  "status": "running",
  "updated_at": "2026-03-21T14:09:00Z"
}
```

## 14. Watch And CLI Mirror

### Watch Endpoints

- `POST /api/v1/agents/{id}/messages/stream`
- `GET /api/v1/runs/{id}/events`
- `GET /api/v1/dispatches/{id}/events`
- `GET /api/v1/hitl-requests/stream`
- `GET /api/v1/looper-runs/{id}/events`

### CLI Mirror

The CLI mirrors the same public model:

- `compozy agents list|get|create|update|delete|fork|validate|compile`
- `compozy agents runtime start|stop|restart|mode|get`
- `compozy agents sessions list|create|get|activate|reset|compact`
- `compozy agents message`
- `compozy agents message --dry-run`

- `compozy workflows list|get|create|update|delete|fork|validate|compile`
- `compozy workflows runtime get`
- `compozy workflows run`
- `compozy workflows run --dry-run`
- `compozy workflows runs list`

- `compozy triggers list|get|create|update|delete|fork|validate|compile|test`
- `compozy triggers runtime get`
- `compozy triggers enable|disable`
- `compozy events emit`
- `compozy events emit --dry-run`

- `compozy schedules list|get|create|update|delete|fork|validate`
- `compozy schedules runtime get`
- `compozy schedules enable|disable|run-now`
- `compozy schedules run-now --dry-run`

- `compozy packs list|get|objects|install|upgrade|uninstall|fork`
- `compozy packs upgrade --dry-run`

- `compozy tasks list|get|create|update|delete|replan`
- `compozy tasks subtasks list|create`
- `compozy tasks artifacts|docs|files`
- `compozy subtasks list|get|update|delete`

- `compozy runs list|get|signal|pause|resume|cancel|checkpoints`
- `compozy runs watch`
- `compozy dispatches list|get|children|retry|cancel`
- `compozy dispatches watch`
- `compozy hitl list|get|answer|cancel`
- `compozy hitl watch`
- `compozy looper-runs list|get|create|subtasks|pause|resume|cancel`
- `compozy looper-runs watch`

- `compozy skills list|get`

- `compozy artifacts list|get|versions`
- `compozy docs list|get|versions`

The CLI should prefer structured output modes such as JSON whenever possible,
because internal agents are expected to use it as a machine-friendly control
surface.

## 15. Skills

Skills are file-backed under `~/.compozy/skills/` and loaded at boot. Read-only through the API.

### Endpoints

| Method | Path | Summary |
|--------|------|---------|
| GET | `/api/v1/skills` | List loaded skills (paginated) |
| GET | `/api/v1/skills/{id}` | Skill detail |

### List Filters

- `q`

### List Response

List response follows `{ items, next_cursor }` convention. Each item includes:

```json
{
  "id": "writing",
  "name": "Writing",
  "description": "Skill for structured document writing",
  "source": "~/.compozy/skills/writing.toml",
  "loaded_at": "2026-03-21T12:00:00Z"
}
```

## 16. Standalone Artifact And Doc Endpoints

Top-level read-only access to artifacts and documents, not scoped to a specific task.

### Endpoints

| Method | Path | Summary |
|--------|------|---------|
| GET | `/api/v1/artifacts` | List all artifacts (paginated, filterable by `artifact_type`, `task_id`) |
| GET | `/api/v1/artifacts/{id}` | Artifact detail with current version |
| GET | `/api/v1/artifacts/{id}/versions` | Version history for an artifact |
| GET | `/api/v1/docs` | List all documents (paginated, filterable by `doc_type`, `task_id`) |
| GET | `/api/v1/docs/{id}` | Document detail with current version |
| GET | `/api/v1/docs/{id}/versions` | Version history for a document |

All responses follow `{ items, next_cursor }` pagination convention.

### Artifact List Filters

- `artifact_type`
- `task_id`
- `q`

### Doc List Filters

- `doc_type`
- `task_id`
- `q`

### Artifact Detail Shape

```json
{
  "id": "artifact_001",
  "task_id": "task_001",
  "type": "prd",
  "current_version_id": "artifact_v3",
  "created_at": "2026-03-21T14:00:00Z",
  "updated_at": "2026-03-21T14:05:00Z"
}
```

### Artifact Version Shape

```json
{
  "id": "artifact_v3",
  "artifact_id": "artifact_001",
  "version_number": 3,
  "created_at": "2026-03-21T14:05:00Z"
}
```

### Doc Detail Shape

```json
{
  "id": "doc_001",
  "task_id": "task_001",
  "type": "brief",
  "current_version_id": "doc_v2",
  "created_at": "2026-03-21T14:00:00Z",
  "updated_at": "2026-03-21T14:03:00Z"
}
```

### Doc Version Shape

```json
{
  "id": "doc_v2",
  "doc_id": "doc_001",
  "version_number": 2,
  "created_at": "2026-03-21T14:03:00Z"
}
```
