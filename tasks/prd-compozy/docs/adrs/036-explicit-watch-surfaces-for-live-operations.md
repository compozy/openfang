# ADR-036: Explicit Watch Surfaces For Live Operations

**Status:** Accepted
**Date:** 2026-03-21

## Decision

The public control plane should use explicit SSE sub-resources for live
operational state instead of a generic watch flag spread across all resources.

The baseline watch surfaces are:

- agent message streaming
- run event streams
- dispatch event streams
- HITL request streams
- looper run event streams

Definition resources remain request/response by default.

## Rationale

- Internal agents need live operational state, but not every resource should
  become a subscription surface.
- Explicit watch endpoints are easier to reason about, document, and automate
  than a generic `watch=true` convention on unrelated resources.
- SSE is a good fit for the local-first, single-user control plane and matches
  the current agent streaming mental model.

## Consequences

- Watch capability becomes part of the public control plane where operations
  are long-lived or interactive.
- The API surface stays cleaner than a blanket subscription model.
- The exact watch endpoints and event names should follow `API-SPEC.md`.
- Replay should be bounded and follow reset-and-snapshot semantics rather than
  promising unbounded event history.
