# ADR-042: Lightweight Definition Contract Schema

**Status:** Accepted
**Date:** 2026-03-21

## Decision

Compozy definition contracts should use one shared lightweight native contract
language instead of full JSON Schema.

This shared contract language applies to:

- `agent_definition.input`
- `agent_definition.output`
- `workflow_definition.input`
- `workflow_definition.output`

The canonical contract node shape is intentionally small:

- `kind`
- `description`
- `nullable`

Object contracts additionally support:

- `fields`
- `required`
- `open`

Array contracts additionally support:

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

Kind-specific metadata may be attached where needed, for example
`artifact_type` or `doc_type`.

Normalization may accept a few convenience aliases, but the canonical logical
form remains small:

- `text` normalizes to `string`
- `json` normalizes to `any`

Object contracts default to `open = false`.

## Rationale

- Full JSON Schema would import far more complexity than Compozy needs for
  config-first authoring.
- OpenFang already has to normalize JSON Schema aggressively for tool/provider
  compatibility, which is a warning sign against making full JSON Schema the
  primary contract language for workflow and agent definitions.
- A small shared contract language is easier to read in TOML, easier to emit in
  JSON, and easier to validate deterministically.
- The product still needs typed contracts strong enough for machine-facing CLI
  and API usage, especially when internal agents will administer the system.

## Consequences

- Definition contracts stay readable and bounded.
- Validation and normalization are simpler than they would be with full JSON
  Schema support.
- If JSON Schema is needed for external integration, it should be generated as a
  derived projection during compile or export rather than accepted as the
  canonical authoring format.
- Workflow definitions gain an explicit distinction between `input` and
  `output` as contracts, while `outputs` remains the projection of runtime
  symbols into the final output value.
