## markdown

## status: completed

<task_context>
<domain>openfang-kernel</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>medium</complexity>
<dependencies>none</dependencies>
</task_context>

# Task 47.0: Minijinja Template Engine Migration

## Overview

Replace the custom `TemplateSegment`-based template renderer in the workflow engine with minijinja. The current implementation only supports simple path references (`{{ input.field }}`, `{{ vars.symbol }}`). Minijinja enables conditionals, filters, default values, and iteration in workflow step inputs — all of which were planned in the original PRD design decisions.

<critical>
- **ALWAYS READ** @CLAUDE.md before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-gaps/techspec.md` (Gap 6) before start
- **YOU CAN ONLY** finish when `make fmt`, `make lint`, and `make test` pass at 100%
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Add `minijinja` dependency to `openfang-kernel` via `cargo add`
- Replace `WorkflowEngine::render_template()` to use minijinja internally
- Maintain `{{ input.* }}` and `{{ vars.* }}` namespace conventions (backward compatible)
- All existing template tests must pass without modification (proves backward compat)
- Add new tests for conditionals, filters, and default values
- Simplify the compiler phase to store raw template source instead of tokenizing into segments
- Keep `CompiledTemplate` and `TemplateSegment` types for backward compatibility of serialized IR
</requirements>

## Subtasks

- [x] 47.1 Add `minijinja` dependency to `openfang-kernel` via `cargo add minijinja`
- [x] 47.2 Create `TemplateRenderer` struct wrapping `minijinja::Environment`
- [x] 47.3 Replace `WorkflowEngine::render_template()` to delegate to `TemplateRenderer`
- [x] 47.4 Update `resolve_step_input()` to use new renderer
- [x] 47.5 Update `workflow_compiler.rs` — add source-only `CompiledTemplate` construction path (skip tokenization for new definitions)
- [x] 47.6 Ensure `CompiledTemplate` with existing `segments` still deserializes correctly (backward compat)
- [x] 47.7 Run all existing template tests — must pass unchanged (regression gate)
- [x] 47.8 Add tests for conditional templates: `{% if vars.status == "ok" %}...{% endif %}`
- [x] 47.9 Add tests for filter usage: `{{ vars.name | upper }}`, `{{ vars.x | default("fallback") }}`
- [x] 47.10 Add tests for iteration: `{% for item in vars.items %}...{% endfor %}`
- [x] 47.11 Add test for error handling: invalid template syntax produces `OpenFangError::Internal`
- [x] 47.12 Run `make fmt && make lint && make test` — all must pass

## Implementation Details

### Current Rendering Pipeline

```
Definition YAML/TOML
  → workflow_compiler.rs: tokenize "{{ input.x }}" into [Text, Reference] segments
  → CompiledTemplate { source, segments }
  → workflow.rs: render_template() iterates segments, resolves References
```

### New Rendering Pipeline

```
Definition YAML/TOML
  → workflow_compiler.rs: store raw source string in CompiledTemplate
  → CompiledTemplate { source, segments: [] }  (segments empty for new defs)
  → workflow.rs: render via minijinja with input + vars as context
```

### Core Interface

```rust
use minijinja::Environment;

struct TemplateRenderer;

impl TemplateRenderer {
    fn render(
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

### Backward Compatibility

The `{{ input.field }}` and `{{ vars.symbol }}` syntax is valid minijinja. Existing workflow definitions will render identically. The only change is that minijinja also supports:
- `{% if %}` / `{% else %}` / `{% endif %}`
- `{{ value | filter }}`
- `{{ value | default("fallback") }}`
- `{% for item in list %}` / `{% endfor %}`

### vars Namespace Change

Current `vars` is `HashMap<String, String>` (string values only). For minijinja to support filters and iteration, `vars` should accept `serde_json::Value`. This change is internal to the renderer — the workflow engine already stores vars as JSON in the database.

### Relevant Files

- `crates/openfang-kernel/src/workflow.rs` — `render_template()`, `resolve_step_input()`
- `crates/openfang-kernel/src/workflow_compiler.rs` — Template compilation phase
- `crates/openfang-types/src/workflow.rs` — `CompiledTemplate`, `TemplateSegment` types

### Dependent Files

- `crates/openfang-kernel/Cargo.toml` — needs `minijinja` dependency
- `crates/openfang-kernel/tests/workflow_integration_test.rs` — may need new test cases

## Deliverables

- Minijinja-powered template rendering in workflow engine
- Backward-compatible with all existing `{{ }}` templates
- New capabilities: conditionals, filters, defaults, iteration
- All existing tests pass unchanged
- New tests for advanced template features
- `cargo add minijinja` to `openfang-kernel`

## Tests

### Unit Tests (Required)

- [x] Simple reference: `{{ input }}` renders to input value
- [x] Nested reference: `{{ input.nested.field }}` resolves JSON path
- [x] Vars reference: `{{ vars.symbol }}` resolves stored variable
- [x] Mixed: `Hello {{ vars.name }}, process {{ input }}` renders correctly
- [x] Conditional: `{% if vars.status == "ok" %}yes{% else %}no{% endif %}` renders branch
- [x] Filter: `{{ vars.name | upper }}` renders uppercase
- [x] Default: `{{ vars.missing | default("fallback") }}` renders fallback
- [x] Iteration: `{% for x in vars.items %}{{ x }},{% endfor %}` renders list
- [x] Error: Invalid syntax `{{ unclosed` returns `OpenFangError::Internal`
- [x] Empty template: `""` renders to `""`

### Integration Tests (Required)

- [x] All existing workflow template tests pass (regression gate — run before AND after changes)
- [x] Workflow with conditional step input executes correctly end-to-end
- [x] Serialized `CompiledTemplate` with segments (old format) still works

### Regression and Anti-Pattern Guards

- [x] Existing template test assertions unchanged (proves backward compatibility)
- [x] No `TemplateSegment` processing code removed yet (deprecate, don't delete)
- [x] No test-only production APIs introduced

### Verification Commands

- [x] `make fmt`
- [x] `make lint`
- [x] `make test`

## Success Criteria

- All existing template tests pass without modification
- New template features (conditionals, filters, defaults) work in step inputs
- `CompiledTemplate` backward compatible with serialized IR
- Zero warnings, zero errors, zero test failures

---

## Notes

- This task is independent of tasks 44-46 and can be executed in parallel
- The `TemplateSegment` type should be kept but deprecated — removal is a future cleanup task
- Minijinja is a lightweight, zero-dependency Jinja2 implementation for Rust (< 100KB)
