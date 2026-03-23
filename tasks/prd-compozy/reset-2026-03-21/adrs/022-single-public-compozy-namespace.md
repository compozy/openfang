# ADR-022: Single Public Compozy Namespace

**Status:** Accepted
**Date:** 2026-03-21

## Decision

The fork exposes a single public product namespace: `Compozy`.

That means:

- one public config root: `~/.compozy`
- one public CLI: `compozy`
- one public HTTP namespace: `/api/v1`

The fork does not expose parallel public OpenFang-branded product surfaces such as `~/.openfang` or a competing public API namespace.

## Rationale

- The product is a new forked product, not a backward-compatible public continuation of OpenFang.
- Dual public identities would create needless confusion in configuration, documentation, and UX.
- Reusing OpenFang internally does not require preserving OpenFang as a public product brand or namespace.

## Consequences

- Documentation, examples, and UI should refer only to Compozy public paths and routes.
- Any OpenFang compatibility that remains should be internal, migratory, or import-oriented.
- Public API design should stay simple and centered on `/api/v1`.
- Public CLI design should stay simple and centered on `compozy` as the main
  control-surface companion to `/api/v1`.
