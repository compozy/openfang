# Technical Specification: Compozy Integration Gaps

## Executive Summary

Following a comprehensive 8-agent audit of the OpenFang repository, this techspec addresses the remaining integration gaps discovered after the full Compozy PRD implementation. The core integration is production-ready (256+ API endpoints, 19 DB tables, 2,496 tests), but six discrete gaps remain: three missing CLI command groups (A2A, Peers, Budget), a duplicated `SessionId` type across crates, missing `ClassifiedError` implementation on `OpenFangError`, missing serde derives on taint/capability types, migration from the custom template engine to minijinja, and active HITL timeout enforcement. Each gap is scoped as an independent work unit with no cross-dependencies.

## System Architecture

### Domain Placement

All changes land in existing crates -- no new crates required:

- `openfang-cli` -- New CLI subcommand groups (A2A, Peers, Budget)
- `openfang-types` -- `SessionId` unification, serde derives on `TaintSink`/`TaintViolation`/`CapabilityCheck`, `ClassifiedError` impl for `OpenFangError`
- `openfang-kernel` -- Minijinja template rendering, HITL timeout monitor
- `openfang-api` -- Consume unified `SessionId`; no structural changes needed

### Component Overview

Six independent work streams:

1. **CLI: A2A Commands** -- Expose existing `/api/a2a/*` endpoints via CLI
2. **CLI: Peers Commands** -- Expose existing `/api/peers` and `/api/network/status` via CLI
3. **CLI: Budget Commands** -- Expose existing `/api/budget/*` endpoints via CLI
4. **Type Unification: SessionId** -- Eliminate duplicate `SessionId` across `openfang-types` and `arky-protocol`
5. **Error Classification: OpenFangError** -- Implement `ClassifiedError` trait for HTTP status mapping
6. **Template Engine: Minijinja Migration** -- Replace custom `TemplateSegment`-based renderer with minijinja
7. **Serde Derives: Taint & Capability** -- Add `Serialize`/`Deserialize` to internal types
8. **HITL Timeout Enforcement** -- Background monitor for expired HITL requests

## Implementation Design

### Gap 1: CLI A2A Commands

**Current state:** API fully implemented at `/api/a2a/*` (discover, send, agents, task status). No CLI binding.

**New CLI subcommand group:**

```rust
/// Agent-to-Agent communication commands.
#[derive(Subcommand)]
pub enum A2aCommands {
    /// List known external A2A agents.
    List,
    /// Discover an external A2A agent by URL.
    Discover {
        /// The agent card URL to discover.
        url: String,
    },
    /// Send a task to an external A2A agent.
    Send {
        /// Target agent URL.
        url: String,
        /// Message to send.
        message: String,
        /// Optional session ID for continuity.
        #[arg(long)]
        session_id: Option<String>,
    },
    /// Check the status of an external A2A task.
    Status {
        /// Task ID to check.
        id: String,
    },
}
```

**API calls:** HTTP GET/POST to `http://{api_addr}/api/a2a/{agents,discover,send,tasks/{id}/status}`.

**Files to modify:**
- `crates/openfang-cli/src/main.rs` -- Add `A2a(A2aCommands)` variant + handlers

### Gap 2: CLI Peers Commands

**Current state:** API at `/api/peers` and `/api/network/status`. No CLI binding.

```rust
#[derive(Subcommand)]
pub enum PeersCommands {
    /// List connected and known peers.
    List,
    /// Show network status and topology.
    Status,
}
```

**Files to modify:**
- `crates/openfang-cli/src/main.rs` -- Add `Peers(PeersCommands)` variant + handlers

### Gap 3: CLI Budget Commands

**Current state:** Full CRUD API at `/api/budget/*`. No CLI binding.

```rust
#[derive(Subcommand)]
pub enum BudgetCommands {
    /// Show global budget status.
    Status,
    /// Update global budget limits.
    Update {
        /// Hourly USD limit.
        #[arg(long)]
        hourly: Option<f64>,
        /// Daily USD limit.
        #[arg(long)]
        daily: Option<f64>,
        /// Monthly USD limit.
        #[arg(long)]
        monthly: Option<f64>,
    },
    /// Show per-agent budget ranking.
    Agents,
    /// Show budget for a specific agent.
    Agent {
        /// Agent ID.
        id: String,
    },
}
```

**Files to modify:**
- `crates/openfang-cli/src/main.rs` -- Add `Budget(BudgetCommands)` variant + handlers

### Gap 4: SessionId Unification

**Current state:** Two separate `SessionId` types exist:

