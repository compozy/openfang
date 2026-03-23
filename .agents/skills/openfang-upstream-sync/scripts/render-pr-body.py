#!/usr/bin/env python3

import argparse
from pathlib import Path


def load_env(path: Path) -> dict[str, str]:
    if not path.exists():
        raise SystemExit(f"ERROR: Missing analysis env file: {path}")
    result: dict[str, str] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        result[key.strip()] = value.strip()
    return result


def load_list(path: Path | None) -> list[str]:
    if path is None or not path.exists():
        return []
    return [line.strip() for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]


def bullet_list(items: list[str], empty_message: str, code: bool = False) -> str:
    if not items:
        return f"- {empty_message}"
    prefix = "- "
    if code:
        return "\n".join(f"{prefix}`{item}`" for item in items)
    return "\n".join(f"{prefix}{item}" for item in items)


def commit_list(items: list[str]) -> str:
    if not items:
        return "- No commits listed."
    rows = []
    for item in items:
        parts = item.split(" ", 1)
        sha = parts[0]
        title = parts[1] if len(parts) > 1 else ""
        rows.append(f"- `{sha}` {title}".rstrip())
    return "\n".join(rows)


def load_optional_text(path_value: str | None, fallback: str) -> str:
    if not path_value:
        return fallback
    path = Path(path_value)
    if not path.exists():
        return fallback
    text = path.read_text(encoding="utf-8").strip()
    return text or fallback


def render(template_path: Path, mapping: dict[str, str]) -> str:
    rendered = template_path.read_text(encoding="utf-8")
    for key, value in mapping.items():
        rendered = rendered.replace(f"{{{{{key}}}}}", value)
    return rendered


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--mode", choices=["question-pr", "question-note", "final-pr"], required=True)
    parser.add_argument("--analysis-env", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--strategy")
    parser.add_argument("--validation-file")
    parser.add_argument("--conflict-summary-file")
    parser.add_argument("--local-adaptations-file")
    parser.add_argument("--residual-risks-file")
    args = parser.parse_args()

    skill_root = Path(__file__).resolve().parent.parent
    assets_dir = skill_root / "assets"

    env = load_env(Path(args.analysis_env))
    commits = load_list(Path(env["COMMITS_FILE"]))
    files = load_list(Path(env["FILES_FILE"]))
    reasons = load_list(Path(env["REASONS_FILE"]))
    overlap = load_list(Path(env["OVERLAP_FILE"]))

    base_mapping = {
        "current_branch": env.get("CURRENT_BRANCH", "unknown"),
        "upstream_ref": env.get("UPSTREAM_REF", "upstream/main"),
        "merge_base": env.get("MERGE_BASE", "unknown"),
        "upstream_head": env.get("UPSTREAM_HEAD", "unknown"),
        "upstream_commit_count": env.get("UPSTREAM_COMMIT_COUNT", "0"),
        "changed_file_count": env.get("CHANGED_FILE_COUNT", "0"),
        "risk_level": env.get("RISK_LEVEL", "unknown"),
        "question_pr_required": env.get("QUESTION_PR_REQUIRED", "unknown"),
        "commit_list": commit_list(commits),
        "file_list": bullet_list(files, "No changed files reported.", code=True),
        "risk_reasons": bullet_list(reasons, "No explicit risk reasons reported."),
        "overlap_list": bullet_list(overlap, "No overlap with local fork changes detected.", code=True),
        "open_questions": "\n".join(
            [
                "- Which local fork-specific behavior must remain authoritative in the affected areas?",
                "- Does this upstream change require migration, rollout notes, or config changes in Compozy?",
                "- Should any upstream change be intentionally skipped or partially adapted?",
            ]
        ),
        "resolution_markers": "\n".join(
            [
                "- `Resolution: proceed`",
                "- `Resolution: proceed-with-followups`",
                "- `Resolution: do-not-sync`",
            ]
        ),
        "strategy": args.strategy or "Not provided.",
        "validation_summary": load_optional_text(args.validation_file, "Validation summary not provided."),
        "conflict_summary": load_optional_text(
            args.conflict_summary_file,
            "No manual conflict summary was recorded.",
        ),
        "local_adaptations": load_optional_text(
            args.local_adaptations_file,
            "No additional local adaptations were recorded.",
        ),
        "residual_risks": load_optional_text(
            args.residual_risks_file,
            "No residual risks were recorded.",
        ),
    }

    template_name = {
        "question-pr": "question-pr-template.md",
        "question-note": "question-note-template.md",
        "final-pr": "final-pr-template.md",
    }[args.mode]

    output = render(assets_dir / template_name, base_mapping)
    output_path = Path(args.output)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(output.rstrip() + "\n", encoding="utf-8")

    print(f"SUCCESS: Rendered {args.mode} to {output_path}")


if __name__ == "__main__":
    main()
