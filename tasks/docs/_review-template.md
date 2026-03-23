## markdown

## status: pending # Options: pending, in-progress, completed, excluded

<task_context>
<domain>engine/infra/[subdomain]</domain>
<type>implementation|integration|testing|documentation</type>
<scope>core_feature|middleware|configuration|performance</scope>
<complexity>low|medium|high</complexity>
<dependencies>external_apis|database|temporal|http_server</dependencies>
</task_context>

# Task X.0: [Parent Task Title]

## Overview

[Brief description of task]

<critical>
- **ALWAYS READ** @AGENTS.md before start - **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** the technicals docs from this PRD before start
- **YOU CAN ONLY** finish a task if `pnpm run lint`, `pnpm run typecheck` and `pnpm run test` (Vitest) are passing, your task should not finish before this
- **IF YOU DON'T CHECK SKILLS** your task will be invalidated and we will generate rework
- **IF YOU DON'T** finish all the issues here, your job will be invalidated
</critical>

<research>
# When you need information about a library, external API or research:
- use .claude/commands/sourcebot.md
- use .claude/commands/perplexity.md
</research>

<requirements>
- [Requirement 1]
- [Requirement 2]
</requirements>

## Subtasks

- [ ] X.1 [Subtask description]
- [ ] X.2 [Subtask description]

## Issues Details

[List of each issue with details and separated by: CRITICAL, MEDIUM, LOW priority]

### Relevant Files

- `path/to/file.go`
- `path/to/file.ts`

### Dependent Files

- `path/to/dependency.go`

## Deliverables

- [List of artifacts that constitute "done" for this task]
- [APIs/endpoints/config updates, migrations, docs]
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for [feature] **(REQUIRED)**
- Regression tests for [feature] **(REQUIRED)**

## Tests

- Unit tests and regression tests:
  - [ ] [Test case 1]
  - [ ] [Test case 2]
  - [ ] [Edge cases / error paths]
- Test coverage target: ≥80%
- All tests must pass
