"""Independent numerical and public-contract checks for the fixed error-budget task."""

from __future__ import annotations

import argparse
import json
import math
import os
from pathlib import Path
import re
import subprocess
import tempfile

from jsonschema import Draft202012Validator, FormatChecker
from referencing import Registry, Resource


ROOT = Path(__file__).resolve().parents[1]


def expected_subset(actual: object, expected: object, location: str = "calculation") -> None:
    if isinstance(expected, dict):
        assert isinstance(actual, dict), location
        for key, value in expected.items():
            assert key in actual, f"{location}.{key} missing"
            expected_subset(actual[key], value, f"{location}.{key}")
    elif isinstance(expected, (int, float)) and not isinstance(expected, bool):
        assert isinstance(actual, (int, float)) and not isinstance(actual, bool), location
        assert math.isclose(actual, expected, rel_tol=1e-10, abs_tol=1e-10), (location, actual, expected)
    else:
        assert actual == expected, (location, actual, expected)


def finite_tree(value: object) -> None:
    if isinstance(value, dict):
        for item in value.values():
            finite_tree(item)
    elif isinstance(value, list):
        for item in value:
            finite_tree(item)
    elif isinstance(value, float):
        assert math.isfinite(value), "Nonfinite receipt number"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--missing-runtime", action="store_true")
    options = parser.parse_args()
    binary = options.binary.resolve(strict=True)
    schemas = {p.stem.removesuffix(".schema"): json.loads(p.read_text(encoding="utf-8"))
               for p in (ROOT / "docs/sre-workbench/schemas").glob("*.schema.json")}
    registry = Registry().with_resources((s["$id"], Resource.from_contents(s)) for s in schemas.values())

    def validate(schema: dict, value: object) -> None:
        Draft202012Validator(schema, registry=registry, format_checker=FormatChecker()).validate(value)

    count = 0

    def run(*argv: str, expected: int = 0, cwd: Path = ROOT,
            env: dict[str, str] | None = None) -> dict:
        nonlocal count
        completed = subprocess.run([str(binary), "--json", *argv], cwd=cwd, env=env,
                                   capture_output=True, timeout=12, check=False)
        assert completed.returncode == expected, (argv[:3], completed.returncode,
                                                  completed.stdout[:1000], completed.stderr[:500])
        assert len(completed.stdout) <= 2 * 1024 * 1024
        result = json.loads(completed.stdout)
        finite_tree(result)
        validate(schemas["result"], result)
        assert result["assessment"] == "not_assessed" and result["record_mode"] == "never"
        count += 1
        return result

    listed = run("task", "list")
    names = {item["task"]["id"] for item in listed["data"]["tasks"]}
    assert {"dashboard-hygiene", "error-budget"} <= names
    item = next(item for item in listed["data"]["tasks"] if item["task"]["id"] == "error-budget")
    metadata = item["task"]
    validate(schemas["task"], metadata)
    described = run("task", "describe", "error-budget")
    assert described["data"]["task"] == metadata

    with tempfile.TemporaryDirectory(prefix="workbench-budget-contract-") as directory:
        scratch = Path(directory)
        input_file = scratch / "measurements café ; literal.json"
        argv = ("task", "run", "error-budget", "--input", str(input_file))

        def write(value: object) -> bytes:
            raw = json.dumps(value, allow_nan=False).encode("utf-8")
            input_file.write_bytes(raw)
            return raw

        write({"slo": 99.9, "bad_minutes": 40.32})
        if options.missing_runtime:
            assert item["availability"]["status"] == "missing_dependency"
            failed = run(*argv, expected=2)
            assert failed["effect_outcome"] == "not_attempted"
            assert any(e["code"] == "missing_dependency" for e in failed["errors"])
            print(f"PASS: {count} error-budget results; missing runtime is explicit.")
            return 0

        assert item["availability"]["status"] == "available"
        cases = json.loads((ROOT / "tests/fixtures/error-budget/cases.json").read_text(encoding="utf-8"))
        completed_results = {}
        for case in cases:
            raw = write(case["input"])
            result = run(*argv)
            assert input_file.read_bytes() == raw, case["name"]
            assert result["execution"]["status"] == "succeeded"
            assert result["effect_outcome"] == "not_applicable"
            assert result["coverage"]["state"] == "complete"
            data = result["data"]
            validate(metadata["output_schema"], data)
            expected_subset(data["calculation"], case["expected"])
            assert data["input"] == {"window_days": 28, "long_window": "1h", "short_window": "5m", **case["input"]}
            assert data["source"]["upstream_revision"] == "7d900fb999abee3b3712e1da881dcc7e80b1f6e7"
            assert data["source"]["upstream_digest"] == "sha256:4e55475e6b10a4a2e195b1a6f162d688b093664c06bee69cb09435f1931cf5c2"
            assert re.fullmatch(r"sha256:[0-9a-f]{64}", data["source"]["adapter_digest"])
            assert data["source"]["binding_digest"] == metadata["digest"]
            status = data["calculation"]["status"]
            if status and status["state"] == "exhausted":
                assert status["remaining"] == 0 and math.copysign(1, status["remaining"]) == 1
            burn = data["calculation"]["burn"]
            if burn:
                assert burn["policy"] == "fixed-30-day-example-v1" and burn["policy_horizon_days"] == 30
            completed_results[case["name"]] = result

        request = {"spec_version": "0.1", "request_id": "budget-contract-001",
                   "operation": "task.run", "operation_version": 1,
                   "target": {"kind": "local", "id": "workstation"},
                   "inputs": {"id": "error-budget", "version": 1,
                              "input": {"slo": 99.9, "bad_minutes": 40.32}},
                   "limits": {"timeout_ms": 1000, "max_output_bytes": 1048576}, "record": "never"}
        request_file = scratch / "request.json"
        request_file.write_text(json.dumps(request), encoding="utf-8")
        called = run("call", "--request", str(request_file))
        reference = completed_results["exact_time_exhaustion"]
        for key in ("calculation", "input", "source", "task"):
            assert called["data"][key] == reference["data"][key], key

        invalid = [
            [], [99.9], [99.9, 28], {}, {"slo": 0}, {"slo": 100}, {"slo": -1}, {"slo": True}, {"slo": None},
            {"slo": "99.9"}, {"slo": 99.9, "window_days": 0},
            {"slo": 99.9, "bad_minutes": -1}, {"slo": 99.9, "bad_minutes": None},
            {"slo": 99.9, "bad_events": 1}, {"slo": 99.9, "total_events": 10},
            {"slo": 99.9, "bad_events": 1, "total_events": 0},
            {"slo": 99.9, "bad_events": 11, "total_events": 10},
            {"slo": 99.9, "bad_minutes": 1, "bad_events": 1, "total_events": 10},
            {"slo": 99.9, "sli_short": 99}, {"slo": 99.9, "sli_long": 101},
            {"slo": 99.9, "sli_long": False}, {"slo": 99.9, "sli_long": -1},
            {"slo": 99.9, "long_window": "1h", "short_window": "30m"},
            {"slo": 99.9, "runtime": "/bin/sh"}, {"slo": 99.9, "extra": {}},
        ]
        for value in invalid:
            raw = write(value)
            denied = run(*argv, expected=2)
            assert denied["effect_outcome"] == "not_attempted" and denied["errors"]
            assert input_file.read_bytes() == raw

        for raw in (b'{"slo":NaN}', b'{"slo":1e999}', b'{"slo":99,"slo":99.9}', b'null'):
            input_file.write_bytes(raw)
            denied = run(*argv, expected=2)
            assert denied["effect_outcome"] == "not_attempted" and denied["errors"]

        for value in ({"slo": 99.9, "window_days": 1e308, "bad_minutes": 0},
                      {"slo": 99.9, "bad_events": 0, "total_events": 5e-324}):
            write(value)
            failed = run(*argv, expected=1)
            assert failed["execution"]["status"] == "failed"
            assert any(error["code"] == "numeric_out_of_range" for error in failed["errors"])
            assert "calculation" not in failed["data"]

        write({"slo": 99.9, "bad_minutes": 40.32})
        invalid_policy = run(*argv, "--fail-on-findings", expected=2)
        assert invalid_policy["effect_outcome"] == "not_attempted"
        marker = scratch / "workspace-code-ran"
        poison = f"from pathlib import Path\nPath({str(marker)!r}).write_text('bad')\nraise RuntimeError('workspace code ran')\n"
        for filename in ("json.py", "decimal.py", "sitecustomize.py", "error_budget.py"):
            (scratch / filename).write_text(poison, encoding="utf-8")
        hostile = dict(os.environ, PYTHONPATH=str(scratch),
                       PYTHONSTARTUP=str(scratch / "sitecustomize.py"), BUDGET_SECRET="synthetic-budget-canary")
        safe = run(*argv, cwd=scratch, env=hostile)
        assert not marker.exists() and safe["data"]["calculation"]["status"]["state"] == "exhausted"
        assert "synthetic-budget-canary" not in json.dumps(safe)

    print(f"PASS: {count} schema-valid error-budget results; {len(cases)} independent numerical fixtures, "
          "units, policy boundaries, admission, overflow, call parity and source binding.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
