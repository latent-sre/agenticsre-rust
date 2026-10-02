"""Check planning schemas, fixtures, local links and ID coverage; no product execution."""

from __future__ import annotations

import json
import re
from pathlib import Path
from urllib.parse import unquote, urlsplit

from jsonschema import Draft202012Validator, FormatChecker
from referencing import Registry, Resource


PACKAGE = Path(__file__).resolve().parent
ROOT = PACKAGE.parents[1]
LINK = re.compile(r"\[[^\]]*\]\(([^)]+)\)")
FENCE = re.compile(r"(?ms)^([\x60~]{3})[^\n]*\n.*?^\1[^\n]*$")


def read_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def contained(path: Path, root: Path) -> Path:
    resolved = path.resolve()
    if not resolved.is_relative_to(root.resolve()):
        raise ValueError(f"Path escapes {root}: {path}")
    return resolved


def anchors(text: str) -> set[str]:
    found: set[str] = set()
    counts: dict[str, int] = {}
    for title in re.findall(r"(?m)^#{1,6} +(.+?)\s*$", FENCE.sub("", text)):
        slug = re.sub(r"[^\w\s-]", "", title.casefold())
        slug = re.sub(r"\s", "-", slug)
        count = counts.get(slug, 0)
        found.add(slug if count == 0 else f"{slug}-{count}")
        counts[slug] = count + 1
    return found


def main() -> int:
    failures: list[str] = []
    schema_paths = sorted((PACKAGE / "schemas").glob("*.schema.json"))
    schemas = {path.name.removesuffix(".schema.json"): read_json(path) for path in schema_paths}
    resources = []
    for name, schema in schemas.items():
        Draft202012Validator.check_schema(schema)
        resources.append((schema["$id"], Resource.from_contents(schema)))
    registry = Registry().with_resources(resources)
    checker = FormatChecker()
    if "date-time" not in checker.checkers:
        print("FAIL: date-time format validation is unavailable; install requirements-docs.txt.")
        return 1

    cases = read_json(PACKAGE / "examples/cases.json")
    instance_names = {case["file"] for case in cases}
    actual_names = {p.name for p in (PACKAGE / "examples").glob("*.json")} - {"cases.json"}
    if actual_names != instance_names or len(instance_names) != len(cases):
        failures.append("Example manifest must cover every JSON fixture exactly once")
    for case in cases:
        instance = read_json(contained(PACKAGE / "examples" / case["file"], PACKAGE / "examples"))
        validator = Draft202012Validator(schemas[case["schema"]], registry=registry, format_checker=checker)
        errors = list(validator.iter_errors(instance))
        if bool(errors) == case["valid"]:
            detail = errors[0].message if errors else "unexpectedly accepted"
            failures.append(f"{case['file']}: {detail}")
        if case["valid"] and "input_schema" in case:
            input_validator = Draft202012Validator(
                schemas[case["input_schema"]], registry=registry, format_checker=checker
            )
            for error in input_validator.iter_errors(instance["inputs"]):
                failures.append(f"{case['file']} operation inputs: {error.message}")
        if case["valid"] and "input_manifest" in case:
            manifest = read_json(contained(PACKAGE / "examples" / case["input_manifest"], PACKAGE / "examples"))
            input_validator = Draft202012Validator(manifest["input_schema"], registry=registry, format_checker=checker)
            for error in input_validator.iter_errors(instance["inputs"]):
                failures.append(f"{case['file']} operation inputs: {error.message}")
        if case["valid"] and isinstance(instance, dict):
            for key in ("input_schema", "output_schema"):
                if key in instance:
                    Draft202012Validator.check_schema(instance[key])

    markdown = sorted(PACKAGE.rglob("*.md"))
    repository_docs = [
        ROOT / "README.md", ROOT / "CONTRIBUTING.md",
        ROOT / "docs/roadmap.md", ROOT / "docs/plan-import.md", ROOT / "docs/poc.md",
        ROOT / "AGENTS.md", ROOT / "docs/development.md", ROOT / "docs/adapter-notes.md",
        ROOT / "docs/poc-verification.md",
        ROOT / "docs/tasks.md", ROOT / "docs/dashboard-verification.md", ROOT / "docs/error-budget.md",
        ROOT / "docs/error-budget-verification.md", ROOT / "docs/grafana.md",
        ROOT / "docs/grafana-verification.md", ROOT / "docs/read-profile.md", ROOT / "docs/mcp.md",
        ROOT / "docs/read-profile-verification.md", ROOT / "docs/ui.md",
        ROOT / "docs/ui-verification.md",
    ]
    for document in [*markdown, *repository_docs]:
        text = FENCE.sub("", document.read_text(encoding="utf-8"))
        for raw in LINK.findall(text):
            target = urlsplit(raw.strip("<>"))
            if target.scheme or target.netloc:
                continue
            try:
                resolved = contained(document.parent / unquote(target.path), ROOT) if target.path else document
            except ValueError as error:
                failures.append(str(error))
                continue
            if not resolved.exists():
                failures.append(f"{document.relative_to(ROOT)}: missing link {raw}")
            elif target.fragment and resolved.suffix == ".md":
                if unquote(target.fragment) not in anchors(resolved.read_text(encoding="utf-8")):
                    failures.append(f"{document.relative_to(ROOT)}: missing anchor {raw}")

    owners = {
        "CAP": ("capabilities.md", 25),
        "REQ": ("product.md", 17),
        "NFR": ("product.md", 12),
        "AC": ("verification.md", 36),
        "WP": ("delivery.md", 16),
        "DEC": ("decisions-and-risks.md", 15),
        "RISK": ("decisions-and-risks.md", 16),
    }
    package_text = "\n".join(path.read_text(encoding="utf-8") for path in markdown)
    for prefix, (owner, count) in owners.items():
        expected = {f"{prefix}-{number:02}" for number in range(1, count + 1)}
        pattern = rf"\b{prefix}-\d{{2}}\b"
        owned = set(re.findall(pattern, (PACKAGE / owner).read_text(encoding="utf-8")))
        referenced = set(re.findall(pattern, package_text))
        if owned != expected or not referenced.issubset(expected):
            failures.append(f"{prefix} coverage: missing={sorted(expected-owned)}, unknown={sorted(referenced-expected)}")

    if failures:
        print("\n".join(failures))
        return 1
    positive = sum(case["valid"] for case in cases)
    print(f"PASS: {len(schemas)} schemas; {positive} positive and {len(cases)-positive} negative fixtures; "
          f"{len(markdown)} package documents plus {len(repository_docs)} repository documents; "
          "all planning IDs covered.")
    print("This checks specification structure, not product behavior or live readiness.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