| Location | Definition | Fields | Notes |
|----------|-----------|--------|-------|
| `openfang-types/src/agent.rs:151` | `pub struct SessionId(pub Uuid)` | Public inner | Simple, fewer methods |
| `arky-protocol/src/id.rs:11` | `pub struct SessionId(Uuid)` | Private inner | More methods (`from_uuid`, `parse_str`, `as_uuid`) |

**Decision:** Keep `arky-protocol::SessionId` as the canonical type (richer API, private field). Re-export from `openfang-types` and remove the duplicate definition.

```rust
// crates/openfang-types/src/agent.rs
// REMOVE the local SessionId struct.
// RE-EXPORT from arky-protocol:
pub use arky_protocol::SessionId;
```

**Risk:** Medium -- requires updating all call sites that rely on `SessionId(pub Uuid)` direct field access. These must use `SessionId::from_uuid()` / `SessionId::as_uuid()` instead.

**Files to modify:**
- `crates/openfang-types/src/agent.rs` -- Remove local `SessionId`, add re-export
- `crates/openfang-types/Cargo.toml` -- Add `arky-protocol` dependency (if not present)
- All crates importing `openfang_types::SessionId` -- Verify field access patterns

### Gap 5: ClassifiedError for OpenFangError

**Current state:** `OpenFangError` (20 variants in `openfang-types/src/error.rs`) uses `thiserror` but does NOT implement `ClassifiedError` from `arky-error`. The arky layer errors (`ProviderError`, `SessionError`, `ToolError`) all implement it with proper HTTP status codes.

**Implementation:**

```rust
use arky_error::ClassifiedError;

impl ClassifiedError for OpenFangError {
    fn error_code(&self) -> &'static str {
        match self {
            Self::AgentNotFound(_) => "AGENT_NOT_FOUND",
            Self::AgentAlreadyExists(_) => "AGENT_ALREADY_EXISTS",
            Self::CapabilityDenied(_) => "CAPABILITY_DENIED",
            Self::QuotaExceeded(_) => "QUOTA_EXCEEDED",
            Self::InvalidState { .. } => "INVALID_STATE",
            Self::SessionNotFound(_) => "SESSION_NOT_FOUND",
            Self::Memory(_) => "MEMORY_ERROR",
            Self::ToolExecution { .. } => "TOOL_EXECUTION_FAILED",
            Self::LlmDriver(_) => "LLM_DRIVER_ERROR",
            Self::Config(_) => "CONFIG_ERROR",
            Self::ManifestParse(_) => "MANIFEST_PARSE_ERROR",
            Self::Sandbox(_) => "SANDBOX_ERROR",
            Self::Network(_) => "NETWORK_ERROR",
            Self::Serialization(_) => "SERIALIZATION_ERROR",
            Self::MaxIterationsExceeded(_) => "MAX_ITERATIONS_EXCEEDED",
            Self::ShuttingDown => "SHUTTING_DOWN",
            Self::Io(_) => "IO_ERROR",
            Self::Internal(_) => "INTERNAL_ERROR",
            Self::AuthDenied(_) => "AUTH_DENIED",
            Self::MeteringError(_) => "METERING_ERROR",
            Self::InvalidInput(_) => "INVALID_INPUT",
        }
    }

    fn is_retryable(&self) -> bool {
        matches!(self, Self::Network(_) | Self::LlmDriver(_))
    }

    fn http_status(&self) -> u16 {
        match self {
            Self::AgentNotFound(_) | Self::SessionNotFound(_) => 404,
            Self::AgentAlreadyExists(_) => 409,
            Self::CapabilityDenied(_) | Self::AuthDenied(_) => 403,
            Self::QuotaExceeded(_) | Self::MaxIterationsExceeded(_) => 429,
            Self::InvalidState { .. } => 409,
            Self::InvalidInput(_) | Self::ManifestParse(_)
            | Self::Config(_) | Self::Serialization(_) => 400,
            Self::ShuttingDown => 503,
            _ => 500,
        }
    }

    fn correction_context(&self) -> Option<serde_json::Value> {
        None
    }
}
```

**Files to modify:**
- `crates/openfang-types/src/error.rs` -- Add `ClassifiedError` impl
- `crates/openfang-types/Cargo.toml` -- Add `arky-error` + `serde_json` dependency

### Gap 6: Minijinja Template Engine Migration

**Current state:** Workflow template rendering uses a custom `TemplateSegment`-based tokenizer (`CompiledTemplate` with `Text` and `Reference` segments). This supports only simple path references like `{{ input.field }}` and `{{ vars.symbol }}` -- no conditionals, no filters, no loops.

**Original design decision:** Minijinja was selected in the PRD decisions doc (`docs/plans/2026-03-23-prd-decisions-design.md`), but the implementation went with the custom approach.

