"""Regressions for the specification gate; uses altered in-memory file reads only."""
from __future__ import annotations

import contextlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("verify_specs", ROOT / "docs/sre-workbench/verify_specs.py")
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)
PACKAGE = verify.PACKAGE


class SpecificationTests(unittest.TestCase):
    def run_gate(self, changes=None):
        original = Path.read_text
        changes = changes or {}

        def read(path, *args, **kwargs):
            return changes[path] if path in changes else original(path, *args, **kwargs)

        output = io.StringIO()
        with patch.object(Path, "read_text", read), contextlib.redirect_stdout(output):
            try:
                status = verify.main()
            except ValueError as error:
                status = 1
                print(error)
        return status, output.getvalue()

    def test_repository_is_valid(self):
        self.assertEqual(self.run_gate()[0], 0)

    def test_json_rejects_nonfinite_values_and_duplicate_keys(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "input.json"
            for raw in ('{"value":NaN}', '{"value":Infinity}', '{"value":-Infinity}',
                        '{"value":1e999}', '{"value":1,"value":2}'):
                with self.subTest(raw=raw):
                    path.write_text(raw, encoding="utf-8")
                    with self.assertRaises(ValueError):
                        verify.read_json(path)

    def test_embedded_references_must_resolve_locally(self):
        path = PACKAGE / "examples/capability-command.json"
        for reference in ("https://example.invalid/schema", "urn:unknown:schema",
                          "urn:sre-workbench:spec:common:0.1#/$defs/missing"):
            with self.subTest(reference=reference):
                instance = verify.read_json(path)
                instance["input_schema"] = {"type": "object", "properties": {
                    "unused": {"$ref": reference}}}
                status, output = self.run_gate({path: json.dumps(instance)})
                self.assertEqual(status, 1, output)

    def test_embedded_local_references_and_literal_ref_data_are_allowed(self):
        path = PACKAGE / "examples/capability-command.json"
        instance = verify.read_json(path)
        instance["output_schema"] = {"$defs": {"text": {"type": "string"}},
            "type": "object", "properties": {"value": {"$ref": "#/$defs/text"}},
            "examples": [{"$ref": "literal-data"}]}
        status, output = self.run_gate({path: json.dumps(instance)})
        self.assertEqual(status, 0, output)

    def test_embedded_dialect_cannot_hide_schema_reference_positions(self):
        path = PACKAGE / "examples/capability-command.json"
        hidden = {"$schema": "http://json-schema.org/draft-07/schema#", "$defs": {
            "unused": {"$ref": "https://example.invalid/schema"}}}
        for schema in (hidden, {"type": "object", "properties": {"unused": hidden}}):
            with self.subTest(schema=schema):
                instance = verify.read_json(path)
                instance["output_schema"] = schema
                status, output = self.run_gate({path: json.dumps(instance)})
                self.assertEqual(status, 1, output)

    def test_external_capabilities_require_digest(self):
        path = PACKAGE / "examples/capability-command.json"
        for kind in ("script", "executable"):
            with self.subTest(kind=kind):
                instance = verify.read_json(path)
                instance["implementation"] = {"kind": kind, "reference": "installed-task"}
                status, output = self.run_gate({path: json.dumps(instance)})
                self.assertEqual(status, 1, output)
                instance["implementation"]["digest"] = "sha256:" + "0" * 64
                status, output = self.run_gate({path: json.dumps(instance)})
                self.assertEqual(status, 0, output)

    def test_capability_definition_and_catalog_are_independently_required(self):
        path = PACKAGE / "capabilities.md"
        original = path.read_text(encoding="utf-8")
        for removed in ("**CAP-01.**", next(line for line in original.splitlines()
                                             if line.startswith("| CAP-01 |"))):
            with self.subTest(removed=removed):
                status, output = self.run_gate({path: original.replace(removed, "", 1)})
                self.assertEqual(status, 1, output)

    def test_acceptance_definition_cannot_be_hidden_by_traceability(self):
        path = PACKAGE / "verification.md"
        original = path.read_text(encoding="utf-8")
        row = next(line for line in original.splitlines() if line.startswith("| AC-01 |"))
        status, output = self.run_gate({path: original.replace(row, "", 1)})
        self.assertEqual(status, 1, output)

    def test_requirement_traceability_is_independently_required(self):
        path = PACKAGE / "verification.md"
        original = path.read_text(encoding="utf-8")
        row = next(line for line in original.splitlines() if line.startswith("| REQ-01 |"))
        status, output = self.run_gate({path: original.replace(row, "", 1)})
        self.assertEqual(status, 1, output)

    def test_unrelated_validation_error_cannot_mask_missing_timestamp_check(self):
        path = PACKAGE / "examples/invalid-result-timestamp.json"
        instance = verify.read_json(path)
        instance["finished_at"] = "2026-10-01T00:00:00Z"
        instance["unrelated_extra"] = True
        status, output = self.run_gate({path: json.dumps(instance)})
        self.assertEqual(status, 1, output)

    def test_wrong_branch_error_cannot_mask_missing_nested_check(self):
        path = PACKAGE / "examples/invalid-saved-query-input.json"
        instance = verify.read_json(path)
        instance["window"]["lookback_seconds"] = 300
        instance["window"]["unrelated_extra"] = True
        status, output = self.run_gate({path: json.dumps(instance)})
        self.assertEqual(status, 1, output)


if __name__ == "__main__":
    unittest.main()
