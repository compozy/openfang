#!/bin/sh
set -eu

METADATA_FILE=""

while [ "$#" -gt 0 ]; do
  case "$1" in
    --metadata-file)
      METADATA_FILE="$2"
      shift 2
      ;;
    *)
      echo "usage: $0 [--metadata-file <path>]" >&2
      exit 2
      ;;
  esac
done

if [ -n "$METADATA_FILE" ]; then
  METADATA_JSON=$(cat "$METADATA_FILE")
else
  METADATA_JSON=$(cargo metadata --format-version 1 --no-deps)
fi

METADATA_JSON="$METADATA_JSON" python3 - <<'PY'
import json
import os
import sys

EXPECTED = {
    "arky-error": set(),
    "arky-protocol": {"arky-error"},
    "arky-tools": {"arky-error", "arky-protocol"},
    "arky-hooks": {"arky-error", "arky-protocol"},
    "arky-session": {"arky-error", "arky-protocol"},
    "arky-provider": {
        "arky-error",
        "arky-hooks",
        "arky-protocol",
        "arky-session",
        "arky-tools",
    },
    "arky-mcp": {
        "arky-error",
        "arky-protocol",
        "arky-tools",
    },
    "arky-codex": {
        "arky-error",
        "arky-mcp",
        "arky-protocol",
        "arky-provider",
        "arky-tools",
    },
    "arky-claude-code": {
        "arky-error",
        "arky-mcp",
        "arky-protocol",
        "arky-provider",
        "arky-tools",
    },
    "arky-config": {
        "arky-claude-code",
        "arky-codex",
        "arky-error",
        "arky-protocol",
        "arky-provider",
    },
    "openfang-types": set(),
    "openfang-memory": {"openfang-types"},
    "openfang-wire": {"openfang-types"},
    "openfang-skills": {"openfang-types"},
    "openfang-hands": {"openfang-types"},
    "openfang-channels": {"openfang-types"},
    "openfang-extensions": {"openfang-types"},
    "openfang-migrate": {"openfang-types"},
    "openfang-runtime": {
        "openfang-types",
        "openfang-memory",
        "openfang-skills",
    },
    "openfang-provider-binding": {
        "arky-claude-code",
        "arky-codex",
        "arky-config",
        "arky-error",
        "arky-protocol",
        "arky-provider",
        "openfang-runtime",
        "openfang-types",
    },
    "openfang-agent-definition": {
        "arky-claude-code",
        "arky-config",
        "openfang-provider-binding",
        "openfang-types",
    },
    "openfang-kernel": {
        "openfang-types",
        "openfang-memory",
        "openfang-runtime",
        "openfang-skills",
        "openfang-hands",
        "openfang-extensions",
        "openfang-wire",
        "openfang-channels",
    },
    "openfang-api": {
        "openfang-types",
        "openfang-kernel",
        "openfang-runtime",
        "openfang-memory",
        "openfang-channels",
        "openfang-wire",
        "openfang-skills",
        "openfang-hands",
        "openfang-extensions",
        "openfang-migrate",
    },
    "openfang-cli": {
        "openfang-types",
        "openfang-kernel",
        "openfang-api",
        "openfang-migrate",
        "openfang-skills",
        "openfang-extensions",
        "openfang-runtime",
    },
    "openfang-desktop": {
        "openfang-types",
        "openfang-kernel",
        "openfang-api",
    },
    "xtask": set(),
}

metadata = json.loads(os.environ["METADATA_JSON"])
packages = metadata.get("packages", [])
workspace_members = set(metadata.get("workspace_members") or [])
package_by_id = {package.get("id"): package for package in packages}

if workspace_members:
    workspace_packages = [
        package_by_id[package_id]
        for package_id in workspace_members
        if package_id in package_by_id
    ]
else:
    workspace_packages = packages

workspace_names = {package["name"] for package in workspace_packages}
internal_names = workspace_names.intersection(EXPECTED.keys())
unknown_workspace = sorted(
    name
    for name in workspace_names
    if (name.startswith("openfang") or name.startswith("arky")) and name not in EXPECTED
)

errors = []
graph = {}

for package in workspace_packages:
    name = package["name"]
    if name not in EXPECTED:
        continue

    deps = {
        dependency["name"]
        for dependency in package.get("dependencies", [])
        if dependency["name"] in internal_names
        and dependency.get("kind") != "dev"
    }
    graph[name] = deps
    disallowed = sorted(deps - EXPECTED[name])
    if disallowed:
        errors.append(
            f"{name} depends on disallowed internal crates: {', '.join(disallowed)}"
        )

for name in sorted(unknown_workspace):
    errors.append(
        f"{name} is a workspace crate missing from dependency-graph expectations"
    )

state = {}
stack = []
cycles = []

def visit(node: str) -> None:
    status = state.get(node)
    if status == "done":
        return
    if status == "visiting":
        if node in stack:
            start = stack.index(node)
            cycle = stack[start:] + [node]
            cycles.append(" -> ".join(cycle))
        return

    state[node] = "visiting"
    stack.append(node)
    for dependency in sorted(graph.get(node, set())):
        visit(dependency)
    stack.pop()
    state[node] = "done"

for package_name in sorted(graph.keys()):
    visit(package_name)

if cycles:
    for cycle in sorted(set(cycles)):
        errors.append(f"dependency cycle detected: {cycle}")

if errors:
    for error in errors:
        print(error, file=sys.stderr)
    sys.exit(1)

checked = ", ".join(sorted(graph.keys()))
print(f"dependency graph OK: {checked}")
PY
