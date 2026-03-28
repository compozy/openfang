## status: pending

<task_context>
<domain>openfang-api/src</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>none</dependencies>
</task_context>

# Task 12.0: Arky Provider Backend — Profiles CRUD Endpoint

## Overview

Create the missing `/api/v1/provider-profiles` CRUD endpoint in the Rust backend. The Arky provider profile system exists in the config layer (`arky-config`) but has no API surface for management. Also fix the `ValidationContext.known_profiles` seeding so agent validation actually checks profile references.

<critical>
- **ALWAYS READ** @CLAUDE.md before start - **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-ui/adr-009-arky-providers.md` and `tasks/prd-ui/analysis_arky_providers.md`
- **YOU CAN ONLY** finish when `make fmt && make lint && make test` pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
- **THIS IS A RUST TASK** — use `rust-best-practices` skill
</critical>

<requirements>
- `GET /api/v1/provider-profiles` — list all provider profiles
- `GET /api/v1/provider-profiles/{id}` — get profile detail
- `POST /api/v1/provider-profiles` — create profile
- `PUT /api/v1/provider-profiles/{id}` — update profile
- `DELETE /api/v1/provider-profiles/{id}` — delete profile
- Profile data model: id, name, driver, model, defaults (max_tokens, reasoning_effort), config (driver-specific behavior layer)
- Fix `ValidationContext.known_profiles` in agent validation routes to seed from profile store
- Profiles persisted in config (file-backed or DB-backed, match existing pattern)
</requirements>

## Subtasks

- [ ] 12.1 Define `ProviderProfile` API types in `crates/openfang-api/src/routes.rs` or a dedicated types module
- [ ] 12.2 Implement profile storage — determine whether file-backed (like workflows) or DB-backed (like tasks)
- [ ] 12.3 Implement `list_provider_profiles` handler
- [ ] 12.4 Implement `get_provider_profile` handler
- [ ] 12.5 Implement `create_provider_profile` handler
- [ ] 12.6 Implement `update_provider_profile` handler
- [ ] 12.7 Implement `delete_provider_profile` handler
- [ ] 12.8 Register routes in `server.rs` under `/api/v1/provider-profiles`
- [ ] 12.9 Fix `ValidationContext.known_profiles` — seed from profile store in agent validate/compile handlers
- [ ] 12.10 Write integration tests for all 5 CRUD endpoints
- [ ] 12.11 Write test for profile validation seeding

## Implementation Details

### Profile Data Model

Based on `arky-config`'s `ProviderProfileConfig`:

```rust
struct ProviderProfileResponse {
    id: String,
    name: String,
    driver: String,
    model: Option<String>,
    defaults: ProviderRequestDefaultsResponse,
    config: Option<serde_json::Value>, // driver-specific behavior layer
    created_at: String,
    updated_at: String,
}

struct ProviderRequestDefaultsResponse {
    max_tokens: Option<u32>,
    reasoning_effort: Option<String>, // "low" | "medium" | "high" | "xhigh"
}
```

### Relevant Files

- `crates/openfang-api/src/routes.rs` (MODIFY — add handlers)
- `crates/openfang-api/src/server.rs` (MODIFY — register routes)
- `crates/openfang-api/tests/api_integration_test.rs` (MODIFY — add tests)
- `crates/arky-config/src/lib.rs` (REFERENCE — ProviderProfileConfig)

## Deliverables

- 5 CRUD endpoints for provider profiles registered and functional
- `known_profiles` seeded in agent validation context
- Integration tests for all endpoints

## Tests

### Unit Tests (Required)

- [ ] Profile CRUD: create, read, update, delete
- [ ] Profile validation: verify `known_profiles` seeding affects validation output

### Integration Tests (Required)

- [ ] `GET /api/v1/provider-profiles` returns list
- [ ] `POST /api/v1/provider-profiles` creates profile, returns 201
- [ ] `GET /api/v1/provider-profiles/{id}` returns profile
- [ ] `PUT /api/v1/provider-profiles/{id}` updates profile
- [ ] `DELETE /api/v1/provider-profiles/{id}` removes profile, returns 204
- [ ] Agent validation with `known_profiles` seeded rejects unknown profile references

### Verification Commands

- [ ] `make fmt`
- [ ] `make lint`
- [ ] `make test`

## Success Criteria

- All 5 CRUD endpoints functional
- `known_profiles` seeded correctly in validation
- All tests pass
- `make fmt && make lint && make test` pass
