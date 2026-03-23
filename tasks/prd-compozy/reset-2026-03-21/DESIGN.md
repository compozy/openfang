# Compozy Reset Architecture

**Status:** Current baseline
**Date:** 2026-03-21

## 1. Summary

Compozy should continue as a fork of OpenFang, but with a different architectural stance than the earlier planning assumed.

OpenFang remains the programmable platform core:

- agents
- skills
- triggers
- schedulers
- generic workflows
- channels and network
- agent-to-agent delegation
- runtime and tool execution

Compozy becomes the product and domain layer on top:

- SDLC primitives
- tasks, subtasks, artifacts, docs, HITL
- separate `runtime.db` and `compozy.db`
- config-first agent and workflow definitions
- Compozy-owned workflow and trigger surfaces
- a single public Compozy namespace
- bundled first-party workflow packages, especially SDLC
- a durable workflow runtime in the OpenFang fork

Arky crates live inside the workspace and provide the deep, typed provider layer for Claude Code and Codex.

## 2. Product Shape

The product is:

- open source
- single-user
- local-first
- autonomous by default
- configurable by TOML, CLI, API, and later UI

The product is not trying to recreate the current Compozy product one-for-one. Billing, subscriptions, auth, and organizations are out of scope.

### Control Plane Primacy

CLI and API are the primary control plane of the product.

The UI is a later client of that control plane, not the place where core
capabilities first appear.

This means the public control plane must be broad enough for:

- direct human administration
- shell and script automation
- internal agentic administration through the same public contracts

The product should therefore publish complete public surfaces for:

- definition management
- operational control
- inspection and validation

The canonical payload and command contract for that control plane lives in
[API-SPEC.md](API-SPEC.md).

The storage and table ownership model lives in
[STORAGE-MODEL.md](STORAGE-MODEL.md) and [DATABASE-SCHEMA.md](DATABASE-SCHEMA.md).

The phased delivery order for the durable runtime lives in
[IMPLEMENTATION-PLAN.md](IMPLEMENTATION-PLAN.md).

## 3. Why Fork OpenFang

The deciding factor is not just execution. It is programmability.

The user should be able to define:

- new agents
- new workflows
- new skills
- new triggers
- new schedules
- multi-agent compositions

OpenFang already has a strong base for this model. Rebuilding all of that from scratch would spend effort on infrastructure that is already aligned with the product direction.

What does need to change in the fork is not the existence of those systems, but their durability and extensibility in the places where the current OpenFang implementation is still too shallow for Compozy's use case.

## 4. System Model

```text
TOML / CLI / API / UI
      |
      v
+---------------------------+
|     Compozy Product       |
| agents, workflows, runs,  |
| tasks, subtasks, HITL,    |
| SDLC packs, UX            |
+-------------+-------------+
              |
              v
+---------------------------+
|    OpenFang Fork Core     |
| agents, skills, tools,    |
| triggers, schedulers,     |
| generic workflows,        |
| channels, OFP/A2A         |
+-------------+-------------+
              |
              v
+---------------------------+
|      Runtime Layer        |
| OpenFang runtime +        |
| Arky crates (Claude Code, |
| Codex)                    |
+---------------------------+

Data:
- runtime data under `~/.compozy/data/`
- `runtime.db` for platform-core runtime state
- `compozy.db` for Compozy domain and durable workflow state
- file-backed definitions under `~/.compozy/` remain the source of truth for
  agents, workflows, triggers, schedules, skills, packs, and templates
```

## 5. One Workflow Center

The current baseline keeps one workflow center inside the OpenFang fork.

- user-defined workflows stay generic and flexible
- the OpenFang workflow runtime is hardened to support durable runs
- Compozy does not introduce a second primary orchestration engine
- new product-facing workflow and trigger surfaces do not create a second engine
- the SDLC flow becomes a first-party workflow package on the same platform

Restate was considered because it solves durable execution well, but it would create a second orchestration center too early. For the current direction, the cleaner move is to harden the OpenFang workflow runtime rather than split the product into two workflow systems.

## 6. Config-First Surface

Agents and workflows should be created by configuration first, not by Rust first.

Rust remains necessary for:

- new primitives
- new providers
- new integrations
- changes to runtime internals

Most product growth should happen through:

- agent definitions
- workflow definitions
- skill packages
- triggers
- schedules

Recommended user-facing layout:

