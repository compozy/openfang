## markdown

## status: completed

<task_context>
<domain>openfang-types</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>low</complexity>
<dependencies>none</dependencies>
</task_context>

# Task 44.0: Types Cleanup — Serde Derives + ClassifiedError

## Overview

Add missing `Serialize`/`Deserialize` derives to three internal types (`TaintSink`, `TaintViolation`, `CapabilityCheck`) and implement the `ClassifiedError` trait for `OpenFangError` to enable consistent HTTP status mapping across the API layer.

<critical>
- **ALWAYS READ** @CLAUDE.md before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-gaps/techspec.md` (Gaps 5 and 7) before start
- **YOU CAN ONLY** finish when `make fmt`, `make lint`, and `make test` pass at 100%
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Add `Serialize, Deserialize` derives to `TaintSink`, `TaintViolation`, and `CapabilityCheck`
- Implement `ClassifiedError` trait from `arky-error` on `OpenFangError`
- Each `OpenFangError` variant must map to a correct HTTP status code
- Retryable variants (`Network`, `LlmDriver`) must return `true` from `is_retryable()`
- All existing tests must continue to pass unchanged
</requirements>

## Subtasks

- [x] 44.1 Add `Serialize, Deserialize` to `TaintSink` in `crates/openfang-types/src/taint.rs`
- [x] 44.2 Add `Serialize, Deserialize` to `TaintViolation` in `crates/openfang-types/src/taint.rs`
- [x] 44.3 Add `Serialize, Deserialize` to `CapabilityCheck` in `crates/openfang-types/src/capability.rs`
- [x] 44.4 Add `arky-error` dependency to `openfang-types` via `cargo add`
- [x] 44.5 Implement `ClassifiedError` for `OpenFangError` with HTTP status mapping
- [x] 44.6 Write unit tests for ClassifiedError impl
- [x] 44.7 Write round-trip serde tests for newly-serializable types
- [x] 44.8 Run `make fmt && make lint && make test` — all must pass

## Implementation Details

### ClassifiedError HTTP Status Mapping

| Variant | HTTP Status | Error Code | Retryable |
|---------|------------|------------|-----------|
| `AgentNotFound` | 404 | `AGENT_NOT_FOUND` | No |
| `AgentAlreadyExists` | 409 | `AGENT_ALREADY_EXISTS` | No |
| `CapabilityDenied` | 403 | `CAPABILITY_DENIED` | No |
| `QuotaExceeded` | 429 | `QUOTA_EXCEEDED` | No |
| `InvalidState` | 409 | `INVALID_STATE` | No |
| `SessionNotFound` | 404 | `SESSION_NOT_FOUND` | No |
| `Memory` | 500 | `MEMORY_ERROR` | No |
| `ToolExecution` | 500 | `TOOL_EXECUTION_FAILED` | No |
| `LlmDriver` | 502 | `LLM_DRIVER_ERROR` | Yes |
| `Config` | 400 | `CONFIG_ERROR` | No |
| `ManifestParse` | 400 | `MANIFEST_PARSE_ERROR` | No |
| `Sandbox` | 500 | `SANDBOX_ERROR` | No |
| `Network` | 502 | `NETWORK_ERROR` | Yes |
| `Serialization` | 400 | `SERIALIZATION_ERROR` | No |
| `MaxIterationsExceeded` | 429 | `MAX_ITERATIONS_EXCEEDED` | No |
| `ShuttingDown` | 503 | `SHUTTING_DOWN` | No |
| `Io` | 500 | `IO_ERROR` | No |
| `Internal` | 500 | `INTERNAL_ERROR` | No |
| `AuthDenied` | 403 | `AUTH_DENIED` | No |
| `MeteringError` | 500 | `METERING_ERROR` | No |
| `InvalidInput` | 400 | `INVALID_INPUT` | No |

### Relevant Files

- `crates/openfang-types/src/error.rs` — ClassifiedError impl target
- `crates/openfang-types/src/taint.rs` — TaintSink, TaintViolation
- `crates/openfang-types/src/capability.rs` — CapabilityCheck
- `crates/arky-error/src/lib.rs` — ClassifiedError trait definition

### Dependent Files

- `crates/openfang-types/Cargo.toml` — needs `arky-error` dependency

## Deliverables

- `ClassifiedError` impl on `OpenFangError` with correct status codes for all 21 variants
- Serde derives on `TaintSink`, `TaintViolation`, `CapabilityCheck`
- Unit tests for error code mapping, HTTP status mapping, and retryability
- Round-trip serde tests for the three newly-serializable types
- `cargo add arky-error` to `openfang-types`

## Tests

### Unit Tests (Required)

- [x] Each `OpenFangError` variant returns correct `error_code()` string
- [x] Each variant returns correct `http_status()` code
- [x] `Network` and `LlmDriver` return `is_retryable() == true`; all others `false`
- [x] `TaintSink` serializes and deserializes round-trip
- [x] `TaintViolation` serializes and deserializes round-trip
- [x] `CapabilityCheck::Granted` and `CapabilityCheck::Denied` serialize/deserialize round-trip

### Integration Tests (Required)

- [x] Existing taint module tests still pass (behavior parity)
- [x] Existing capability module tests still pass (behavior parity)
- [x] Existing error-related tests across all crates still pass

### Regression and Anti-Pattern Guards

- [x] No changes to existing test files — purely additive
- [x] No test-only production APIs introduced
- [x] Assertions target observable behavior (error codes, status codes, serde output)

### Verification Commands

- [x] `make fmt`
- [x] `make lint`
- [x] `make test`

## Success Criteria

- All 21 OpenFangError variants have ClassifiedError coverage
- Three types gain Serialize/Deserialize without breaking existing consumers
- Zero warnings, zero errors, zero test failures
