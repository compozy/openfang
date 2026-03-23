# Prompt Template

<!--
  ============================================================================
  HOW TO USE THIS TEMPLATE
  ============================================================================

  This template provides a comprehensive structure for creating high-quality
  AI prompts for software development tasks. Follow these guidelines:

  1. **Fill in ALL required sections** (marked with ⚠️ REQUIRED)
  2. **Include conditional sections** only when applicable to your task
  3. **Use inline examples** as guidance - replace with your specific content
  4. **Keep the XML-style tags** - they help structure the prompt
  5. **Be specific and detailed** - vague prompts lead to vague implementations
  6. **Reference actual files** - include real file paths and line numbers
  7. **Highlight critical patterns** - use ⚠️ for must-follow rules

  SECTIONS OVERVIEW:
  - Metadata & Identity: Task context and role definition
  - Scope & Architecture: What's included/excluded, impact analysis
  - Technical Requirements: Backend, frontend, database, Electron (conditional)
  - Implementation Guidance: Detailed examples and API specs
  - Quality & Validation: Acceptance criteria, deliverables, testing
  - Execution Plan: Step-by-step implementation guide
  - Best Practices: Patterns to follow and avoid
  - Critical Rules: MUST-READ rule files and compliance
  - References: External docs and relevant files
  - Research & Output: MCP tools usage and expected deliverables

  ============================================================================
-->

## ⚠️ TASK METADATA (Required)

```xml
<task_metadata>
  <domain>[System area: issues|projects|auth|organizations|settings|etc.]</domain>
  <type>[Task type: implementation|refactoring|bug-fix|integration|feature]</type>
  <scope>[Scope: full-stack|backend-only|frontend-only|database|electron|infrastructure]</scope>
  <complexity>[Complexity: low|medium|high]</complexity>
  <estimated_subtasks>[Number of major subtasks: e.g., 5-8]</estimated_subtasks>
</task_metadata>
```

**Example:**

```xml
<task_metadata>
  <domain>issues</domain>
  <type>refactoring</type>
  <scope>full-stack</scope>
  <complexity>high</complexity>
  <estimated_subtasks>10-12</estimated_subtasks>
</task_metadata>
```

---

## ⚠️ SECTION 1: IDENTITY & CONTEXT (Required)

### 1.1 Role Definition

```xml
<role>
You are a [role: senior frontend/backend/full-stack engineer]. You will [action: implement/refactor/fix] [what: brief description of the task] in the existing monorepo, covering [scope: backend (Elysia + PostgreSQL + Drizzle) and/or frontend (React + TanStack Router + TanStack DB)], following the [approach: established patterns/greenfield approach] and best practices from [relevant rules: .cursor/rules/*.mdc].
</role>
```

**Example:**

```xml
<role>
You are a senior full-stack engineer. You will refactor the issue archiving system to change `archived` from a status enum value to a separate boolean field, covering both backend (Elysia + PostgreSQL + Drizzle) and frontend (React + TanStack Router + TanStack DB), following the project's established patterns and greenfield approach.
</role>
```

### 1.2 Dependent Tasks

```xml
<dependent_tasks>
- Based on existing implementation: [file paths and brief description]
- Review current [feature] implementation in [specific files]
- Follows patterns from: [specific patterns or previous tasks]
</dependent_tasks>
```

**Example:**