```text
~/.compozy/
  config.toml
  agents/
  workflows/
  skills/
  triggers/
  schedules/
  packs/
  templates/
  logs/
  data/
```

The same logical objects should be visible through TOML, CLI, API, and later
UI.

CLI and API should be first-class authoring and administration surfaces, not
just helpers behind the UI.

The canonical authoring format for file-backed definitions should be TOML.

That does **not** mean the runtime executes TOML directly.

The authoring and execution path should be:

1. parse TOML or the equivalent JSON model from CLI/API
2. validate the definition
3. normalize the definition
4. compile it into an internal IR
5. execute only the IR

This keeps:

- TOML as the source-friendly config format
- JSON as the API transport format for the same logical model
- IR as the execution format
- files as the source of truth for definitions

Definitions remain file-backed even when they are created or updated through
CLI and API.

The storage boundary is:

- files for definitions
- `runtime.db` for platform runtime state
- `compozy.db` for durable workflow and product-domain state

The detailed storage split lives in [STORAGE-MODEL.md](STORAGE-MODEL.md).

This does **not** mean Compozy should redefine every schema from scratch.

- `AgentManifest` remains the base definition model for agents in the fork.
- OpenFang `skills` remain the base skill system.
- OpenFang `cron/schedules` remain the base scheduling system.
- Compozy adds product UX, packaging, defaults, and extensions where needed.
- The main schema and runtime refactor pressure is on `workflows` and `triggers`, not on everything at once.
- Workflow and trigger product surfaces can diverge from the current OpenFang public contracts when the old shapes block the new model.
- The public product should expose one namespace only: `Compozy`.

### Agent Definition Shape

`AgentManifest` remains the internal base model for agents, but the public
Compozy `agent_definition` should be more product-oriented and should compile
into `AgentManifest` plus provider/runtime metadata.

The public agent surface should also distinguish between:

- primary organization for navigation
- secondary classification for search and filtering

That means agent metadata should include both:

- `group` as the primary, stable user-facing category
- `tags` as free-form secondary metadata

`group` exists to keep large agent inventories navigable in UI, API, and file
layouts. It should not be overloaded to mean:

- pack origin
- permission scope
- tenant or organization
- workflow ownership
- technical namespace

It is a product-facing organizational field.

Examples:

- `group = "sdlc"`
- `group = "research"`
- `group = "prompting"`

`tags` remain useful for cross-cutting labels such as `docs`, `review`,
`planning`, or `prd`.

The most important adjustment is the provider block.

Compozy should not model providers as only:

- `driver`
- `model`
- `profile`

and should also not collapse provider config into one untyped JSON bag.

The public agent schema should separate provider concerns into:

- `provider.driver`
- `provider.model`
- `provider.profile`
- `provider.defaults`
- `provider.config`
- optional `provider.request_extra`

`provider.defaults` is for request-level settings whose semantics are portable
enough across providers, for example:

- `max_tokens`
- `reasoning_effort`

`provider.config` is a typed, namespaced block whose shape depends on the
selected provider driver.

Examples:

- `provider.config` for `codex` can carry fields such as `web_search`,
  `include_plan_tool`, `resume_last`, `sandbox_mode`, `sandbox_network_access`,
  `rmcp_client`, `reasoning_summary`, and `model_verbosity`
- `provider.config` for `claude_code` can carry fields such as
  `continue_conversation`, `fork_session`, `allowed_tools`,
  `disallowed_tools`, `additional_directories`, `max_budget_usd`,
  `fallback_model`, and `mcp_servers`
- wrapper-style Claude-compatible providers can expose wrapper-specific fields
  in `provider.config` and shared Claude settings in `provider.config.base`

The design rule is:

- keep cross-provider request defaults small
- expose provider-specific settings in typed, namespaced blocks
- keep low-level install/runtime plumbing out of normal agent documents

Provider configuration should be split into three layers:

- installation or workspace configuration
- named provider profiles
- per-agent provider configuration

The classification rule is:

- agent-level config changes how a specific agent behaves
- installation/workspace config changes how a provider is installed,
  authenticated, connected, or executed on this machine
- profiles are reusable behavior presets that sit between those two layers

Examples of install-level or workspace-level settings that should not normally
live in every `agent_definition`:

- provider binary path
- raw environment maps
- shared app-server keys
- low-level timeout tuning
- client identity/version plumbing