**Why migrate:** Minijinja enables:
- Conditional logic in step inputs: `{% if vars.status == "approved" %}...{% endif %}`
- Filters for data transformation: `{{ vars.name | upper }}`
- Default values: `{{ vars.fallback | default("none") }}`
- Iteration: `{% for item in vars.items %}...{% endfor %}`
- Better error messages with source locations

**Migration plan:**

1. Add `minijinja` dependency to `openfang-kernel`
2. Keep `CompiledTemplate` and `TemplateSegment` types as-is for backward compatibility during transition
3. Add a `MinijinjaRenderer` that wraps `minijinja::Environment`
4. Replace `WorkflowEngine::render_template()` to use minijinja internally
5. The `{{ input.* }}` and `{{ vars.* }}` namespace convention stays the same -- minijinja natively supports this via context objects
6. Deprecate `TemplateSegment::Reference` compilation in `workflow_compiler.rs` -- compile phase stores raw source string instead of tokenizing
7. Remove custom tokenizer code after migration is verified

**Core interface:**

```rust
use minijinja::Environment;

struct TemplateRenderer {
    env: Environment<'static>,
}

impl TemplateRenderer {
    fn render(
        &self,
        source: &str,
        input: &serde_json::Value,
        vars: &HashMap<String, serde_json::Value>,
    ) -> Result<String, OpenFangError> {
        let mut env = Environment::new();
        env.add_template("__inline__", source)
            .map_err(|e| OpenFangError::Internal(format!("template parse: {e}")))?;
        let tmpl = env.get_template("__inline__").expect("just added");
        let ctx = minijinja::context! {
            input => input,
            vars => vars,
        };
        tmpl.render(ctx)
            .map_err(|e| OpenFangError::Internal(format!("template render: {e}")))
    }
}
```

**Backward compatibility:** Existing `{{ input.field }}` and `{{ vars.symbol }}` syntax is valid minijinja syntax. No workflow definitions need to change.

**Files to modify:**
- `crates/openfang-kernel/Cargo.toml` -- `cargo add minijinja`
- `crates/openfang-kernel/src/workflow.rs` -- Replace `render_template()` implementation
- `crates/openfang-kernel/src/workflow_compiler.rs` -- Simplify compile phase (store raw source, skip tokenization)
- `crates/openfang-types/src/workflow.rs` -- Keep `CompiledTemplate` struct, add `source`-only construction path

### Gap 7: Serde Derives on Taint & Capability Types

**Current state:** Three types lack `Serialize`/`Deserialize`:

| Type | Location | Current Derives |
|------|----------|----------------|
| `TaintSink` | `openfang-types/src/taint.rs:116` | `Debug, Clone` |
| `TaintViolation` | `openfang-types/src/taint.rs:163` | `Debug, Clone` |
| `CapabilityCheck` | `openfang-types/src/capability.rs:76` | `Debug, Clone` |

**Fix:** Add `#[derive(Serialize, Deserialize)]` to all three.

**Files to modify:**
- `crates/openfang-types/src/taint.rs` -- Add serde derives to `TaintSink` and `TaintViolation`
- `crates/openfang-types/src/capability.rs` -- Add serde derives to `CapabilityCheck`

### Gap 8: HITL Timeout Enforcement

**Current state:** `hitl_request` table stores `timeout_at` column, but no background task monitors for expired requests. Timed-out requests remain in `pending` status indefinitely.

**Design:** Spawn a periodic background task (every 30s) that scans for `status = 'pending' AND timeout_at < now()` and transitions them to `timed_out`.

```rust
// In kernel boot, inside start_background_agents():
tokio::spawn({
    let stores = workflow_stores.clone();
    let cancel = cancel_token.clone();
    async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = interval.tick() => {
                    if let Err(e) = stores.hitl.expire_timed_out_requests().await {
                        tracing::warn!("HITL timeout sweep failed: {e}");
                    }
                }
            }
        }
    }
});
```

**Files to modify:**
- `crates/openfang-memory/src/hitl.rs` -- Add `expire_timed_out_requests()` method
- `crates/openfang-kernel/src/kernel.rs` -- Add background sweep task in `start_background_agents()`

## Impact Analysis

