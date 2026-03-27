# Fork-Specific Conflict Resolution Patterns

## Core Principle

In a fork sync, conflicts fall into three categories with different resolution defaults:

1. **Fork-owned code**: Code the fork added that does not exist upstream. **Always keep.**
2. **Upstream-owned code**: Code upstream changed that the fork did not touch. **Always take upstream.**
3. **Shared code**: Code both fork and upstream modified. **Requires judgment.**

## Pattern 1: Fork Added Fields/Methods to Upstream Struct

**Scenario:** Fork added custom fields to a struct that upstream also modified.

```rust
// Upstream version
pub struct KernelConfig {
    pub agents: Vec<AgentConfig>,
    pub max_retries: u32,  // NEW upstream field
}

// Fork version
pub struct KernelConfig {
    pub agents: Vec<AgentConfig>,
    pub budget_limit: f64,  // Fork-specific field
}
```

**Resolution:** Merge both additions:

```rust
pub struct KernelConfig {
    pub agents: Vec<AgentConfig>,
    pub max_retries: u32,   // From upstream
    pub budget_limit: f64,  // Fork-specific
}
```

**Verify:** Check all `Default` impl, `new()` constructors, and deserialization still compile.

## Pattern 2: Upstream Changed a Function the Fork Also Changed

**Scenario:** Both modified the same function body.

**Resolution steps:**
1. Read the upstream commit message to understand WHY they changed it
2. Read the fork's change to understand WHY the fork changed it
3. If the intents are compatible, merge both changes
4. If the intents conflict, ask the user (this should have been caught in Step 4)

**Example:**

```rust
// Upstream: Added error handling
pub fn boot(&self) -> Result<()> {
    validate_config(&self.config)?;  // Upstream added this
    self.start_services()
}

// Fork: Added custom initialization
pub fn boot(&self) -> Result<()> {
    self.init_budget_tracker();  // Fork added this
    self.start_services()
}

// Merged: Keep both
pub fn boot(&self) -> Result<()> {
    validate_config(&self.config)?;  // Upstream addition
    self.init_budget_tracker();       // Fork addition
    self.start_services()
}
```

## Pattern 3: Upstream Restructured Imports

**Scenario:** Upstream moved types between modules, breaking fork's `use` statements.

**Resolution:**
1. Note all upstream renames/moves from the diff
2. Update fork code to use new import paths
3. Search for ALL references, not just conflicted files:

```bash
# Find all usages of the old path in fork code
grep -r "use openfang_types::old_module" crates/
```

## Pattern 4: Upstream Changed Trait Signatures

**Scenario:** Upstream added or changed methods on a trait the fork implements.

**Resolution:**
1. Identify all fork implementations of the changed trait
2. Add the new method signatures to fork implementations
3. If the fork has custom trait implementations, ensure they match the new contract

```bash
# Find all implementations of the changed trait
grep -rn "impl.*TraitName" crates/
```

## Pattern 5: Cargo.toml Dependency Conflicts

**Scenario:** Both fork and upstream changed `Cargo.toml`.

**Resolution:**
1. Accept upstream's dependency changes first
2. Re-add fork-specific dependencies
3. Run `cargo check` to verify no version conflicts
4. If version conflicts exist, determine compatible version range

```bash
# After merging Cargo.toml
cargo check 2>&1 | head -50
# Fix any dependency resolution errors
```

## Pattern 6: Database Migration Numbering

**Scenario:** Both fork and upstream added migrations with the same timestamp prefix.

**Resolution:**
1. Keep upstream's migration as-is (preserve upstream compatibility)
2. Renumber the fork's migration to a later timestamp
3. Verify the fork migration still works after upstream's migration

## Pattern 7: Upstream Deleted Code the Fork Extended

**Scenario:** Upstream removed a function/module that the fork built upon.

**Resolution:** This is HIGH risk. Options:
1. Port the fork's extensions to use upstream's replacement
2. Keep the deleted code as fork-specific (if upstream replaced it with something incompatible)
3. Ask the user which approach to take

## Post-Resolution Checklist

After resolving all conflicts, verify:

- [ ] All `use` imports resolve (no broken paths)
- [ ] All trait implementations satisfy their contracts
- [ ] All struct initializations include all fields
- [ ] All `Default` impls match struct definitions
- [ ] No duplicate functions or methods
- [ ] Cargo.toml dependencies resolve cleanly
- [ ] `make fmt && make lint && make test` pass
