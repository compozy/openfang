# ADR-023: Public API Exposure Rules

**Status:** Accepted
**Date:** 2026-03-21

## Decision

The public API under `/api/v1` follows three exposure rules:

1. Expose almost directly what already has the right shape.
2. Put an adapter in front of reusable internals whose current public contract is wrong or lossy.
3. Introduce new public resources where the product needs concepts that OpenFang does not expose correctly today.

Applied to the current fork:

- `agents` should use a Compozy-owned definition-first contract with explicit
  operational sub-resources
- `workflows` should use a Compozy-owned definition, validation, and execution
  contract
- `triggers` should use a Compozy-owned definition, validation, and
  event-automation contract
- `schedules` should map to the typed cron scheduler model almost 1:1
- `tasks` and `subtasks` should use Compozy-owned product-domain contracts
- `runs`, `dispatches`, `hitl-requests`, and `looper-runs` should be new public resources

The fork should not promote these old public contracts as product surfaces:

- the ad hoc legacy `schedules` CRUD
- `approvals` as if it were product HITL
- the current workflow CRUD surface
- the current agent-centric trigger surface

## Rationale

- Some OpenFang surfaces are already strong enough to reuse.
- Others are semantically wrong, overly lossy, or too coupled to older assumptions.
- Public API contracts become long-lived commitments, so the fork should not publish the wrong ones just because they already exist.
- The API is part of the primary control plane, so it must be usable by both
  humans and machines, including internal agents.

## Consequences

- `CronScheduler` becomes the main scheduling base for `/api/v1/schedules`.
- Agents keep reusable OpenFang internals but gain a cleaner public product
  contract.
- Workflow and trigger internals are reused, but their public contracts are redesigned.
- New execution resources become first-class product concepts instead of hidden runtime details.
- Validation, inspection, and operational surfaces should be treated as normal
  parts of the public API when the product model needs them.
- Exact payload conventions should be defined centrally and reused across these
  surfaces.