| Affected Component | Type of Impact | Description & Risk Level | Required Action |
|---|---|---|---|
| `openfang-cli` | New subcommands | Adds A2a, Peers, Budget command groups. Low risk (additive). | Add + test 3 command groups |
| `openfang-types::SessionId` | Breaking type change | Consumers of `SessionId(pub Uuid)` must switch to accessor methods. Medium risk. | Grep all `.0` field accesses |
| `openfang-types::error` | New trait impl | `ClassifiedError` impl on `OpenFangError`. Low risk (additive). | Add dep on `arky-error` |
| `openfang-types::taint` | Derive addition | Serde derives on 2 types. Low risk. | Verify no conflicts |
| `openfang-types::capability` | Derive addition | Serde derive on 1 type. Low risk. | Verify no conflicts |
| `openfang-kernel::workflow` | Renderer replacement | Minijinja replaces custom tokenizer. Medium risk -- must preserve `{{ }}` semantics. | Thorough regression testing |
| `openfang-kernel::workflow_compiler` | Compiler simplification | Skip tokenization, store raw source. Medium risk. | Ensure compiled IR compat |
| `openfang-memory::hitl` | New query | `expire_timed_out_requests()`. Low risk (additive). | Add + test |
| `openfang-kernel::kernel` | New background task | HITL timeout sweep every 30s. Low risk. | CancellationToken wiring |

## Testing Approach

### Unit Tests

| Gap | Key Test Scenarios |
|-----|---|
| CLI A2A | Verify HTTP calls match API contract; test JSON output formatting |
| CLI Peers | Verify list/status output parsing |
| CLI Budget | Verify status/update/agent-detail output |
| SessionId | All existing tests pass with re-exported type; `.0` accesses compile-fail |
| ClassifiedError | Each variant maps to correct HTTP status; retryable variants identified |
| Minijinja | All existing template tests pass unchanged; new tests for conditionals/filters/defaults |
| Serde derives | Round-trip serialization for TaintSink, TaintViolation, CapabilityCheck |
| HITL timeout | Expired requests transition to `timed_out`; non-expired requests unchanged; sweep is idempotent |

### Integration Tests

| Gap | Test File |
|-----|-----------|
| CLI commands | Manual smoke test via `openfang a2a list`, `openfang peers list`, `openfang budget status` |
| Minijinja | Extend `crates/openfang-kernel/tests/workflow_integration_test.rs` with conditional template workflows |
| HITL timeout | Extend `crates/openfang-api/tests/dispatch_hitl_v1_api_test.rs` with timeout expiry scenario |

## Development Sequencing

### Build Order

1. **Gap 7: Serde derives** (trivial, no deps, unblocks nothing but quick win)
2. **Gap 5: ClassifiedError** (adds `arky-error` dep to `openfang-types`, foundational for API consistency)
3. **Gap 4: SessionId unification** (type-level change, must be done before adding new CLI code that uses SessionId)
4. **Gap 1-3: CLI commands** (A2A, Peers, Budget -- independent, can be parallelized across agents)
5. **Gap 6: Minijinja migration** (largest change, isolated to kernel workflow rendering)
6. **Gap 8: HITL timeout** (depends on existing HITL infra, can be done last)

### Technical Dependencies

- Minijinja migration requires `cargo add minijinja` to `openfang-kernel`
- `ClassifiedError` impl requires `cargo add arky-error` to `openfang-types`
- SessionId unification may require `cargo add arky-protocol` to `openfang-types` (verify if already present)
- No external service dependencies; all changes are local

## Technical Considerations

### Key Decisions

| Decision | Rationale | Alternative Rejected |
|----------|-----------|---------------------|
| Keep `arky-protocol::SessionId` as canonical | Richer API, private field (encapsulation) | Keep `openfang-types` version (less safe) |
| Minijinja over custom tokenizer | Jinja2 is industry standard; enables conditionals/filters | Keep custom (limits workflow expressiveness) |
| 30s HITL sweep interval | Balances responsiveness vs DB load | Webhook-based (over-engineered for SQLite) |
| Additive CLI commands (no refactor) | Follows existing CLI patterns; minimal diff | Full CLI restructure (unnecessary scope) |

### Known Risks

| Risk | Mitigation |
|------|-----------|
| Minijinja syntax edge cases vs custom tokenizer | Keep all existing template tests; add regression suite |
| SessionId field access breakage | `cargo check` catches all `.0` accesses at compile time |
| HITL sweep race with manual answer | Use `UPDATE WHERE status = 'pending'` -- atomic, no race |
| CLI output format inconsistency | Follow existing CLI output patterns (JSON with `--json` flag) |

### Standards Compliance

- All changes follow `rust-best-practices` (error handling with `?`, `thiserror`, no `unwrap()`)
- `make fmt && make lint && make test` must pass before each gap is marked complete
- Conventional commits: `feat(cli): add a2a commands`, `refactor(types): unify SessionId`, etc.
- No new crates introduced; all changes in existing crate boundaries
