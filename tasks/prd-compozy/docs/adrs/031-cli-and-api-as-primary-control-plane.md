# ADR-031: CLI And API As Primary Control Plane

**Status:** Accepted
**Date:** 2026-03-21

## Decision

CLI and API are the primary control plane of Compozy.

UI is a later client of that control plane, not the place where core product
capabilities first appear.

The control plane must be broad enough for:

- direct human administration
- script and automation use
- internal agentic administration through the same public contracts

Each major surface should eventually expose:

- definition management
- operational control
- inspection and validation

## Rationale

- The product will likely ship a strong CLI and API before a rich UI.
- Internal agents should be able to create, inspect, and manipulate the system
  through the same public contracts instead of hidden internal pathways.
- A weak or UI-dependent control plane would make the product harder to
  automate and would undercut the goal of a living, self-manageable system.

## Consequences

- Public API and CLI design should optimize for both humans and machines.
- The spec should describe complete target surfaces, not only a reduced first
  implementation slice.
- Validation, inspection, and operational endpoints are part of the product
  contract, not just convenience features.
- UI planning should assume it is consuming an already-capable public control
  plane.
- The canonical public payload and command contract should be written down in a
  dedicated spec instead of remaining implicit in scattered notes.
