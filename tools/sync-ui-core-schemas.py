#!/usr/bin/env python3
"""Synchronize the browser's embedded authoritative core schema closure, offline."""
import argparse
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ROOTS = {
    "result", "common", "context-resolve-input", "context-data",
    "evidence-reference-input", "evidence-compare-input", "report-create-input",
    "evidence-offline-data", "evidence-comparison-data", "report-data",
}


def synchronize(specification: dict) -> None:
    sources = {}
    for path in (ROOT / "docs/sre-workbench/schemas").glob("*.schema.json"):
        schema = json.loads(path.read_text())
        if "$id" in schema:
            sources[schema["$id"]] = schema
    names = {key: "Core" + "".join(word.capitalize() for word in key.split(":")[-2].split("-"))
             for key in sources}
    visited = set()

    def embed(key: str) -> None:
        if key in visited:
            return
        visited.add(key)  # Receipt -> change plan -> receipt and offline receipt recursion.
        name = names[key]

        def relocate(value):
            if isinstance(value, list):
                return [relocate(item) for item in value]
            if not isinstance(value, dict):
                return value
            result = {field: relocate(item) for field, item in value.items()
                      if field not in ("$id", "$schema")}
            reference = value.get("$ref")
            if isinstance(reference, str):
                if reference.startswith("#"):
                    result["$ref"] = "#/components/schemas/" + name + reference[1:]
                elif reference.split("#")[0] in sources:
                    target, _, fragment = reference.partition("#")
                    embed(target)
                    result["$ref"] = "#/components/schemas/" + names[target] + fragment
            return result

        specification["components"]["schemas"][name] = relocate(sources[key])

    for key in sources:
        if key.split(":")[-2] in ROOTS:
            embed(key)


    # OpenAPI generators cannot address structural oneOf/prefixItems nodes as TypeScript
    # members. Lift the exact source schema objects into named equivalent components.
    components = specification["components"]["schemas"]
    aliases = {
        "#/components/schemas/CoreDashboardHygieneData/oneOf/0/properties/supervisor": "CoreDashboardHygieneSupervisor",
        "#/components/schemas/CoreEvidenceCollectionPayload/properties/steps/prefixItems/0/properties/definition": "CoreEvidenceMetricsDefinition",
        "#/components/schemas/CoreEvidenceCollectionPayload/properties/steps/prefixItems/1/properties/definition": "CoreEvidenceLogsDefinition",
    }
    for pointer, name in aliases.items():
        target = specification
        for token in pointer[2:].split("/"):
            target = target[int(token)] if isinstance(target, list) else target[token]
        components[name] = target

    def named_references(value):
        if isinstance(value, list):
            for item in value:
                named_references(item)
        elif isinstance(value, dict):
            reference = value.get("$ref")
            if isinstance(reference, str) and reference in aliases:
                value["$ref"] = "#/components/schemas/" + aliases[reference]
            for item in value.values():
                named_references(item)

    named_references(components)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    arguments = parser.parse_args()
    path = ROOT / "docs/ui/openapi.json"
    original = path.read_text()
    specification = json.loads(original)
    synchronize(specification)
    generated = json.dumps(specification, indent=2) + "\n"
    if arguments.check:
        if generated != original:
            parser.error("embedded core schema closure differs; run tools/sync-ui-core-schemas.py")
    else:
        path.write_text(generated)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
