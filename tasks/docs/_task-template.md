## markdown

## status: pending

<task_context>
<domain>engine/infra/[subdomain]</domain>
<type>implementation|integration|testing|documentation</type>
<scope>core_feature|middleware|configuration|performance</scope>
<complexity>low|medium|high|critical</complexity>
<dependencies>none|task1,task2</dependencies>
</task_context>

# Task X.0: [Parent Task Title]

## Overview

[Brief description of task]

<critical>
- **ALWAYS READ** @AGENTS.md before start - **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** the technical docs from this PRD before start
- **YOU CAN ONLY** finish when `pnpm run lint`, `pnpm run typecheck`, and the scope-appropriate test command pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- [Requirement 1]
- [Requirement 2]
</requirements>

## Subtasks

- [ ] X.1 [Subtask description]
- [ ] X.2 [Subtask description]

## Implementation Details

[Relevant sections from tech spec]

### Relevant Files

- `path/to/file.ts`
- `path/to/file.go`

### Dependent Files

- `path/to/dependency.ts`

## Deliverables

- [List of artifacts that constitute "done" for this task]
- [APIs/endpoints/config updates, migrations, docs]
- Unit tests for changed behavior
- Integration tests for affected flows

## Tests

### Unit Tests (Required)

- [ ] [Module A] behavior matrix: [core happy paths + edge cases + failure paths]
- [ ] [Module B] state/lifecycle correctness: [init -> update -> terminal/reset]
- [ ] [Schema/contract tests]: validation and serialization guarantees where applicable

### Integration Tests (Required)

- [ ] End-to-end flow 1: [input] -> [critical transitions] -> [expected output]
- [ ] End-to-end flow 2 (failure/recovery): [fault injection] -> [fallback/recovery behavior]
- [ ] Cross-boundary compatibility: [adapter/protocol/api compatibility expectations]

### Regression and Anti-Pattern Guards

- [ ] Existing behavior parity tests updated or preserved (explicitly name target suite/files).
- [ ] Assertions target observable behavior and contracts, not only mock internals.
- [ ] No test-only production APIs introduced to satisfy test setup/teardown.

### Verification Commands

- [ ] `pnpm run lint`
- [ ] `pnpm run typecheck`
- [ ] [Scope-appropriate test command]

## Success Criteria

- All required checks pass
- All subtasks completed
- [Measurable outcomes]
- [Quality requirements]

---

## Notes

- Save executable task files as `_task_<number>.md` (example: `_task_1.md`).
- `scripts/markdown/check.go` in `prd-tasks` mode discovers only files matching `^_task_\d+\.md$`.
- Keep `## status:` and `<task_context>` fields intact so parser metadata is available in execution prompts.