```xml
<dependent_tasks>
- Based on existing implementation: `packages/backend/src/db/schema/issues.ts` (schema), `packages/backend/src/modules/issues/*` (API)
- Review current status update implementation in `use-update-issue.ts` and `issue-list.tsx`
- Follows patterns from: TanStack Router for subroutes, TanStack DB collections for data fetching
</dependent_tasks>
```

### 1.3 Context

```xml
<context>
- [Current state: what exists now]
- [Problem statement: why this is needed]
- [Background information: relevant history or constraints]
- [Key technical facts: schema structure, API endpoints, data flow]
</context>
```

**Example:**

```xml
<context>
- Currently `archived` is one of 5 status values: `["planned", "in_progress", "blocked", "completed", "archived"]`
- Issues are archived by changing their status to "archived" via PATCH endpoint
- Frontend has `useArchiveIssue` hook that sets status to "archived"
- Archive button only appears for completed issues
- We need to change this to a cleaner architecture where `archived` is a separate boolean field
</context>
```

---

## ⚠️ SECTION 2: SCOPE & ARCHITECTURE (Required)

### 2.1 Scope Definition

```xml
<scope>
[MVP/Full Feature], keeping the project [minimalist/comprehensive]:

**Backend Changes:**
- [Bullet list of backend changes]
- [Schema modifications]
- [API endpoint updates]
- [Business logic changes]

**Frontend Changes:**
- [Bullet list of frontend changes]
- [Component updates]
- [Hook modifications]
- [Routing changes]

**Database Migration:**
- [Migration approach: greenfield vs. migration]
- [Schema changes needed]

**What's Explicitly Excluded:**
- [Feature/behavior NOT included]
- [Future enhancements]
- [Out of scope items]
</scope>
```

**Example:**

```xml
<scope>
Greenfield refactoring (no backwards compatibility needed):

**Backend Changes:**
- Add `archived` boolean column to `issues` table with default `false`
- Remove "archived" from `issue_status` enum (keep only 4 statuses)
- Update schema to include archived field and index
- Create new database migration
- Update repository to filter out archived issues by default in list queries

**Frontend Changes:**
- Add new `/archived` subroute using TanStack Router
- Create archived issues view (flat list, no status grouping)
- Update types to include `archived: boolean` field
- Update issues collection to exclude archived by default
- Create separate archived collection for `/archived` route

**What's Explicitly Excluded:**
- No bulk archive operations in this MVP
- No archive automation or workflows
- No archive history or audit trail
</scope>
```

### 2.2 Impact Analysis

```xml
<impact_analysis>
[Detail the potential impact of this feature on existing components, services, and data stores]

| Affected Component | Type of Impact | Risk Level | Required Action |
| ------------------ | -------------- | ---------- | --------------- |
| [component/file]   | [change type]  | [Low/Med/High] | [action needed] |

**Categories to consider:**
- **Direct Dependencies**: Modules that will call or be called by this feature
- **Shared Resources**: Database tables, caches, queues used by multiple components
- **API Changes**: Modifications to existing endpoints or contracts (breaking/non-breaking)
- **Performance Impact**: Components that might experience load changes
- **UI Components**: Visual components that need updates
</impact_analysis>
```

**Example:**

```xml
<impact_analysis>
| Affected Component | Type of Impact | Risk Level | Required Action |
| ------------------ | -------------- | ---------- | --------------- |
| `issues` table schema | Schema Change (Breaking) | Medium | Run migration, regenerate types |
| `issue_status` enum | Enum Change (Breaking) | Medium | Update all status checks |
| `issues-collection.ts` | API Change (Non-breaking) | Low | Update filter logic |
| `use-archive-issue.ts` | Implementation Change | Low | Change from status to boolean |
| `issue-list.tsx` | UI Change (Non-breaking) | Low | Remove "archived" status section |
| API clients consuming issues | Response Change | Medium | Notify frontend team to regenerate types |
</impact_analysis>
```

### 2.3 Technical Considerations

```xml
<technical_considerations>
**Key Architectural Decisions:**
- [Decision 1: what was decided and why]
- [Decision 2: rationale and benefits]

**Trade-offs Considered:**
- [Trade-off 1: what was weighed]
- [Trade-off 2: chosen approach and why]

**Alternatives Rejected:**
- [Alternative 1: why not chosen]
- [Alternative 2: disadvantages]

**Known Risks & Mitigation:**
- [Risk 1: description] → Mitigation: [approach]
- [Risk 2: description] → Mitigation: [approach]

**Performance Considerations:**
- [Performance impact if applicable]
- [Optimization strategies]

**Security Considerations:**
- [Security implications if applicable]
- [Validation requirements]
</technical_considerations>
```

**Example:**

```xml
<technical_considerations>
**Key Architectural Decisions:**
- Separate boolean field instead of status enum value: Clearer separation between workflow state and visibility state
- Dedicated archive/unarchive endpoints: Makes intent explicit, easier to add validation

**Trade-offs Considered:**
- Boolean vs. soft delete (deletedAt timestamp): Boolean is simpler for binary state
- Single collection with filter vs. separate archived collection: Separate collection provides better performance and clearer intent

**Alternatives Rejected:**
- Keep "archived" in status enum: Mixing workflow and visibility concerns, unclear semantics
- Add "archived_at" timestamp only: Boolean is sufficient for MVP, timestamp adds unnecessary complexity

**Known Risks & Mitigation:**
- Migration might fail on large datasets → Mitigation: Run in development first, test migration script
- Frontend might show stale data during migration → Mitigation: Clear query cache after migration

**Performance Considerations:**
- Add index on `archived` column for efficient filtering
- Composite index on (projectId, archived) for project-specific queries
</technical_considerations>
```

---

## SECTION 3: TECHNICAL REQUIREMENTS (Conditional - Include as Needed)

### 3.1 Backend Requirements (Include if backend changes needed)

```xml
<backend_requirements>
- **Language/Stack**: TypeScript, Elysia 1.x, Drizzle ORM, PostgreSQL, Zod validation

- **Schema Changes** (`path/to/schema/file.ts`):
  - [Specific changes with code examples or descriptions]
  - [Index additions]
  - [Constraint changes]

- **Migration**:
  - Create new migration file in `packages/backend/drizzle/`
  - [Specific migration steps]
  - [Commands to run: bun run db:generate, bun run db:migrate]

- **Repository Changes** (`path/to/repository.ts`):
  - Update `method()` to [description]
  - Add new methods: [list with signatures]
  - Update mapping functions

- **Model/Types Changes** (`path/to/model.ts`):
  - [Type additions/modifications]
  - [Schema updates]

- **Route Changes** (`path/to/route.ts`):
  - Keep/Update existing endpoints: [list]
  - Add new endpoints: [list with details]
  - Request/response schemas

- **API Endpoints**:
  - `METHOD /path/:param`
    - Body: [schema]
    - Response: [schema]
    - Errors: [list status codes and conditions]

- **Validation**:
  - [Validation rules]
  - [Business logic constraints]

- **Error Codes**:
  - `400` - [condition]
  - `404` - [condition]
  - `409` - [condition]
  - `422` - [condition]
  - `500` - [condition]
</backend_requirements>
```

### 3.2 Frontend Requirements (Include if frontend changes needed)

```xml
<frontend_requirements>
- **Stack**: TypeScript, React 19, TanStack (Router, DB, Query, Forms), lucide-react, ShadCN

- **File Naming Convention**: kebab-case for all React files

- **Routing Changes**:
  - Update/Create routes in `path/to/route.tsx`
  - Parent route: [description]
  - Child routes: [list with paths]
  - Uses `<Outlet />` pattern for nested routes

- **Type Changes**:
  - `path/to/generated/api-schema.ts` is **auto-generated** from backend OpenAPI
  - After backend changes, regenerate: `cd packages/electron && bun run generate:api-schema`
  - This automatically updates: [list what changes]

- **Collection Changes**:
  - Update `path/to/collection.ts`:
    - [Specific changes]
  - Create new collection: `path/to/new-collection.ts`
    - [Purpose and configuration]

- **Hook Changes**:
  - Update `path/to/use-hook.ts`:
    - [Changes needed]
  - Create new hooks: [list with purposes]
  - ⚠️ **CRITICAL**: Always pass collection from data hook to mutation hook (no new instances!)

- **Component Changes**:
  - Update `path/to/component.tsx`:
    - [Specific changes with line number references]
  - Create new components: [list]
  - Component patterns to follow: [reference existing patterns]

- **UI/UX Requirements**:
  - [Visual requirements]
  - [Interaction patterns]
  - [Accessibility requirements: aria-labels, keyboard navigation]
  - [Design tokens only: bg-background, text-foreground (NEVER bg-white, text-black)]

- **State Management**:
  - [If using Zustand, describe store changes]
  - [Query invalidation strategy]
</frontend_requirements>
```

---

## SECTION 4: IMPLEMENTATION GUIDANCE (Conditional - Include for Complex Tasks)

### 4.1 Implementation Details

````xml
<implementation_details>
[For complex patterns, provide detailed code examples. Keep examples focused and ≤30 lines]

## Example: New Hook Pattern

```typescript
// packages/path/to/use-hook.ts

import type { createCollection } from "../db/collection";

type Collection = ReturnType<typeof createCollection>;

/**
 * [Hook description and purpose]
 *
 * **Important**: You must pass the same collection instance that's being used
 * to display the data. Creating a new collection instance will result in
 * "key not found" errors.
 *
 * Usage:
 * ```tsx
 * const { data, collection } = useData();
 * const mutate = useMutate(collection); // Pass collection!
 *
 * mutate({ id: '123' });
 * ```
 */
