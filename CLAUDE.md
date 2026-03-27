# OpenFang — Agent Instructions

## Project Overview

OpenFang is an open-source Agent Operating System written in Rust (14+ crates).
- Config: `~/.openfang/config.toml`
- Default API: `http://127.0.0.1:4200`
- CLI binary: `target/release/openfang.exe` (or `target/debug/openfang.exe`)

## HIGH PRIORITY

- **IF YOU DON'T CHECK SKILLS** your task will be invalidated and we will generate rework
- **YOU CAN ONLY** finish a task after `make fmt && make lint && make test` are **ALL passing at 100%**. No exceptions.
- **ALWAYS** check dependent crate APIs before writing tests to avoid wrong code
- **NEVER** use workarounds, especially in tests - always use the `no-workarounds` skill for any fix/debug task + `test-anti-patterns` for tests
- **ALWAYS** use the `no-workarounds` and `systematic-debugging` skills when fixing bugs or complex issues
- **ALWAYS** use `requirements-clarity` before implementing ambiguous multi-crate features or underspecified requests
- **USE** `qa-test-planner` when defining regression scope or test strategy for significant changes
- **USE** `adversarial-review` before closing large or high-risk diffs that deserve a critical second pass

## MANDATORY REQUIREMENTS

- **MUST** run `make fmt && make lint && make test` before completing ANY subtask. All three commands must exit with **zero errors and zero warnings**. If any command fails, fix the issues and re-run until all pass.
- **ALWAYS USE** the `rust-best-practices` skill for ALL Rust work
- **YOU SHOULD NEVER** add dependencies by hand to Cargo.toml - always use `cargo add` instead
- **USE** `executing-plans` skill when working through PRD task files
- **USE** `fix-coderabbit-review` skill when addressing CodeRabbit review issues
- **USE** `ratatui-tui` skill when working on the interactive CLI/TUI
- **THIRD PARTY LIBRARIES** (just applied when needing external resources):
  - **MANDATORY** Use the `find-docs` skill for **EXTERNAL** libraries, frameworks, SDKs, APIs, and code patterns when you need up-to-date documentation, references, or examples
  - **NEVER** use `find-docs` (or other external-documentation workflows) to search **local** project code. For local code, use `codebase_search` or `Grep`/`Glob` instead

### CRITICAL: Git Commands Restriction

- **ABSOLUTELY FORBIDDEN**: **NEVER** run `git restore`, `git checkout`, `git reset`, `git clean`, `git rm`, `git push --force`, `git branch -D`, or any other git commands that modify or discard working directory changes **WITHOUT EXPLICIT USER PERMISSION**.
- **DATA LOSS RISK**: These commands can **PERMANENTLY LOSE CODE CHANGES** and cannot be easily recovered.
- **REQUIRED ACTION**: If you need to revert or discard changes, **YOU MUST ASK THE USER FIRST** and wait for explicit permission before executing any destructive git command.
- **VIOLATION CONSEQUENCE**: Running destructive git commands without explicit permission will result in **IMMEDIATE TASK REJECTION** and potential **IRREVERSIBLE DATA LOSS**.

### Code Search and Discovery

- **TOOL HIERARCHY**: Use tools in this order:
  1. `codebase_search` (if available) - Preferred semantic search tool
  2. `Grep` or `Glob` (when exact string matching is needed)
- **FORBIDDEN**: Never use `grep` or `find` via Bash for semantic code discovery without first trying dedicated tools.

## Build, Test, and Development Commands

All commands run from repository root via Makefile:

```bash
make fmt          # Format (uses nightly for unstable rustfmt options)
make fmt-check    # Check formatting without modifying files
make lint         # fmt-check + clippy with -D warnings
make lint-clippy  # Run clippy lints only
make lint-fix     # Auto-fix clippy warnings where possible
make test         # Run all tests with --all-features
make verify       # fmt + lint + test (full verification — MANDATORY before completing any task)
make build        # Build in release mode
make check        # Type-check without producing binaries
make coverage     # Print code coverage summary
make clean        # Clean build artifacts
```

**MANDATORY Verification (BLOCKING):** Before completing ANY task, you **MUST** run all three commands and they **MUST** all pass at 100%:

1. `make fmt` - Format all code. Must exit cleanly.
2. `make lint` - Must pass with **zero warnings and zero errors** (includes fmt check + clippy with `-D warnings`).
3. `make test` - All tests must pass with **zero failures**.