Additional installation or workspace concerns include:

- credential wiring
- transport setup
- app-server bootstrap and lifecycle plumbing
- cache and runtime directories
- region or account defaults that belong to the whole installation

Examples of fields that are appropriate at agent level:

- `provider.driver`
- `provider.model`
- `provider.profile`
- `provider.defaults.max_tokens`
- `provider.defaults.reasoning_effort`
- behavior-level provider flags such as allowed tools, conversation or session
  behavior, fallback model choice, and agent-local budget caps

Profiles should be the reusable middle layer.

Examples:

- `default`
- `fast-research`
- `safe-doc-writer`

Profiles may contain:

- default model selection
- portable request defaults
- typed provider config that is safe to reuse across many agents

`provider.request_extra` may exist, but only as a constrained escape hatch.

It should be:

- optional
- request-level only
- clearly advanced or experimental
- bounded to small JSON-like request overrides

It should **not** be used for:

- credentials
- raw environment variables
- provider bootstrap
- transport plumbing
- process lifecycle tuning
- any other installation or workspace infrastructure concerns

### Complete Agent Schema

The public `agent_definition` should use these top-level fields:

- `id`
- `name`
- `version`
- `description`
- `enabled`
- `group`
- `tags`

And these main blocks:

- `provider`
- `prompt`
- `capabilities`
- `runtime`
- `input`
- `output`

The public agent surface should not embed `triggers` or `schedules`.
Those remain separate system surfaces.

`prompt` should stay simple and explicit:

- `system`
- `instructions`
- `skills`

`capabilities` should describe the operational envelope of the agent, not an
interactive permission system. Expected fields include:

- `tools`
- `primitives`
- `delegation`
- `workspace`
- `network`

`runtime` should describe the execution behavior of the agent and should stay
small:

- `autonomous`
- `memory_policy`
- `hitl`

`input` and `output` are product contracts, not raw OpenFang runtime fields.
They should use one shared lightweight contract language across agents and
workflows.

### Shared Definition Contract Schema

Compozy should use one small native contract shape for:

- `agent_definition.input`
- `agent_definition.output`
- `workflow_definition.input`
- `workflow_definition.output`

The contract should be inspired by JSON Schema, but it should **not** adopt
full JSON Schema as the canonical authoring format.

Each contract node should support:

- `kind`
- `description`
- `nullable`

Object contracts should additionally support:

- `fields`
- `required`
- `open`

Array contracts should additionally support:

- `items`

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

Kind-specific metadata may appear where needed, for example:

- `artifact_type` for `artifact_ref`
- `doc_type` for `doc_ref`

Normalization may accept a few convenience aliases, but the canonical logical
form should stay small. For example:

- `text` normalizes to `string`
- `json` normalizes to `any`

Object contracts should default to `open = false`. If a definition needs an
open-ended bag of JSON values, it should say so explicitly with either:

- `kind = "any"`
- or `kind = "object"` plus `open = true`

The canonical contract language should **not** support full JSON Schema
features such as:

- `$ref`
- `$defs`
- `oneOf`
- `anyOf`
- `patternProperties`
- provider-specific schema extensions

If a JSON Schema representation is useful for tool interop or external
integration, it should be generated as a derived projection during compile or
export. It should not be the primary authoring language for definition
contracts.

### Definition Validation And Normalization

Compozy should use bounded layered validation instead of either:

- permissive parse-only acceptance
- or an overengineered validator that tries to execute the whole system

The validation pipeline should be:

1. schema validation
2. reference validation
3. semantic validation
4. normalization

Meaning:

- schema validation checks shape, field types, required fields, and enums
- reference validation checks named references such as agents, workflows,
  primitives, skills, providers, and reserved identifiers
- semantic validation checks cross-field compatibility and per-kind rules
- normalization fills defaults, canonicalizes equivalent forms, and produces a
  stable logical representation for compile

The design rule is:

- validate early
- normalize deterministically
- compile after validation
- do not execute logic during validation

Boundaries that keep this from becoming a seven-headed monster:

- no provider boot or network calls during validation
- no template execution
- no whole-system dry-run hidden inside `validate`
- no attempt to prove all runtime behavior statically
- no deep dynamic analysis beyond obvious cycles and impossible references

For `agent_definition`, validation should at least cover:

- required identity and provider fields
- known provider driver
- provider-profile compatibility where profile names are used
- typed validation of `provider.config` for the selected driver
- bounded validation of `provider.request_extra` as request-level only
- `output.kind` compatibility with output-specific fields
- legal values for `delegation`, `workspace`, `memory_policy`, and `hitl`

For `workflow_definition`, validation should at least cover:

- unique step IDs
- legal `kind`, `uses`, `flow`, and `runtime` combinations
- references to existing agents, workflows, and primitives
- binding references to known symbols such as `input`, `vars`, and
  `steps.<id>.output`
- `save_as` and `outputs` references that can be resolved structurally
- mode-specific requirements such as `flow.when` for conditional steps and
  loop termination rules for loop steps

Validation should allow structural reasoning about bindings without trying to
materialize real values.

### Internal Compilation Model

The public agent schema should compile into three internal layers:

- `AgentManifest`
- `ProviderBinding`
- `AgentProductMetadata`

`AgentManifest` carries the parts that already belong to the OpenFang platform
model.

`ProviderBinding` carries:

- provider identity
- request defaults
- provider-specific typed config

`AgentProductMetadata` carries:

- `version`
- `enabled`
- `group`
- `tags`
- `input`
- `output`

This keeps the public product contract clean without forcing Compozy to invent
a second agent runtime model.

### Agent API Surface

The target public API for agents should be complete in the product spec, even
if implementation is phased.

The canonical resource remains the agent definition:

- `GET /api/v1/agents`
- `POST /api/v1/agents`
- `POST /api/v1/agents/validate`
- `POST /api/v1/agents/compile`
- `GET /api/v1/agents/{id}`
- `PUT /api/v1/agents/{id}`
- `DELETE /api/v1/agents/{id}`
- `GET /api/v1/agents/{id}/compiled`

But the product should also expose operational sub-resources for direct agent
use, because agents are part of the programmable platform, not only passive
definitions.

Recommended operational surface:

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

Design rule:

- `/api/v1/agents` answers who the agent is
- operational sub-resources answer what the loaded agent is doing

This preserves a clean definition-first resource model without hiding the
OpenFang-style direct agent operations that remain part of the product.

The matching CLI surface should mirror this model under `compozy agents ...`,
because CLI and API are the primary control plane for both humans and internal
agents.

Exact request and response payloads live in [API-SPEC.md](API-SPEC.md).

### Workflow API Surface

The target public API for workflows should also be complete in the product
spec, even if implementation is phased.

Definition and validation surface:

- `GET /api/v1/workflows`
- `POST /api/v1/workflows`
- `POST /api/v1/workflows/validate`
- `POST /api/v1/workflows/compile`
- `GET /api/v1/workflows/{id}`
- `PUT /api/v1/workflows/{id}`
- `DELETE /api/v1/workflows/{id}`
- `GET /api/v1/workflows/{id}/compiled`

Operational and inspection surface:

- `GET /api/v1/workflows/{id}/runtime`
- `POST /api/v1/workflows/{id}/runs`
- `GET /api/v1/workflows/{id}/runs`

Design rule:

- `/api/v1/workflows` answers what the workflow definition is
- workflow sub-resources answer how that definition compiles and executes

The matching CLI surface should mirror this model under
`compozy workflows ...`.

Exact request and response payloads live in [API-SPEC.md](API-SPEC.md).

### Trigger API Surface

The target public API for triggers should be complete in the product spec, even
if implementation is phased.

Definition and validation surface:

- `GET /api/v1/triggers`
- `POST /api/v1/triggers`
- `POST /api/v1/triggers/validate`
- `POST /api/v1/triggers/compile`
- `GET /api/v1/triggers/{id}`
- `PUT /api/v1/triggers/{id}`
- `DELETE /api/v1/triggers/{id}`
- `GET /api/v1/triggers/{id}/compiled`

Operational and inspection surface:

- `GET /api/v1/triggers/{id}/runtime`
- `POST /api/v1/triggers/{id}/enable`
- `POST /api/v1/triggers/{id}/disable`
- `POST /api/v1/triggers/{id}/test`

Triggers also imply a shared public event ingress for the control plane:

- `POST /api/v1/events`

Design rule:

- `/api/v1/triggers` answers what event-driven automation is defined
- trigger sub-resources answer whether it is enabled, how it resolves, and how
  it behaves against sample events
- `/api/v1/events` injects real events into the system event pipeline

