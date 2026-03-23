# Compozy Storage Model

**Status:** Current storage baseline
**Date:** 2026-03-21

This document defines where Compozy stores authoritative state.

## 1. Source Of Truth By Category

Compozy uses three persistence layers with different ownership boundaries:

- file-backed definitions under `~/.compozy/`
- `runtime.db` for platform-core runtime state
- `compozy.db` for Compozy domain and durable workflow state

## 2. File-Backed Definitions

The source of truth for product definitions is the filesystem under
`~/.compozy/`.

That includes:

- `agents/`
- `workflows/`
- `triggers/`
- `schedules/`
- `skills/`
- `packs/`
- `templates/`

API and CLI mutations of these resources should update their file-backed
definitions rather than treating either database as the authoritative store for
definition objects.

Pack-managed definitions also remain file-backed, but under managed pack
content in `packs/`.

The storage rule for packs is:

- managed pack content lives under `~/.compozy/packs/`
- top-level definition directories contain user-owned definitions
- explicit same-ID forks are stored in the top-level definition directories and
  shadow managed pack objects
- managed pack content may be rewritten by install, upgrade, or uninstall
  operations
- user-owned fork files must not be overwritten by pack upgrades

This preserves the config-first model and keeps product authoring portable,
inspectable, and diffable.

## 3. `runtime.db`

`runtime.db` owns platform-core operational state that belongs to the OpenFang
side of the fork.

Representative categories:

- agent runtime state
- agent session state
- agent message history for direct agent use
- schedule runtime state
- scheduler execution metadata
- channel and network runtime state
- provider/runtime operational projections that belong to the platform core

Representative objects:

- `agent_runtime`
- `agent_session`
- `agent_message`
- `schedule_runtime`
- scheduler execution receipts or similar runtime metadata

The rule is:

- if the state belongs primarily to the platform runtime and not to Compozy
  product semantics, it belongs in `runtime.db`

## 4. `compozy.db`

`compozy.db` owns Compozy product-domain state and durable workflow execution
state.

Representative workflow execution objects:

- `workflow_run`
- `workflow_checkpoint`
- `workflow_signal`
- `agent_dispatch`
- `hitl_request`
- `looper_run`

Representative product-domain objects:

- `artifact`
- `artifact_version`
- `doc`
- `doc_version`
- `task`
- `subtask`
- task and looper progress state that belongs to product execution semantics

The rule is:

- if losing the state would break product continuity, workflow durability, or
  domain correctness, it belongs in `compozy.db`

## 5. Definitions Versus Runtime Records

Definitions should not be duplicated as a second authoritative database model.

That means:

- `agent_definition` is file-backed
- `workflow_definition` is file-backed
- `trigger_definition` is file-backed
- `schedule_definition` is file-backed

The databases may store:

- indexes
- caches
- compiled projections
- load/runtime metadata

but not a competing source of truth for the same user-authored definition.

## 6. Cross-Boundary Rules

- no cross-database SQL joins
- resource relationships are resolved in application code
- file-backed definitions are referenced by stable IDs
- runtime objects and domain objects may reference the same definition IDs, but
  their ownership boundary remains unchanged

## 7. Write Path Rules

For config-first resources:

1. validate the submitted definition
2. normalize it
3. write the canonical file-backed representation
4. reload or reindex the runtime as needed

For runtime and domain resources:

- write directly to the owning database

This keeps authoring flows and execution flows separate.

## 8. Recovery Rules

- `runtime.db` should allow the platform core to recover agent/session/runtime
  continuity that matters to direct agent operation
- `compozy.db` should allow durable workflow recovery, dispatch recovery, HITL
  recovery, and domain continuity
- file-backed definitions should remain readable and authoritative even if both
  databases need repair or regeneration