**If any of these commands fail, the task is NOT complete.** Fix all issues and re-run until all three pass.

Alternatively, the raw cargo commands:
```bash
cargo +nightly-2026-03-15 fmt --all --check       # Formatting (nightly required)
cargo clippy --all-targets --all-features -- -D warnings  # Linting
cargo test --all-features                          # Tests
```

## Coding Style & Naming Conventions

- **Edition**: 2024
- **Max line width**: 100
- **Imports granularity**: Crate-level, vertical layout
- **Format**: `make fmt` (nightly channel for unstable options). See `.rustfmt.toml` for full rules.
- **Lint**: `make lint` (clippy with `-D warnings`). See `.clippy.toml` for disallowed macros/methods.
- **Naming**: `snake_case` (fn/var), `CamelCase` (type), `SCREAMING_SNAKE_CASE` (const)
- **No `get_` prefix**: Use `fn name()` not `fn get_name()`
- **Conversions**: `as_` (cheap &), `to_` (expensive), `into_` (ownership)
- **Iterators**: `iter()` / `iter_mut()` / `into_iter()`
- **Newtypes**: Use `struct Email(String)` for domain semantics
- **Pre-allocate**: `Vec::with_capacity()`, `String::with_capacity()`

### Error Handling

- Use `thiserror` for all error types (library code)
- Return `Result<T, E>` for fallible operations; never `panic!` in library code
- Never use `unwrap()` in production code (use `expect()` with messages in dev only)
- Use `?` operator for error propagation, not `match` chains
- Each crate defines its own error enum with `thiserror`

### Async

- All I/O-bound operations are async, using `tokio` runtime
- `CancellationToken` from `tokio-util` for cooperative cancellation
- Never hold locks across `.await` points
- Use `JoinSet` for managing multiple concurrent tasks
- Sync for CPU-bound work; async is for I/O

### Traits

- Keep traits small and focused (4-6 methods max)
- Use dynamic dispatch (`Box<dyn Trait>`) for heterogeneous collections
- Static dispatch for monomorphic paths
- All public traits must be `Send + Sync`
- Use `#[async_trait]` for async trait methods

### Testing

- Use `pretty_assertions::assert_eq` instead of `std::assert_eq` (enforced by clippy)
- Name tests descriptively: `process_should_return_error_when_input_empty()`
- One assertion per test when possible
- Use `#[tokio::test]` for async tests
- Use doc tests (`///`) for public API examples

### Documentation

- `//!` for module-level docs
- `///` for public items
- `//` comments explain _why_ (safety, workarounds, design rationale)
- Every `TODO` needs a linked issue: `// TODO(#42): ...`

### Disallowed Patterns (enforced by .clippy.toml)

- **No `log` crate**: Use `tracing` instead
- **No `todo!()`, `dbg!()`, `unimplemented!()`**: Do not commit these
- **No `std::assert_eq`/`std::assert_ne`**: Use `pretty_assertions` versions
- **No `for_each`/`try_for_each`**: Use `for` loops for side-effects
- **No `map_or`/`map_or_else`**: Use `map(..).unwrap_or(..)` for legibility

## Dependency Graph Validation

Run `./scripts/check-deps.sh` to validate the internal crate dependency graph. This prevents accidental circular dependencies or unauthorized cross-crate references.

## Commit & Pull Request Guidelines

- Use Conventional Commits: `feat: ...`, `fix: ...`, `build: ...`, `refactor: ...`, `test: ...`, `docs: ...`
- Before opening a PR: run `make verify` (fmt + lint + test)
- PRs should include: clear description and linked issue
- Do not rewrite unrelated files or reformat whole repo - limit diffs to your change

## Agent Skill Dispatch Protocol

Every agent MUST follow this protocol before writing code:

### Step 1: Identify Task Domain

