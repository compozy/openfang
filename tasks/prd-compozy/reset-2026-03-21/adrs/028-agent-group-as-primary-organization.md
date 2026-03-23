# ADR-028: Agent Group As Primary Organization

**Status:** Accepted
**Date:** 2026-03-21

## Decision

The public Compozy `agent_definition` should include an optional `group` field.

`group` is the primary user-facing organization field for agents. It exists in
addition to `tags`, not instead of `tags`.

## Rationale

- As agent inventories grow, `tags` alone are not a strong primary navigation
  mechanism.
- The product needs a stable top-level categorization field for lists,
  sidebars, filters, and sections in UI and API responses.
- A dedicated `group` field is simpler and clearer than inferring one category
  from tags.

## Consequences

- `group` should be treated as product metadata, not as a runtime or
  permission-related field.
- `tags` remain available for free-form secondary classification and search.
- `group` should not be overloaded to represent pack origin, tenant identity,
  workflow ownership, or technical namespaces.
- CLI, API, and UI can use `group` for primary organization while continuing to
  use `tags` for filtering and discovery.