The matching CLI surface should mirror this model under
`compozy triggers ...` and `compozy events ...`.

Exact request and response payloads live in [API-SPEC.md](API-SPEC.md).

### Schedule API Surface

The public schedule surface should stay close to the typed OpenFang cron model
instead of inventing a second scheduling DSL.

Definition and validation surface:

- `GET /api/v1/schedules`
- `POST /api/v1/schedules`
- `POST /api/v1/schedules/validate`
- `GET /api/v1/schedules/{id}`
- `PUT /api/v1/schedules/{id}`
- `DELETE /api/v1/schedules/{id}`

Operational and inspection surface:

- `GET /api/v1/schedules/{id}/runtime`
- `POST /api/v1/schedules/{id}/enable`
- `POST /api/v1/schedules/{id}/disable`
- `POST /api/v1/schedules/{id}/run-now`

Design rule:

- `schedule` and `delivery` should stay close to the typed OpenFang cron model
- schedule action kinds should stay close to the OpenFang action model
- schedule action payloads should align with the rest of the Compozy control
  plane

The matching CLI surface should mirror this model under
`compozy schedules ...`.

Exact request and response payloads live in [API-SPEC.md](API-SPEC.md).

## 7. Durable Workflow Runtime

The OpenFang workflow runtime should be refactored from an in-memory run model to a durable run model.

First durable-cut objects:

- `workflow_run`
- `workflow_checkpoint`
- `agent_dispatch`
- `hitl_request`
- `workflow_signal`
- `looper_run`

Secondary objects can be added later when they prove necessary, especially:

- `workflow_step_run`
- richer derived run history or observability tables

The design rule is simple:

- if losing the state would break product continuity, it belongs in `compozy.db`
- if losing the state only interrupts a single execution loop and it can be retried or recreated, it can remain runtime-ephemeral

The current OpenFang workflow model is a good conceptual starting point, but the current OpenFang workflow file/API surfaces are not sufficient as the primary public contract for durable, restart-safe product workflows. The fork should evolve the model and expose a new Compozy-facing workflow surface rather than create a second workflow center.

## 8. Domain Primitives

Compozy adds reusable primitives instead of hiding all product logic in one hardcoded SDLC flow.

Foundational primitives:

- `artifact.*`
- `doc.*`
- `hitl.*`
- `task.*`
- `subtask.*`
- `agent.*`
- `capability.*`

These primitives are available to first-party and user-defined workflows alike. The SDLC package is just one composition built on top of them.

### Task And Subtask Domain

Compozy should treat `task` as the main durable work object of the product.

This replaces the old Compozy distinction where:

- `issue` was the root planning object
- and nested `tasks` represented executable work items

In the new model:

- the old `issue` concept becomes `task`
- the old nested `task` concept becomes `subtask`

`task` is the domain-level unit of work:

- objective
- planning context
- linked artifacts and docs
- linked file refs
- repository and label context when relevant
- durable identity across replanning

`subtask` is the executable unit of work inside a task:

- concrete instruction
- execution status
- assignee or executor target
- dependencies
- local execution input
- result

The old OpenFang shared task queue should not remain the canonical product
model. At most, it survives as a legacy adapter or runtime mechanism where that
is still useful.

Compozy should keep `task` and `subtask` as the public domain names.

The naming collision with the old OpenFang task queue should be resolved
internally through:

- separate modules and types
- separate tables and storage ownership
- explicit legacy adapters where needed

It should not be resolved by renaming the Compozy domain to less natural public
terms just to mirror legacy OpenFang internals.

`task` should anchor the durable working context of product execution.

That means task-level ownership for refs such as:

- artifacts
- docs
- files
- repositories
- labels

`subtask` should carry only local execution context and execution result.

`workflow_run` should remain the durable execution record, but it should not
become the main store of product work context when that context belongs to the
task itself.

## 9. Agent Delegation

OpenFang already has local agent-to-agent delegation and peer/network concepts. Compozy should keep that.

The key change is persistence:

- `agent.call`
- `agent.send`
- `agent.spawn`

must produce durable dispatch records when invoked inside important workflow runs. That preserves:

- lineage
- observability
- recovery after restart
- coordination between workflow state and agent execution

## 10. Autonomy and HITL

The product should remain autonomous by default.

- tools do not ask for permission by default
- workflows do not stop for interactive approval prompts
- skills do not create hidden permission gates
- HITL is the only intentional human pause, and only when explicitly modeled