- **Kernel**: boot, lifecycle, state -> `rust-best-practices`
- **API/Server**: routes, endpoints, axum -> `rust-best-practices`
- **Types/Config**: shared types, config loading -> `rust-best-practices`
- **Memory/Persistence**: SQLite, migrations, repositories -> `rust-best-practices`
- **Runtime/Workflow**: workflow engine, dispatch, HITL -> `rust-best-practices`
- **Provider/Arky**: provider trait, bindings, adapters -> `rust-best-practices`
- **CLI/TUI**: interactive terminal UI -> `ratatui-tui` + `rust-best-practices`
- **Bug fix**: any domain -> `systematic-debugging` + `no-workarounds` + domain skills
- **Tests**: any domain -> `test-anti-patterns` + domain skills
- **PRD task execution** -> `executing-plans` + domain skills
- **CodeRabbit reviews** -> `fix-coderabbit-review` + domain skills
- **Ambiguous requirements / multi-crate scope** -> `requirements-clarity`
- **Test plan / regression scope** -> `qa-test-planner` + `test-anti-patterns`
- **Large or high-risk diff review** -> `adversarial-review`
- **External libraries / documentation research** -> `find-docs`
- **Architecture diagrams** -> `mermaid-diagrams`
- **Documentation / writing** -> `writing-clearly-and-concisely` + `crafting-effective-readmes`
- **Skill discovery / capability gaps** -> `find-skills`

### Step 2: Activate All Matching Skills

| Domain | Required Skills | Conditional Skills |
|--------|----------------|-------------------|
| Any Rust code | `rust-best-practices` | |
| CLI/TUI work | `ratatui-tui` | + `rust-best-practices` |
| Bug fix | `systematic-debugging` + `no-workarounds` | + `test-anti-patterns` (test failures) |
| Writing tests | `test-anti-patterns` | + domain skill for code being tested |
| PRD task execution | `executing-plans` | + domain skills |
| CodeRabbit reviews | `fix-coderabbit-review` | + domain skills |
| Ambiguous requirements | `requirements-clarity` | + domain skills after scope is clear |
| Test planning / regression design | `qa-test-planner` | + `test-anti-patterns` |
| High-risk change review | `adversarial-review` | + `receiving-code-review` (follow-up feedback) |
| External libraries, SDKs, docs | `find-docs` | |
| Task completion | `verification-before-completion` | |
| Code review response | `receiving-code-review` | |
| Git rebase/conflicts | `git-rebase` | |
| Architecture audit | `architectural-analysis` | + `adversarial-review` (for risky structural changes) |
| Architecture diagrams | `mermaid-diagrams` | |
| Documentation / writing | `writing-clearly-and-concisely` | + `crafting-effective-readmes` (READMEs) |
| Post-implementation review | `lesson-learned` | |
| Skill discovery / workflow extension | `find-skills` | |

### Step 3: Verify Before Completion

Before any agent marks a task as complete:

1. Activate `verification-before-completion` skill
2. Run `make fmt && make lint && make test` - all three must pass at 100% with zero errors and zero warnings
3. Read and verify the full output - no skipping
4. Only then claim completion

## Anti-Patterns for Agents

**NEVER do these:**

1. **Skip skill activation** because "it's a small change" - every domain change requires its skill
2. **Activate only one skill** when the task touches multiple domains
3. **Forget `verification-before-completion`** before marking tasks done
4. **Use `find-docs` for local project code** — local code belongs in `codebase_search`, `Grep`, or `Glob`
5. **Write tests without `test-anti-patterns`** - leads to bad test patterns
6. **Fix bugs without `systematic-debugging`** - leads to symptom-patching
7. **Apply workarounds without `no-workarounds`** - type assertions, lint suppressions, error swallowing are all rejected
8. **Start implementation with unclear scope and skip `requirements-clarity`** - this creates avoidable rework
9. **Skip `qa-test-planner` when designing meaningful regression coverage** - this weakens validation quality
10. **Ship large or risky diffs without `adversarial-review`** - this misses obvious failure modes
11. **Complete tasks without running `make fmt && make lint && make test`** - all three must pass. Skipping any invalidates the task.
12. **Claim task is done when any check has warnings or errors** - zero warnings, zero errors, zero test failures. No exceptions.
13. **Use `unwrap()` in library code** - always use `?` or `expect()` with a message
14. **Use `log` crate** - use `tracing` instead (enforced by clippy)
15. **Commit `todo!()`, `dbg!()`, or `unimplemented!()`** - enforced by clippy

## MANDATORY: Live Integration Testing

**After implementing any new endpoint, feature, or wiring change, you MUST run live integration tests.** Unit tests alone are not enough — they can pass while the feature is actually dead code. Live tests catch:
- Missing route registrations in server.rs
- Config fields not being deserialized from TOML
- Type mismatches between kernel and API layers
- Endpoints that compile but return wrong/empty data

