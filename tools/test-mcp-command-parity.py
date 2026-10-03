"""Regression checks for the command client's semantic receipt comparison."""
from __future__ import annotations

import copy
import importlib.util
from pathlib import Path
import unittest


spec = importlib.util.spec_from_file_location(
    "mcp_command_acceptance", Path(__file__).with_name("verify-mcp-commands.py")
)
assert spec is not None and spec.loader is not None
client = importlib.util.module_from_spec(spec)
spec.loader.exec_module(client)


class ReceiptParity(unittest.TestCase):
    def setUp(self):
        self.receipt = {
            "request_id": "request-a", "run_id": "run-a",
            "started_at": "2026-10-02T19:42:48Z",
            "finished_at": "2026-10-02T19:42:49Z", "duration_ms": 1000,
            "sources": [{"kind": "command", "observed_at": "2026-10-02T19:42:48Z",
                         "digest": "fixed-digest", "identity": "git"}],
            "data": {"supervisor": {"process_id": 20, "process_group_id": 20},
                     "root": {"id": "selected"},
                     "tool": {"exit_status": 0, "execution_confirmed": True}},
            "output": {"stdout": " M tracked.txt\n", "stderr": ""},
        }

    def test_observation_timestamp_does_not_change_receipt_semantics(self):
        later = copy.deepcopy(self.receipt)
        later["sources"][0]["observed_at"] = "2026-10-02T19:42:50Z"
        self.assertEqual(client.comparable(self.receipt), client.comparable(later))

    def test_semantic_differences_are_preserved(self):
        for path, replacement in (
            (("sources", 0, "digest"), "changed-digest"),
            (("sources", 0, "identity"), "rg"),
            (("data", "root", "id"), "hidden"),
            (("data", "tool", "exit_status"), 1),
            (("data", "tool", "execution_confirmed"), False),
            (("output", "stdout"), "different output"),
        ):
            with self.subTest(path=path):
                changed = copy.deepcopy(self.receipt)
                parent = changed
                for component in path[:-1]:
                    parent = parent[component]
                parent[path[-1]] = replacement
                self.assertNotEqual(client.comparable(self.receipt), client.comparable(changed))

    def test_comparison_preserves_original_receipt(self):
        original = copy.deepcopy(self.receipt)
        client.comparable(self.receipt)
        self.assertEqual(self.receipt, original)


if __name__ == "__main__":
    unittest.main()