This keeps the system aligned with OpenFang's spirit and with the stated product goal of real autonomy.

Capabilities in this baseline describe availability and environment, not interactive approval.

Examples:

- `git available`
- `worktree supported`
- `github configured`
- `network enabled`

## 11. Tool and Skill Model

The runtime should move away from a large hardcoded dispatcher and toward a registry-driven model.

Recommended layers:

- built-in tools
- provider-backed tools
- Compozy domain primitives
- user-installed skills and tools

Skills remain a packaging and behavior layer. They do not replace durable primitives.

OpenFang's skill system remains canonical in the fork:

- `skill.toml` and `SKILL.md` stay valid
- the existing runtimes stay valid
- the registry and conversion model stay valid

Compozy should extend the product experience around skills, not replace the skill model itself.

## 12. Looper

The looper is not the whole workflow system and not a parallel orchestration engine.

It is a specialized executor for iterative work over subtasks:

- select subtask
- execute
- observe result
- continue or replan

It should run on the same durable workflow foundation and reuse the same operational objects where possible.

The looper must not infer sequencing or concurrency implicitly.

Its execution policy should be explicit in the workflow or in the request that
starts it.

The minimum policy shape should include:

- `mode`
- `max_parallelism`
- `selection`

Recommended execution modes:

- `sequential`
- `parallel`

The looper policy defines the maximum concurrency envelope.

Subtasks may further restrict execution through fields such as:

- `depends_on`
- `parallelizable`

That means:

- the looper decides the allowed execution envelope
- subtasks may narrow it
- subtasks do not widen it beyond what the looper policy permits

## 13. Repo and Workspace Automation

Git, PR, and workspace automation remain optional delegated capabilities.

The product must work without forcing one repo model on every installation. Users can opt into repo automation and choose the workspace strategy that fits their environment.

## 14. Priorities

### Foundational

- durable workflow runtime
- config-first agents and workflows
- reusable domain primitives
- persisted agent delegation
- separate `runtime.db` and `compozy.db`
- explicit HITL model
- internal Arky crates for Claude Code and Codex
- real tool registry

### Important But Deferrable

- rich editing UI
- highly polished SDLC packs
- advanced repo automation
- deeper network UX
- broader Arky crate coverage beyond Claude Code and Codex

### Do Not Do Now

- a second workflow center
- a permission-prompt runtime
- a giant Compozy-specific DSL on day one
- a total provider layer rewrite
- a greenfield rebuild of all OpenFang platform systems

## 15. Canonical Versus Extended OpenFang Systems

The fork should distinguish between systems that remain canonical from OpenFang and systems that need stronger Compozy-driven evolution.

Keep canonical as the base:

- `AgentManifest`
- `SkillManifest` and the skill registry
- typed cron scheduler types and job semantics
- agent-to-agent delegation and OFP/A2A foundations

Extend in the fork:

- workflow runtime internals and workflow run durability
- trigger runtime internals and targeting beyond waking agents
- product packaging through workflow packs
- Compozy domain primitives and persistent run objects

Compozy-owned product surfaces:

- workflow definitions
- trigger definitions
- run, dispatch, HITL, and looper views and APIs

Public `schedules` should map to the typed scheduler model. The older ad hoc schedule CRUD should not be promoted as a product surface.

Avoid creating parallel schemas unless the OpenFang base model blocks a real requirement, but do not preserve old public contracts by default when they block the new product shape.

## 16. Packs, Built-Ins, And Safe Forking

Workflow packs are versioned distribution units for managed product
definitions.

They may contain:

- agents
- workflows
- triggers
- schedules
- templates
- supporting metadata

First-party packs, including SDLC, should follow semantic versioning.

Design rules:

- installations pin exact pack versions
- upgrades are explicit operations, not silent background behavior
- pack upgrades should support dry-run and explanation before mutation
- managed pack contents are read-only from the user's point of view

Built-ins should be treated as bundled first-party packs rather than as a
separate mutation model.

That means:

- built-ins still have pack identity and version
- built-ins are loaded through the pack system
- upgrading a bundled pack follows the same managed-pack rules

Pack-managed definitions should remain immutable in place.

Users and internal agents should customize them through explicit fork
operations instead of editing managed files directly.

The safe default is an explicit same-ID fork that creates a user-owned
override.

