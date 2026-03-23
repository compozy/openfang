# Compozy Reset Baseline

**Status:** Current architecture baseline
**Date:** 2026-03-21

This folder is the clean reset of the Compozy architecture documentation after the OpenFang review. It replaces the earlier mixed planning set as the main reference point.

## Start Here

- [DESIGN.md](DESIGN.md)
- [API-SPEC.md](API-SPEC.md)
- [STORAGE-MODEL.md](STORAGE-MODEL.md)
- [DATABASE-SCHEMA.md](DATABASE-SCHEMA.md)
- [IMPLEMENTATION-PLAN.md](IMPLEMENTATION-PLAN.md)
- [INITIAL-RUNTIME-MIGRATIONS.md](INITIAL-RUNTIME-MIGRATIONS.md)
- [OPEN-QUESTIONS.md](OPEN-QUESTIONS.md)
- [adrs/](adrs/)

## What This Folder Tries To Do

- capture only the decisions that were explicitly validated
- keep OpenFang as the platform core
- define how Compozy adds product domain, persistence, and workflow durability
- define where Compozy introduces new public surfaces instead of inheriting OpenFang contracts wholesale
- keep a single public namespace for the product: `Compozy`
- treat CLI and API as the primary control plane, with UI as a later client
- keep `agent_definition` based on `AgentManifest` while allowing typed
  provider-specific config where the internal Arky crates require it
- keep `task` and `subtask` as the public domain names of the product, even if
  legacy OpenFang queue concepts survive internally
- avoid mixing accepted decisions with earlier assumptions that no longer hold

## How To Read Older `_analysis` Files

Older files outside this folder are still useful as research and historical context, but they are not the current baseline unless this folder points back to them explicitly.

That applies especially to workflow and trigger surfaces: older OpenFang-facing files and routes may still be useful as implementation references or import paths, but they are not public product surfaces for the fork.