export function useMutate(collection: Collection) {
  return ({ id }: { id: string }) => {
    try {
      collection.update(id, draft => {
        draft.field = "new-value";
        draft.updatedAt = new Date().toISOString();
      });
    } catch (error) {
      console.error("Failed to mutate:", error);
      throw error;
    }
  };
}
````

## Example: Component Update

[Show specific component changes with line number references and before/after if helpful]
</implementation_details>

````

### 4.2 API Endpoints Specification

```xml
<api_endpoints>
**Existing (Updated)**:
````

METHOD /path/:param

- Description: [brief description]
- Query params: [list if applicable]
- Request body: [schema or type]
- Response: [schema or type]
- Errors: [status codes with conditions]

```

**New Endpoints**:
```

METHOD /path/:param/action

- Description: [brief description]
- Request body: [schema or type]
- Response: [schema or type]
- Validation: [business rules]
- Errors: [status codes with conditions]

```

**Authentication**: [if required, specify auth headers/tokens]
</api_endpoints>
```

### 4.3 Request Examples

````xml
<request_examples>
**Example 1: [Action Description]**
```bash
curl -X METHOD http://localhost:PORT/api/path \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <TOKEN>" \
  -d '{"field": "value"}'
````

**Example 2: [Action Description]**

```bash
curl http://localhost:PORT/api/path?query=value
```