That override:

- lives in the normal top-level definition directories such as `agents/`,
  `workflows/`, `triggers/`, or `schedules/`
- shadows the managed pack definition for this installation
- records upstream provenance through `forked_from` metadata
- is not overwritten by later pack upgrades

The managed upstream object continues to exist underneath for comparison,
inspection, and future rebase or refresh workflows.

Normal definition creation should not accidentally collide with managed pack
IDs. Same-ID shadowing should happen only through an explicit fork operation.

This keeps two properties at once:

- pack upgrades stay predictable
- local customization remains powerful enough for a living, agent-managed
  system

Public product identity rules:

- one config root: `~/.compozy`
- one CLI: `compozy`
- one API namespace: `/api/v1`
- no user-facing `.openfang` product surface

## 17. Workflow v2

The fork keeps the OpenFang workflow model as the conceptual base, but evolves it into a durable `workflow v2`.

The goal is not to replace the model with a second DSL. The goal is to preserve the OpenFang mental model while fixing the gaps that matter for Compozy:

- durable runs
- richer step actions
- workflow-level signals
- compatibility with domain primitives
- compatibility with looper startup

The current control-flow ideas remain useful:

- `sequential`
- `fan_out`
- `conditional`
- `loop`

An explicit `collect` step remains useful after fan-out, but it should be treated as step action, not as one more control mode in the public contract.

The main structural addition is to separate:

- what a step does
- how it behaves in the flow

Recommended minimum step kinds:

- `agent`
- `primitive`
- `workflow`
- `wait_signal`
- `start_looper`
- `emit_event`
- `collect`
- `noop`

Recommended modes:

- `sequential`
- `fan_out`
- `conditional`
- `loop`

Definitions should be validated and compiled into an internal workflow IR before execution.

The validation layer should remain bounded:

- it should catch malformed workflows, bad references, and impossible symbol
  usage
- it should not attempt symbolic execution of the workflow
- it should not try to simulate provider behavior or external side effects

The public workflow contract of the fork does not need to preserve the current OpenFang workflow files, routes, or editor behavior. Those legacy surfaces can survive as import paths when useful, but they should not constrain the new workflow surface.

The implementation order should be runtime-first:

- first harden durable run objects and state transitions
- then enrich the public workflow schema and UX

## 18. HITL Inside Agent Steps

Compozy HITL is not only a workflow-level pause.

The product must support HITL inside an active agent step. Typical example:

- the PRD-writing agent asks clarification questions
- waits for one or more human answers
- resumes the same reasoning thread
- completes the same step

That means:

- `wait_signal` is useful, but it does not represent all HITL
- `agent` steps must be interruptible and resumable
- `hitl_request` must be linked to the active `agent_dispatch`
- a workflow step can remain `running` while an in-step HITL interaction is pending

The workflow runtime therefore needs two kinds of waiting:

- workflow-level waits
- in-step interaction waits inside an active dispatch

## 19. Trigger v2

The OpenFang trigger engine remains the base, but the target model widens beyond waking an agent with a prompt.

The fork should preserve the existing pattern-matching mental model and extend the target side with explicit dispatch targets such as:

- `agent_message`
- `workflow_start`
- `workflow_signal`

This keeps one event system while making it useful for durable workflow runs and product-level automations.

The trigger system should not replace in-step HITL. It should focus on system-level reactions:

- start a workflow
- signal a workflow
- wake an agent
- react to domain events

The public trigger contract of the fork should be Compozy-owned. The old OpenFang trigger routes and file shapes may remain available as import or legacy paths, but they should not define the new product surface.

## 20. Public API Exposure Rules

The public API under `/api/v1` should follow three exposure rules:

1. Expose almost directly what already has the right shape.
2. Put an adapter in front of reusable internals whose current public contract is wrong or lossy.
3. Introduce new public resources where the product needs concepts that OpenFang does not expose correctly today.

Applied to the current fork:

- `agents` should be exposed through a Compozy-owned definition-first surface
  with operational sub-resources
- `workflows` should be exposed through a Compozy-owned definition,
  validation, and execution surface
- `triggers` should be exposed through a Compozy-owned definition, validation,
  and event-automation surface
- `schedules` should map almost directly to the typed cron scheduler model
- `tasks` and `subtasks` should be exposed as Compozy-owned product-domain
  resources
