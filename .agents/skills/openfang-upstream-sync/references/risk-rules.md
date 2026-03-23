# Upstream Sync Risk Rules

Use these rules to interpret the analyzer output and decide whether a draft
question PR is required before integrating `upstream/main`.

## Risk Levels

### `low`

Typical signals:

- documentation-only changes
- examples, static assets, or isolated tooling
- no overlap with fork-modified files
- no config, schema, API, or runtime changes

Default action:

- proceed directly to the sync branch

### `medium`

Typical signals:

- shared code paths changed without obvious schema or contract break
- moderate overlap with files already modified in the fork
- high commit count or high changed-file count
- integration surfaces such as channels, extensions, docs, or workflows changed

Default action:

- open a draft question PR first

### `high`

Typical signals:

- runtime, kernel, API, memory, CLI, or types changed
- migrations, SQL, schemas, or config contracts changed
- dependency graph or workspace manifests changed
- overlap touches infrastructure files already heavily customized in the fork

Default action:

- open a draft question PR first

## Sensitive Areas For This Fork

Treat these areas as sensitive by default:

- `Cargo.toml`
- `Cargo.lock`
- `Makefile`
- `xtask/`
- `crates/openfang-api/`
- `crates/openfang-cli/`
- `crates/openfang-kernel/`
- `crates/openfang-memory/`
- `crates/openfang-runtime/`
- `crates/openfang-types/`
- `crates/openfang-migrate/`
- `crates/arky-config/`
- `crates/arky-provider/`
- `crates/arky-protocol/`
- `crates/arky-session/`

## Escalation Rule

If the risk level is unclear, or the overlap report suggests the fork already
diverged in the same files, escalate to `QUESTION_PR_REQUIRED=true`.

## Question PR Rule

The draft question PR is not a formality. It is the technical gate for:

- compatibility decisions
- migration planning
- intentional upstream omissions
- preserving Compozy-owned behavior

Do not bypass the draft PR when the analyzer reports `medium` or `high` risk.
