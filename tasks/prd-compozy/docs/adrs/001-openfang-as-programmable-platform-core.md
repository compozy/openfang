# ADR-001: OpenFang As Programmable Platform Core

**Status:** Accepted
**Date:** 2026-03-21

## Decision

Compozy continues as a fork of OpenFang. OpenFang remains the platform core for agents, skills, triggers, schedulers, generic workflows, channels, networking, and runtime execution.

## Rationale

- The product goal is broader than SDLC automation. Users should be able to create their own agents and workflows.
- OpenFang already provides a strong programmable substrate for that model.
- Rebuilding those platform features from zero would spend effort on areas that already align with the desired product shape.

## Consequences

- The fork must invest in hardening and extensibility, not in replacing the platform core wholesale.
- OpenFang is not treated as a temporary executor. It remains central to the product architecture.