**Response Format**:

```json
{
  "ok": true,
  "data": {
    "field": "value"
  }
}
```

**Error Response**:

```json
{
  "ok": false,
  "error": {
    "code": "ERROR_CODE",
    "message": "Human-readable message"
  }
}
```

</request_examples>

````

---

## ⚠️ SECTION 5: QUALITY & VALIDATION (Required)

### 5.1 Acceptance Criteria

```xml
<acceptance_criteria>
[Success conditions organized by layer - be specific and measurable]

**Backend**:
- [Specific backend criteria]
- [API behavior requirements]
- [Data persistence verification]
- [Error handling requirements]

**Frontend**:
- [UI behavior requirements]
- [User interaction flows]
- [Visual consistency requirements]
- [Accessibility requirements]
- [Performance requirements: loading states, etc.]

**Database**:
- [Schema correctness]
- [Index creation]
- [Data integrity]
- [Migration success]

**Testing**:
- [All tests pass: lint, typecheck, test]
- [No console errors or warnings]
- [Manual testing scenarios completed]
- [Edge cases verified]

**Quality**:
- [ ] Code follows all rule files (.cursor/rules/*.mdc)
- [ ] No "key not found" errors in collections
- [ ] Proper loading/error states
- [ ] No regression in existing features
</acceptance_criteria>
````

### 5.2 Deliverables

```xml
<deliverables>
[Checkable list of artifacts that constitute "done"]

**Code**:
- [ ] Schema migration created and applied
- [ ] Backend API endpoints implemented
- [ ] Frontend types regenerated (if applicable)
- [ ] Hooks created/updated following collection sharing pattern
- [ ] Components updated with proper accessibility

**Documentation**:
- [ ] Code comments for complex logic
- [ ] API endpoint documentation (if new endpoints)
- [ ] README updates (if configuration changes)

**Testing**:
- [ ] Unit tests added/updated
- [ ] Integration tests passing
- [ ] Manual testing completed

