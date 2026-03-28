# ADR-009: Dedicated Phase for Arky Provider UI Integration

## Status

Accepted

## Date

2026-03-27

## Context

The Arky provider subsystem (10 crates under `crates/arky-*`) introduces a layered provider SDK with 10 drivers, 3-tier config merging (workspace < profile < agent), driver-specific typed configs, reasoning effort controls, and inline MCP passthrough. None of this is exposed in the current UI, which still uses the legacy model-catalog provider system.

Additionally, the backend is missing a `/api/v1/provider-profiles` CRUD endpoint — the Arky profile system exists in the config layer but has no API surface for management.

## Decision

Create a dedicated Phase 4.5 (between Agents v1 migration and Looper/Packs) specifically for the Arky provider UI integration. This phase covers:

1. Provider Profiles management page (requires new backend endpoint)
2. Agent spawn wizard upgrade to Arky drivers
3. Driver-specific config editor (codex, claude-code, claude-compatible)
4. Reasoning effort and max_tokens controls
5. Compiled binding inspector panel
6. Inline MCP server config distinction from global MCP

A backend task must be filed for the missing `/api/v1/provider-profiles` CRUD endpoint before this phase can begin.

## Alternatives Considered

### Alternative 1: Fold into Phase 4 (Agents Rebuild)

- **Description**: Add provider config to the agents page rebuild.
- **Pros**: One pass over the agents page
- **Cons**: Phase 4 becomes very large. Driver-specific config is complex enough to warrant focused attention. Provider profiles are a settings-level feature, not an agent-level feature.
- **Why rejected**: Mixing agent CRUD rebuild with provider config redesign creates too large a diff and too many failure modes.

### Alternative 2: Defer to Follow-Up PRD

- **Description**: Document as a known gap, implement later.
- **Pros**: Keeps current scope manageable
- **Cons**: Users cannot configure Arky-backed agents through the dashboard. The spawn wizard continues to use the wrong provider list.
- **Why rejected**: The dashboard is meant to be the single control plane. Leaving the provider system out defeats that goal.

### Alternative 3: Minimal (Just Fix Spawn Wizard)

- **Description**: Only update the spawn wizard to show Arky drivers.
- **Pros**: Small scope, quick fix
- **Cons**: No profiles, no driver-specific config, no reasoning effort. Users can pick a driver but can't configure it.
- **Why rejected**: Picking a driver without configuring it (sandbox mode, region, MCP servers) makes the driver selection meaningless.

## Consequences

### Positive

- Provider system gets focused attention and proper UX design
- Provider profiles become a first-class managed resource
- Driver-specific configs are properly typed and validated
- Compiled binding inspector helps operators debug provider resolution

### Negative

- Adds a full phase to the timeline
- Requires a backend task (provider-profiles CRUD endpoint) before UI work
- Phase ordering: Phase 4 (agents) ships without full Arky awareness, then Phase 4.5 enhances it

### Risks

- The missing `/api/v1/provider-profiles` endpoint is a backend dependency. If not built, the profiles management page cannot be completed. Mitigation: the provider profiles page can be deferred within Phase 4.5 while driver-specific config and reasoning effort can proceed using the existing agent definition fields.
- The `ValidationContext.known_profiles` is always empty (latent bug). The backend task should also fix this.

## Implementation Notes

### Backend Prerequisites

1. New CRUD endpoint: `GET/POST/PUT/DELETE /api/v1/provider-profiles`
2. Seed `ValidationContext.known_profiles` from the profile store in agent validation routes
3. Fix the always-empty `known_profiles` guard in validation

### Phase 4.5 UI Scope

1. **Provider Profiles page** (Settings or standalone)
   - List profiles with driver, model, defaults
   - Create/edit form with driver selector -> typed config fields
   - Delete with confirmation

2. **Spawn Wizard upgrade**
   - Replace model-catalog provider dropdown with Arky driver selector (10 drivers)
   - Show driver-specific config fields based on selected driver
   - Profile picker (from provider profiles)
   - Reasoning effort selector (None/Low/Medium/High/XHigh)
   - max_tokens override field

3. **Agent Detail: Provider Config section**
   - Show resolved driver, model, profile reference
   - Display driver-specific config (read-only or editable)
   - Display defaults (reasoning_effort, max_tokens)
   - "View Compiled Binding" expandable showing `/api/v1/agents/{id}/compiled` output

4. **Inline MCP distinction**
   - Agent detail differentiates global MCP servers (from kernel config) from inline MCP servers (from `provider.config.claude_code.mcp_servers`)
   - Edit inline MCP servers in the provider config section

## References

- `crates/arky-config/src/lib.rs` — 3-tier config merge system
- `crates/arky-claude-code/src/profile.rs` — driver taxonomy and compatible providers
- `crates/openfang-provider-binding/src/lib.rs` — compiled ProviderBinding struct
- `tasks/prd-ui/analysis_prd_tasks_1_15.md` — Task 10 (provider profiles), Task 11 (binding compile), Task 12 (typed adapters)