- `runs`, `dispatches`, `hitl-requests`, and `looper-runs` should be new public resources

The product should not promote these old public contracts:

- the ad hoc legacy `schedules` CRUD
- `approvals` as if it were product HITL
- the current workflow CRUD surface
- the current agent-centric trigger surface

The public control plane should be broad enough for both:

- direct user administration through CLI and API
- internal agentic administration through those same contracts

## 21. Workflow v2 Public Schema

The public workflow contract should reflect the target product model, not a temporary subset of the current OpenFang route payload.

Top-level workflow fields:

- `id`
- `name`
- `version`
- `description`
- `enabled`
- `tags`
- `input`
- `output`
- `defaults`
- `steps`
- `outputs`

Step structure:

- `id`
- `name`
- `kind`
- `uses`
- `with`
- `save_as`
- `flow`
- `runtime`

Supported step kinds:

- `agent`
- `primitive`
- `workflow`
- `wait_signal`
- `start_looper`
- `emit_event`
- `collect`
- `noop`

Supported flow modes:

- `sequential`
- `fan_out`
- `conditional`
- `loop`

This contract should be planned as the target shape even if runtime support reaches it in phases.

`input` and `output` use the shared definition contract schema.

`outputs` remains a projection block that maps runtime symbols into the final
workflow result shape described by `output`.

## 22. Trigger v2 Public Schema

The public trigger contract should center on explicit matching and explicit targets.

Top-level trigger fields:

- `id`
- `name`
- `description`
- `enabled`
- `max_fires`
- `cooldown_secs`
- `match`
- `target`

Match fields remain intentionally simple:

- `event`
- `source`
- `contains`
- `filters`

Supported target kinds:

- `agent_message`
- `workflow_start`
- `workflow_signal`

`workflow_signal` must include an explicit selector for the destination run.

## 23. Runtime Execution Resource Surfaces

The public product should expose these execution resources:

- `runs`
- `dispatches`
- `hitl-requests`
- `looper-runs`

These are part of the primary control plane and should be designed for both
human operators and machine-driven administration.

And it should treat `workflow_signal` as a first-class durable execution object, even when it is not always promoted as a primary end-user resource.

Minimum semantic roles:

- `run` = durable execution of a workflow
- `dispatch` = delegated execution inside a run
- `hitl-request` = explicit human interaction tied to a run or dispatch
- `looper-run` = durable execution of the specialized looper

Complementary product-domain roles:

- `task` = durable product work root with stable identity and context
- `subtask` = executable child work unit inside a task

The canonical way to start looper execution should be creation of a
`looper-run` against an explicit `task_id`, not an implicit side effect hidden
inside task mutation.

The canonical request and response payloads for these resources live in
[API-SPEC.md](API-SPEC.md).

## 24. Watch And Subscription Policy

The public control plane should use explicit SSE sub-resources for live
operational state instead of a generic `watch=true` query parameter spread
across every endpoint.

Definition resources should remain request/response by default.

Live watch should exist where the resource is genuinely operational and
long-lived, especially:

- agent message streams
- workflow run event streams
- dispatch event streams
- HITL request streams
- looper run event streams

This keeps the API machine-friendly without turning every resource into an
ambiguous subscription surface.

## 25. Relationship To Earlier Material

This document supersedes the earlier mixed `_analysis` baseline as the main design reference.

Older files remain useful for:

- codebase review
- historical reasoning
- rejected or partial alternatives

They should not be treated as the current architecture unless this reset baseline points back to them explicitly.

## 26. New Product Surfaces Versus Legacy Surfaces

The fork distinguishes between reused platform systems and the public product surfaces exposed to users.

Primary Compozy-facing surfaces should exist for:

- workflows
- triggers
- runs
- dispatches
- HITL requests
- looper runs

These surfaces should be designed for the product that is actually being built, not for preserving old OpenFang route or file compatibility.

Legacy OpenFang workflow and trigger surfaces may still be kept internally for:

- implementation reference
- importer utilities
- transitional internal adapters

They are not public product surfaces.

## 27. Safe Delivery Order

The safest delivery order for the fork is:

1. durable workflow runtime objects and state transitions
2. persisted dispatch, signal, and HITL handling
3. Compozy-owned workflow and trigger surfaces
4. richer workflow authoring syntax and UX

This order preserves the approved product goals without overcommitting to fragile public schemas before the runtime is durable enough to justify them.