**Quality Checks** (MANDATORY):
- [ ] `pnpm run lint` passes with no errors
- [ ] `pnpm run typecheck` passes with no errors
- [ ] `pnpm run test` passes with no failures
- [ ] No console errors in browser/terminal
- [ ] Follows all .cursor/rules/*.mdc files
</deliverables>
```

### 5.3 Testing Strategy

```xml
<testing_strategy>
**Unit Tests**:
- **Components to test**: [list]
- **Mock requirements**: [external services, collections, etc.]
- **Critical scenarios**:
  - [ ] [Scenario 1: happy path]
  - [ ] [Scenario 2: error path]
  - [ ] [Scenario 3: edge case]

**Integration Tests**:
- **End-to-end flows**: [describe full user flows]
- **Data consistency checks**: [verify data persistence]
- **Test scenarios**:
  - [ ] [Flow 1]
  - [ ] [Flow 2]

**Manual Testing Checklist**:
- [ ] [User action 1] → [Expected result]
- [ ] [User action 2] → [Expected result]
- [ ] [Error scenario 1] → [Expected error message/behavior]
- [ ] [Edge case 1] → [Expected handling]
- [ ] Verify with different data sets
- [ ] Test with network disconnected (if applicable)
- [ ] Test keyboard navigation and screen readers

**Performance Testing** (if applicable):
- [ ] Page load time acceptable
- [ ] No memory leaks during repeated actions
- [ ] Query performance acceptable with large datasets
</testing_strategy>
```

---

## ⚠️ SECTION 6: EXECUTION PLAN (Required)

````xml
<suggested_steps>
[Numbered, detailed implementation steps grouped by layer. Include specific file paths and commands]

1. **Backend - Database Schema**
   - Update `packages/backend/src/db/schema/file.ts`:
     - [Specific change 1]
     - [Specific change 2]
   - Generate migration: `cd packages/backend && pnpm run db:generate`
   - Review generated migration in `packages/backend/drizzle/`
   - Apply migration: `pnpm run db:migrate`
   - Verify schema: `pnpm run db:studio` (opens Drizzle Studio)

2. **Backend - Repository Layer**
   - Update `packages/backend/src/modules/domain/repository.ts`:
     - Add field to `mapEntity()` function (line X)
     - Update `list()` method to [description]
     - Add new methods: [list with signatures]
   - Update `packages/backend/src/modules/domain/model.ts`:
     - [Type changes]

3. **Backend - API Routes**
   - Update `packages/backend/src/modules/domain/route.ts`:
     - Add new endpoints: [list]
     - Update existing endpoints: [list]
     - Add validation: [details]
   - Update use cases if needed
   - Test endpoints manually with curl

4. **Backend - Testing & Verification**
   - Run `pnpm run typecheck` to verify types
   - Run `pnpm run test` to ensure existing tests pass
   - Manually test new endpoints with curl/Postman
   - Verify error responses

5. **Frontend - Types (Auto-Generated)**
   - Regenerate API schema from backend OpenAPI spec:
     ```bash
     cd packages/electron
     pnpm run generate:api-schema
     ```
   - Verify generated types in `packages/electron/src/generated/api-schema.ts`
   - Check that [specific types] have [expected fields]

6. **Frontend - Data Layer (Collections & Hooks)**
   - Update `packages/electron/src/renderer/src/systems/domain/db/collection.ts`:
     - [Specific changes]
   - Create new collection if needed: `path/to/new-collection.ts`
   - Update hooks: `path/to/use-hook.ts`
     - ⚠️ Ensure collection sharing pattern (pass collection as param)
   - Create new hooks: [list]

7. **Frontend - Routing** (if applicable)
   - Update `path/to/route.tsx`:
     - [Route changes]
   - Create layout component if needed: `path/to/layout.tsx`
   - Update `main.tsx` to register nested routes

8. **Frontend - Components**
   - Update existing components: `path/to/component.tsx`
     - [Specific changes with line references]
   - Create new components: [list]
   - Follow ShadCN patterns and design tokens

9. **Frontend - Testing & Verification**
   - Run `pnpm run lint` to check code quality
   - Run `pnpm run typecheck` to verify types
   - Run `pnpm run test` for unit tests
   - Manual testing: [list scenarios]

10. **Integration Testing**
    - Test full flow: [describe end-to-end flow]
    - Verify data consistency
    - Check collection updates properly
    - Test with multiple users/projects (if applicable)
    - Verify no console errors

11. **Quality Checks** (MANDATORY - DO NOT SKIP)
    - Run `pnpm run lint` - must pass
    - Run `pnpm run typecheck` - must pass
    - Run `pnpm run test` - must pass
    - Manual smoke testing of all functionality
    - Verify no breaking changes to existing features
    - Check browser console for errors/warnings

12. **Documentation & Cleanup**
    - Add comments for complex logic
    - Update README if configuration changed
    - Clean up any debug code or console.logs
    - Review all changes one final time
</suggested_steps>
````

---

## ⚠️ SECTION 7: BEST PRACTICES & ANTI-PATTERNS (Required)

### 7.1 Best Practices

```xml
<best_practices>
[Patterns to follow with clear explanations - reference project patterns]

- **Greenfield Approach** (if applicable): Don't worry about backwards compatibility - make clean architectural decisions for alpha phase

- **Collection Sharing** (CRITICAL for TanStack DB):
  - ALWAYS pass collection from data hook to mutation hook
  - Creating new collection instances = "key not found" errors
  - Example: `const { data, collection } = useData(); useMutate(collection);`

- **Optimistic Updates**:
  - Provide instant UI feedback
  - TanStack DB handles automatic rollback on errors
  - Use `collection.update()` for optimistic mutations

- **Type Safety**:
  - Leverage TypeScript to catch issues early
  - Regenerate types after backend changes
  - Use strict null checks

- **Database Design**:
  - Add indexes on frequently queried columns
  - Use composite indexes for multi-column queries
  - Consider query performance implications

- **API Design**:
  - Separate endpoints for different concerns (clear intent)
  - Use appropriate HTTP methods (GET, POST, PATCH, DELETE)
  - Return consistent error response formats

- **Error Handling**:
  - Proper error states in UI
  - Graceful degradation
  - User-friendly error messages
  - Automatic rollback mechanisms

- **Accessibility**:
  - Include aria-labels for icon-only buttons
  - Keyboard navigation support
  - Screen reader compatibility

- **Design Tokens Only** (CRITICAL):
  - ALWAYS use design tokens: `bg-background`, `text-foreground`, etc.
  - NEVER use explicit colors: `bg-white`, `text-black`, `bg-gray-500`

- **File Naming**:
  - React/frontend: kebab-case (e.g., `use-archive-issue.ts`, `issue-list.tsx`)
  - Follow project conventions consistently

- **Testing**:
  - Test at each layer (unit, integration, manual)
  - Cover edge cases and error paths
  - Run quality checks before completion
</best_practices>
```

### 7.2 Anti-Patterns (Should Not)

```xml
<should_not>
[Explicit prohibitions - what NOT to do]

**Collection Management**:
- Don't create new collection instances in mutation hooks (causes "key not found")
- Don't bypass optimistic updates (use collection.update(), not direct API)

**Data & State**:
- Don't skip database migrations (schema must match code)
- Don't hardcode IDs, URLs, or configuration values
- Don't use manual cache updates when TanStack DB provides automatic updates

**API & Backend**:
- Don't mix concerns in single endpoint (e.g., archive + status update together)
- Don't skip validation (validate inputs on both frontend and backend)
- Don't return inconsistent response formats

**Frontend & UI**:
- Don't use explicit colors (bg-white, text-black) - use design tokens only
- Don't forget aria-labels for accessibility
- Don't create components that mix too much logic with rendering
- Don't skip error handling in UI

**Code Quality**:
- Don't use workarounds - implement proper solutions
- Don't skip quality checks (lint, typecheck, test)
- Don't ignore existing patterns - follow established conventions

**Testing**:
- Don't skip manual testing of user flows
- Don't ignore edge cases
- Don't forget to test error scenarios

**General**:
- Don't sacrifice code quality for backwards compatibility (if greenfield/alpha)
- Don't make assumptions - verify with actual code/data
- Don't forget to invalidate queries after mutations
- Don't commit commented-out code or debug logs
</should_not>
```

---

## ⚠️ SECTION 8: CRITICAL RULES & COMPLIANCE (Required - Read First!)

### 8.1 Mandatory Rule Files

```xml
<rules>
**⚠️ CRITICAL: Read these rule files BEFORE starting implementation**

These rules are MANDATORY and enforced. Violating them results in immediate task rejection.

**🔴 MUST READ FIRST (Most Critical)**:
2. `@.cursor/rules/data-fetch.mdc` - Collection sharing pattern (prevents "key not found" errors)

**Backend Rules** (include if backend work):
3. `@.cursor/rules/elysia.mdc` - Elysia API patterns and best practices

**Frontend Rules** (include if frontend work):
4. `@.cursor/rules/react.mdc` - React component patterns and hooks
5. `@.cursor/rules/tanstack-router.mdc` - TanStack Router nested routing
6. `@.cursor/rules/shadcn.mdc` - ShadCN component usage patterns
7. `@.cursor/rules/tailwindcss.mdc` - Design tokens (NEVER bg-white, ALWAYS bg-background)
8. `@.cursor/rules/zustand.mdc` - Global state management (if needed)
9. `@.cursor/rules/tanstack-forms.mdc` - Form handling patterns (if needed)

**Additional Rules** (include if applicable):
10. `@.cursor/rules/[other-relevant-rule].mdc` - [description]

**🔴 Most Critical Patterns (Cannot Be Violated)**:
- ⚠️ **Collection Sharing**: ALWAYS share the same collection instance between hooks
  - Creating new collection instances causes "key not found" errors
  - Pattern: `const { data, collection } = useData(); useMutate(collection);`

- ⚠️ **No Linebreaks**: Functions must have NO extra linebreaks between statements
  - ✅ Correct: `function foo() { const x = 1; return x; }`
  - ❌ Wrong: `function foo() { const x = 1;\n\n return x; }`

- ⚠️ **Design Tokens**: NEVER use explicit colors, ALWAYS use design tokens
  - ✅ Correct: `bg-background`, `text-foreground`, `border-border`
  - ❌ Wrong: `bg-white`, `text-black`, `bg-gray-500`

- ⚠️ **Quality Checks**: MUST run before completing ANY task
  - `pnpm run lint` - must pass
  - `pnpm run typecheck` - must pass
  - `pnpm run test` - must pass

**Enforcement**:
Violating these standards results in immediate task rejection. Code that doesn't follow these rules will NOT be accepted.

**Before Starting**:
Read the relevant rule files listed above. They contain detailed patterns and examples you MUST follow.
</rules>
```

### 8.2 Standards Compliance Checklist

```xml
<standards_compliance>
[Explicit checklist to confirm rule adherence - check before completion]

**Code Quality**:
- [ ] Functions are concise and focused (single responsibility)
- [ ] No commented-out code or debug statements
- [ ] Proper error handling throughout

**Data & State Management**:
- [ ] Follows data-fetch.mdc (collection sharing pattern)
- [ ] No new collection instances in mutation hooks
- [ ] Proper optimistic updates with rollback
- [ ] Query invalidation after mutations

**UI & Styling**:
- [ ] Uses design tokens only (bg-background, NOT bg-white)
- [ ] Follows shadcn.mdc component patterns
- [ ] Proper accessibility (aria-labels, keyboard navigation)
- [ ] Responsive design if applicable

**File Naming & Structure**:
- [ ] Follows kebab-case for React/frontend files
- [ ] Components in proper directory structure
- [ ] Hooks named with 'use' prefix
- [ ] Clear, descriptive file names

**Backend** (if applicable):
- [ ] Follows elysia.mdc patterns
- [ ] Proper request/response validation
- [ ] Consistent error response format
- [ ] Database indexes added for queries

**Testing & Quality**:
- [ ] `pnpm run lint` passes with no errors
- [ ] `pnpm run typecheck` passes with no errors
- [ ] `pnpm run test` passes all tests
- [ ] Manual testing completed
- [ ] No console errors or warnings

**Documentation**:
- [ ] Complex logic has comments
- [ ] API changes documented
- [ ] README updated if needed
</standards_compliance>
```

---

## SECTION 9: REFERENCES & FILE CONTEXT (Conditional - Include as Helpful)

### 9.1 References

```xml
<references>
[External documentation, rule files, and existing implementations]

**External Documentation**:
- [Library Name]: [URL to docs]
- [API/Service]: [URL to docs]
- [Pattern/Standard]: [URL to reference]

**Internal Rule Files**:
- `.cursor/rules/[rule-name].mdc` - [Brief description]
- `.cursor/rules/[rule-name].mdc` - [Brief description]

**Existing Implementations** (as reference):
- `path/to/existing-file.ts` - [What pattern to follow]
- `path/to/existing-component.tsx:123-145` - [Specific example with line numbers]

**Project Documentation**:
- `README.md` - [Relevant section]
- `ARCHITECTURE.md` - [Relevant pattern]
</references>
```

**Example:**

```xml
<references>
**External Documentation**:
- Drizzle ORM Migrations: https://orm.drizzle.team/docs/migrations
- PostgreSQL Enums: https://www.postgresql.org/docs/current/datatype-enum.html
- TanStack Router Nested Routes: https://tanstack.com/router/latest/docs/framework/react/guide/route-trees
- TanStack DB Collections: https://tanstack.com/db/latest

**Internal Rule Files**:
- `@.cursor/rules/elysia.mdc` - Elysia route patterns and best practices
- `@.cursor/rules/data-fetch.mdc` - TanStack DB collection sharing (CRITICAL!)
- `@.cursor/rules/react.mdc` - React component and hook patterns
- `@.cursor/rules/tanstack-router.mdc` - Nested routing with Outlet pattern

**Existing Implementations**:
- `packages/backend/src/modules/issues/repository.ts` - Repository pattern reference
- `packages/electron/src/renderer/src/systems/issues/hooks/use-update-issue.ts` - Hook pattern
- `packages/electron/src/renderer/src/systems/issues/components/issue-list.tsx:103-116` - ItemActions component pattern
</references>
```

### 9.2 Relevant Files (Directly Modified)

```xml
<relevant_files>
[Files that will be directly modified by this task - provide full paths]

**Backend**:
- packages/backend/src/db/schema/[file].ts - [What changes]
- packages/backend/drizzle/[migration].sql - [What changes]
- packages/backend/src/modules/[domain]/repository.ts - [What changes]
- packages/backend/src/modules/[domain]/route.ts - [What changes]

**Frontend**:
- packages/electron/src/generated/api-schema.ts - [Auto-regenerated]
- packages/electron/src/renderer/src/systems/[domain]/[file].tsx - [What changes]
- packages/electron/src/renderer/src/systems/[domain]/hooks/[file].ts - [What changes]
</relevant_files>
```

### 9.3 Dependent Files (Files That Depend on Changes)

```xml
<dependent_files>
[Files that depend on the changes but may not be directly modified]

**Backend**:
- packages/backend/src/db/index.ts - Database client
- packages/backend/src/modules/[domain]/usecases.ts - May need updates

**Frontend**:
- packages/electron/src/lib/api/* - API client
- packages/electron/src/lib/query-client.ts - TanStack Query config
- packages/electron/src/renderer/src/[other-components].tsx - Consumers of changed data
</dependent_files>
```

---

## ⚠️ SECTION 10: RESEARCH & OUTPUT (Required)

### 10.1 External Research Tools

```xml
<perplexity>
[Guidance on when and how to use Perplexity MCP and Context7 for up-to-date information]

**When to Use**:
- Need latest library versions or API changes
- Research best practices for new patterns
- Verify migration strategies
- Check for breaking changes in dependencies
- Find official documentation for features

**Perplexity MCP Usage**:
- Use detailed prompts, not search-style queries
- Example: "Explain the best practices for TanStack Router nested routes with TypeScript, including how to properly type route parameters and use Outlet components"
- Can specify recency: day, week, month, year

**Context7 MCP Usage** (Two-step process):
1. First, resolve library ID:
   - Tool: `resolve-library-id`
   - Input: `"react-router"` or `"@tanstack/router"`
   - Output: Context7-compatible ID (e.g., `/tanstack/router`)

2. Then, get documentation:
   - Tool: `get-library-docs`
   - Input: Library ID from step 1
   - Optional: Specify topic to focus on
   - Output: Up-to-date documentation

**IMPORTANT**:
- MUST use these tools when dealing with external libraries
- NEVER rely only on model's training data for library-specific details
- Always verify current API signatures and patterns
</perplexity>
```

### 10.2 Greenfield Approach Reminder

```xml
<greenfield>
**YOU SHOULD ALWAYS** have in mind that this should be done in a greenfield approach.

- Project is in **alpha phase** - no production users
- **No backwards compatibility** required
- Supporting both old and new approaches just introduces complexity
- **Never sacrifice quality** because of backwards compatibility concerns
- Make clean architectural decisions without legacy constraints
- If a refactor makes the code better, do it - don't preserve old patterns

**Exception**: If the prompt explicitly states backwards compatibility is needed, follow those specific instructions.
</greenfield>
```

### 10.3 Output Requirements

```xml
<output>
[Define what should be delivered after implementation]

**Code Implementation**:
- All modified files with complete implementations
- New files following project structure and conventions
- Proper imports and exports

**Summary & Documentation**:
1. **Brief Summary**: 2-3 paragraphs describing what was implemented
2. **Key Decisions**: Important architectural or technical decisions made during implementation
3. **Testing Results**:
   - Quality check results (`pnpm run lint`, `pnpm run typecheck`, `pnpm run test`)
   - Manual testing outcomes
   - Edge cases discovered and how they were handled
4. **API Changes** (if applicable): List of new/modified endpoints with examples
5. **Migration Notes** (if applicable): Steps taken for database migration

**Issues & Discoveries**:
- Any edge cases discovered during implementation
- Challenges faced and how they were resolved
- Potential future improvements identified

**Verification**:
- Screenshot or console output showing tests passing
- Confirmation that all deliverables are complete
- No outstanding errors or warnings

**Format**:
- Clear markdown formatting
- Code examples in proper code blocks
- Tables for structured information
- Links to relevant files with line numbers

**MANDATORY**:
Before marking task complete, verify ALL these have been delivered and `pnpm run lint && pnpm run typecheck && pnpm run test` passes.
</output>
```

---

## TEMPLATE USAGE CHECKLIST

Before submitting a prompt created from this template, verify:

- [ ] **All required sections included** (marked with ⚠️ REQUIRED)
- [ ] **Conditional sections** included only if applicable
- [ ] **Specific file paths** provided (not generic "update the file")
- [ ] **Line number references** for existing code changes
- [ ] **Concrete examples** instead of vague descriptions
- [ ] **Critical rules** highlighted with ⚠️ symbols
- [ ] **Deliverables** are checkable and measurable
- [ ] **Testing strategy** includes unit, integration, and manual tests
- [ ] **Quality checks** explicitly required (lint, typecheck, test)
- [ ] **Standards compliance** checklist included
- [ ] **Research tools** mentioned if external libraries involved
- [ ] **Greenfield approach** reminder included (if applicable)

---

## ADDITIONAL NOTES FOR TEMPLATE USERS

### 📝 Writing Effective Prompts

1. **Be Specific**: Don't say "update the component" - say "Update `issue-list.tsx` line 103-116 to add Archive button in ItemActions"

2. **Provide Context**: Explain WHY changes are needed, not just WHAT to change

3. **Reference Patterns**: Point to existing code as examples: "Follow the pattern in `use-update-issue.ts`"

4. **Include Line Numbers**: When referencing specific code, include line numbers

5. **Highlight Constraints**: Make business rules and constraints explicit

6. **Think About Impact**: Consider what components/files will be affected

### 🎯 When to Use Each Section

- **Use `<impact_analysis>`** when changes affect multiple components or have migration risk
- **Use `<implementation_details>`** for complex patterns that need code examples
- **Use `<technical_considerations>`** when there are trade-offs or important decisions
- **Use `<electron_requirements>`** only when IPC/main process changes needed
- **Use `<perplexity>`** when dealing with external libraries or need latest docs

### ⚠️ Common Mistakes to Avoid

1. **Too Vague**: "Update the issues system" → Should specify exact files and changes
2. **No Testing Strategy**: Just saying "test it" instead of specific test cases
3. **Forgetting Quality Checks**: Not requiring lint/typecheck/test to pass
4. **Unclear Acceptance Criteria**: Vague success conditions instead of measurable outcomes

### 📚 Template Maintenance

- Keep this template in sync with project evolution
- Update examples when new patterns emerge
- Add new conditional sections as needs arise
- Review and refine based on prompt effectiveness

---

**Version**: 1.0
**Last Updated**: [Date]
**Maintained By**: [Team/Person]
