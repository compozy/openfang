# Task 5 Review: Shared Definition Contract Types

## Status: PASS

## Checklist
- [x] `contract.rs` created at `crates/openfang-types/src/contract.rs`
- [x] `pub mod contract;` registered in `crates/openfang-types/src/lib.rs`
- [x] `ContractKind` enum: all 7 structural kinds (`string`, `integer`, `number`, `boolean`, `object`, `array`, `any`) and all 6 semantic kinds (`artifact_ref`, `doc_ref`, `issue_ref`, `task_ref`, `task_list`, `run_ref`) — total 13 variants, no `Text`/`Json` alias variants
- [x] `ContractNode` struct with all required fields: `kind`, `description`, `nullable`, `fields`, `required`, `open`, `items`, `artifact_type`, `doc_type`
- [x] `ContractSchema` type alias pointing to `ContractNode`
- [x] Normalization: `normalize(node)` and `ContractNode::normalize()` resolve `text`→`String` and `json`→`Any` aliases, set `open = false` on objects, `nullable = false` globally; recursive over `fields` and `items`
- [x] Validation: `validate(node)` and `ContractNode::validate()` collect all issues in a single pass without early exit
- [x] `ContractValidationIssue` with `path: String` and `message: String` — `path()` and `message()` accessors present
- [x] `ContractValidationError` with `thiserror`: `UnknownKind`, `MisplacedField`, `MissingRequiredFieldReference`
- [x] `fields` uses `BTreeMap<String, ContractNode>` (deterministic order; spec permitted `IndexMap` or sorted — `BTreeMap` satisfies the determinism requirement)
- [x] Serde implementation via `RawContractNode` intermediate: `#[serde(try_from = "RawContractNode", into = "RawContractNode")]` — enables alias handling without adding alias variants to `ContractKind`
- [x] All 10 required unit tests present with correct names
- [x] All 5 required integration tests present
- [x] Regression guard: `ContractKind` rejects `"text"` and `"json"` directly (test `contract_kind_should_not_accept_alias_variants_directly`)
- [x] `validate()` is pure/synchronous — no async, no provider boot, no network
- [x] No `unwrap()` in production code paths
- [x] TOML and JSON round-trip tests pass (confirmed by test `toml_round_trip_should_match_json_round_trip` and `artifact_ref_should_round_trip_with_artifact_type`)

## Findings

**Correctly implemented:**
- The `RawContractNode` intermediate serde bridge is an elegant solution that satisfies both the "no alias variants in the enum" requirement and the alias deserialization requirement simultaneously.
- The `kind_alias`, `nullable_explicit`, `required_explicit`, and `open_explicit` private fields on `ContractNode` allow the serialization round-trip to faithfully preserve the declared (alias) form before normalization and omit defaults from the serialized output — matching the API-SPEC.md wire shape.
- Validation correctly recurses into nested `fields` and `items` nodes and reports issues with structured dot-path notation (`fields.payload.fields`, `required[0]`, etc.).
- The `BTreeMap` choice for `fields` provides deterministic iteration order without the `indexmap` dependency, which is acceptable per the spec's anti-pattern guard ("use `IndexMap` or sort before asserting").
- All `validate()` multi-violation scenarios are tested (`validate_should_collect_multiple_issues_in_one_pass`).
- `#[serde(deny_unknown_fields)]` on `RawContractNode` prevents silent ingestion of unrecognized keys, which is consistent with ADR-042's "narrow schema" intent.

**Minor notes:**
- The spec suggested `Option<IndexMap<String, ContractNode>>` for `fields` but `Option<BTreeMap<String, ContractNode>>` was used. Both guarantee deterministic ordering. No functional difference for this module.
- The `ContractKindAlias::from_str` helper returns `Option<Self>` rather than implementing `std::str::FromStr`, which is clean since aliases are only needed during deserialization and normalization, not as a public API.

## Files Reviewed
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-types/src/contract.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-types/src/lib.rs`
