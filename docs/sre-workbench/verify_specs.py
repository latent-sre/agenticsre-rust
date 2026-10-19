"""Check planning schemas, fixtures, local links and ID coverage; no product execution."""

from __future__ import annotations

import json
import math
import re
from collections import Counter
from pathlib import Path
from urllib.parse import unquote, urlsplit

from jsonschema import Draft202012Validator, FormatChecker
from jsonschema.exceptions import SchemaError
from referencing import Registry, Resource
from referencing.exceptions import Unresolvable
from referencing.jsonschema import DRAFT202012


PACKAGE = Path(__file__).resolve().parent
ROOT = PACKAGE.parents[1]
LINK = re.compile(r"\[[^\]]*\]\(([^)]+)\)")
FENCE = re.compile(r"(?ms)^([\x60~]{3})[^\n]*\n.*?^\1[^\n]*$")


def read_json(path: Path):
    def reject_constant(value):
        raise ValueError(f"Non-JSON constant: {value}")

    def finite_float(value):
        result = float(value)
        if not math.isfinite(result):
            raise ValueError("JSON number exceeds finite float range")
        return result

    def unique_object(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError(f"Duplicate JSON key: {key}")
            result[key] = value
        return result

    return json.loads(path.read_text(encoding="utf-8"), parse_constant=reject_constant,
                      parse_float=finite_float, object_pairs_hook=unique_object)


def check_embedded_schema(schema, registry):
    """Check every schema position, including unused branches, without retrieval.

    Resource traversal knows which keywords contain schemas, so literal example/default
    data is not mistaken for a schema. The anonymous root permits local pointers/anchors.
    """
    Draft202012Validator.check_schema(schema)
    root = Resource(contents=schema, specification=DRAFT202012)
    local = registry.with_resource("", root)
    visited = set()

    def visit(resource, resolver, *, enter=True):
        if enter:
            resolver = resolver.in_subresource(resource)
        contents = resource.contents
        # The pinned referencing API exposes the effective base only on the resolver.
        # Include it because the same pointed-to object may be reached in two scopes;
        # exclude dynamic-scope history, which grows while following reference cycles.
        identity = (id(contents), resolver._base_uri)
        if identity in visited:
            return
        visited.add(identity)
        if isinstance(contents, dict):
            if contents.get("$schema", Draft202012Validator.META_SCHEMA["$id"]) != Draft202012Validator.META_SCHEMA["$id"]:
                raise ValueError("Embedded schemas must use JSON Schema Draft 2020-12")
            for keyword in ("$ref", "$dynamicRef"):
                if keyword in contents:
                    uri = contents[keyword].partition("#")[0]
                    if uri and uri not in registry:
                        raise ValueError(f"Unapproved schema reference: {contents[keyword]}")
                    try:
                        resolved = resolver.lookup(contents[keyword])
                    except Unresolvable as error:
                        raise ValueError(f"Unapproved or unresolved schema reference: {contents[keyword]}") from error
                    try:
                        Draft202012Validator.check_schema(resolved.contents)
                    except SchemaError as error:
                        raise ValueError("Reference target must be a valid Draft 2020-12 schema") from error
                    # lookup already entered the target's resource scope. Do not apply
                    # its $id a second time when following a pointer into literal data.
                    visit(Resource(contents=resolved.contents, specification=DRAFT202012),
                          resolved.resolver, enter=False)
        for child in DRAFT202012.subresources_of(contents):
            visit(Resource(contents=child, specification=DRAFT202012), resolver)

    visit(root, local.resolver())


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


def all_errors(errors):
    for error in errors:
        yield error
        yield from all_errors(error.context)


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
        if not case["valid"]:
            expected = case.get("expected_error")
            observed = ({"keyword": error.validator, "path": list(error.absolute_path),
                         "schema_path": list(error.absolute_schema_path)}
                        for error in all_errors(errors))
            if expected is None or expected not in observed:
                failures.append(f"{case['file']}: missing expected validation error {expected}")
        if case["valid"] and "input_schema" in case:
            input_validator = Draft202012Validator(
                schemas[case["input_schema"]], registry=registry, format_checker=checker
            )
            for error in input_validator.iter_errors(instance["inputs"]):
                failures.append(f"{case['file']} operation inputs: {error.message}")
        if case["valid"] and "input_manifest" in case:
            manifest = read_json(contained(PACKAGE / "examples" / case["input_manifest"], PACKAGE / "examples"))
            check_embedded_schema(manifest["input_schema"], registry)
            input_validator = Draft202012Validator(manifest["input_schema"], registry=registry, format_checker=checker)
            for error in input_validator.iter_errors(instance["inputs"]):
                failures.append(f"{case['file']} operation inputs: {error.message}")
        if case["valid"] and isinstance(instance, dict):
            for key in ("input_schema", "output_schema"):
                if key in instance:
                    check_embedded_schema(instance[key], registry)

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
        ROOT / "docs/investigation-ui.md",
        ROOT / "docs/investigation-ui-verification.md",
        ROOT / "docs/investigation-mcp.md",
        ROOT / "docs/investigation-mcp-verification.md",
        ROOT / "docs/mcp-verification.md", ROOT / "docs/packaging.md",
        ROOT / "docs/packaging-verification.md", ROOT / "docs/context.md",
        ROOT / "docs/context-verification.md",
        ROOT / "docs/diagnostics.md", ROOT / "docs/diagnostics-config.md",
        ROOT / "docs/diagnostics-ui.md",
        ROOT / "docs/diagnostics-ui-verification.md",
        ROOT / "docs/diagnostics-verification.md",
        ROOT / "docs/saved-queries.md",
        ROOT / "docs/saved-queries-verification.md",
        ROOT / "docs/evidence.md",
        ROOT / "docs/evidence-collection-verification.md",
        ROOT / "docs/evidence-comparison-verification.md",
        ROOT / "docs/runbooks.md",
        ROOT / "docs/runbooks-verification.md",
        ROOT / "docs/offline-evidence.md",
        ROOT / "docs/offline-evidence-verification.md",
        ROOT / "docs/reports.md",
        ROOT / "docs/reports-verification.md",
        ROOT / "docs/toil.md",
        ROOT / "docs/toil-verification.md",
        ROOT / "docs/config-check.md",
        ROOT / "docs/config-check-verification.md",
        ROOT / "docs/alert-rules.md",
        ROOT / "docs/alert-rules-verification.md",
        ROOT / "docs/extensions.md",
        ROOT / "docs/extensions-verification.md",
        ROOT / "docs/extension-runtime.md",
        ROOT / "docs/extension-runtime-verification.md",
        ROOT / "docs/jobs.md",
        ROOT / "docs/jobs-verification.md",
        ROOT / "docs/runner-transport-notes.md",
        ROOT / "docs/runner.md",
        ROOT / "docs/runner-verification.md",
        ROOT / "docs/fleet.md",
        ROOT / "docs/fleet-verification.md",
        ROOT / "docs/changes.md",
        ROOT / "docs/schedules.md",
        ROOT / "docs/schedules-verification.md",
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
        owner_text = FENCE.sub("", (PACKAGE / owner).read_text(encoding="utf-8"))
        owned = set(re.findall(pattern, owner_text))
        referenced = set(re.findall(pattern, package_text))
        if owned != expected or not referenced.issubset(expected):
            failures.append(f"{prefix} coverage: missing={sorted(expected-owned)}, unknown={sorted(referenced-expected)}")
        rows = re.findall(rf"(?m)^\|\s*({prefix}-\d{{2}})\s*\|", owner_text)
        if Counter(rows) != Counter(expected):
            failures.append(f"{prefix} defining table must contain each ID exactly once")
        if prefix == "CAP":
            definitions = re.findall(r"(?m)^\*\*(CAP-\d{2})\.\*\*", owner_text)
            if Counter(definitions) != Counter(expected):
                failures.append("CAP detailed definitions must contain each ID exactly once")

    traceability = FENCE.sub("", (PACKAGE / "verification.md").read_text(encoding="utf-8"))
    traceability_rows = re.findall(r"(?m)^\|\s*(REQ-\d{2})\s*\|([^\n]+)", traceability)
    expected_requirements = {f"REQ-{number:02}" for number in range(1, 18)}
    if Counter(row[0] for row in traceability_rows) != Counter(expected_requirements):
        failures.append("Requirement traceability must contain each REQ exactly once")
    traced_cases = set(re.findall(r"\bAC-\d{2}\b", "\n".join(row[1] for row in traceability_rows)))
    if traced_cases != {f"AC-{number:02}" for number in range(1, 37)}:
        failures.append("Requirement traceability must cover every acceptance case")

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