### How to Run Live Integration Tests

#### Step 1: Stop any running daemon
```bash
tasklist | grep -i openfang
taskkill //PID <pid> //F
# Wait 2-3 seconds for port to release
sleep 3
```

#### Step 2: Build fresh release binary
```bash
cargo build --release -p openfang-cli
```

#### Step 3: Start daemon with required API keys
```bash
GROQ_API_KEY=<key> target/release/openfang.exe start &
sleep 6  # Wait for full boot
curl -s http://127.0.0.1:4200/api/health  # Verify it's up
```
The daemon command is `start` (not `daemon`).

#### Step 4: Test every new endpoint
```bash
# GET endpoints — verify they return real data, not empty/null
curl -s http://127.0.0.1:4200/api/<new-endpoint>

# POST/PUT endpoints — send real payloads
curl -s -X POST http://127.0.0.1:4200/api/<endpoint> \
  -H "Content-Type: application/json" \
  -d '{"field": "value"}'

# Verify write endpoints persist — read back after writing
curl -s -X PUT http://127.0.0.1:4200/api/<endpoint> -d '...'
curl -s http://127.0.0.1:4200/api/<endpoint>  # Should reflect the update
```

#### Step 5: Test real LLM integration
```bash
# Get an agent ID
curl -s http://127.0.0.1:4200/api/agents | python3 -c "import sys,json; print(json.load(sys.stdin)[0]['id'])"

# Send a real message (triggers actual LLM call to Groq/OpenAI)
curl -s -X POST "http://127.0.0.1:4200/api/agents/<id>/message" \
  -H "Content-Type: application/json" \
  -d '{"message": "Say hello in 5 words."}'
```

#### Step 6: Verify side effects
After an LLM call, verify that any metering/cost/usage tracking updated:
```bash
curl -s http://127.0.0.1:4200/api/budget       # Cost should have increased
curl -s http://127.0.0.1:4200/api/budget/agents  # Per-agent spend should show
```

#### Step 7: Verify dashboard HTML
```bash
# Check that new UI components exist in the served HTML
curl -s http://127.0.0.1:4200/ | grep -c "newComponentName"
# Should return > 0
```

#### Step 8: Cleanup
```bash
tasklist | grep -i openfang
taskkill //PID <pid> //F
```

### Key API Endpoints for Testing
| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/api/health` | GET | Basic health check |
| `/api/agents` | GET | List all agents |
| `/api/agents/{id}/message` | POST | Send message (triggers LLM) |
| `/api/budget` | GET/PUT | Global budget status/update |
| `/api/budget/agents` | GET | Per-agent cost ranking |
| `/api/budget/agents/{id}` | GET | Single agent budget detail |
| `/api/network/status` | GET | OFP network status |
| `/api/peers` | GET | Connected OFP peers |
| `/api/a2a/agents` | GET | External A2A agents |
| `/api/a2a/discover` | POST | Discover A2A agent at URL |
| `/api/a2a/send` | POST | Send task to external A2A agent |
| `/api/a2a/tasks/{id}/status` | GET | Check external A2A task status |

## Architecture Notes

- **Don't touch `openfang-cli`** — user is actively building the interactive CLI
- `KernelHandle` trait avoids circular deps between runtime and kernel
- `AppState` in `server.rs` bridges kernel to API routes
- New routes must be registered in `server.rs` router AND implemented in `routes.rs`
- Dashboard is Alpine.js SPA in `static/index_body.html` — new tabs need both HTML and JS data/methods
- Config fields need: struct field + `#[serde(default)]` + Default impl entry + Serialize/Deserialize derives

## Common Gotchas

- `openfang.exe` may be locked if daemon is running — use `--lib` flag or kill daemon first
- `PeerRegistry` is `Option<PeerRegistry>` on kernel but `Option<Arc<PeerRegistry>>` on `AppState` — wrap with `.as_ref().map(|r| Arc::new(r.clone()))`
- Config fields added to `KernelConfig` struct MUST also be added to the `Default` impl or build fails
- `AgentLoopResult` field is `.response` not `.response_text`
- CLI command to start daemon is `start` not `daemon`
- On Windows: use `taskkill //PID <pid> //F` (double slashes in MSYS2/Git Bash)
