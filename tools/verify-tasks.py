"""Public CLI acceptance for the fixed dashboard task; run in the test sandbox."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

from jsonschema import Draft202012Validator, FormatChecker
from referencing import Registry, Resource


ROOT = Path(__file__).resolve().parents[1]


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

    def run(*argv: str, expected: int | tuple[int, ...] = 0, cwd: Path = ROOT,
            env: dict[str, str] | None = None) -> dict:
        nonlocal count
        result = subprocess.run([str(binary), "--json", *argv], cwd=cwd, env=env,
                                capture_output=True, timeout=12, check=False)
        codes = (expected,) if isinstance(expected, int) else expected
        if result.returncode not in codes:
            raise RuntimeError(f"{argv[:3]} returned {result.returncode}, expected {codes}; "
                               f"stdout={result.stdout[:1000]!r}; stderr={result.stderr[:500]!r}")
        if len(result.stdout) > 2 * 1024 * 1024:
            raise RuntimeError("Task receipt exceeds encoded result ceiling")
        value = json.loads(result.stdout)
        validate(schemas["result"], value)
        assert value["assessment"] == "not_assessed" and value["record_mode"] == "never"
        count += 1
        return value

    listed = run("task", "list")
    task = next(item for item in listed["data"]["tasks"] if item["task"]["id"] == "dashboard-hygiene")
    validate(schemas["task"], task["task"])
    description = run("task", "describe", "dashboard-hygiene")
    assert description["data"]["task"] == task["task"]
    unknown = run("task", "describe", "workspace-script", expected=2)
    assert unknown["effect_outcome"] == "not_attempted"

    with tempfile.TemporaryDirectory(prefix="workbench-task-contract-") as temp:
        scratch = Path(temp)
        model = scratch / "dashboard ; literal café.json"
        raw = (ROOT / "tests/fixtures/dashboard-hygiene/clean.json").read_bytes()
        model.write_bytes(raw)
        input_file = scratch / "input.json"
        input_file.write_text(json.dumps({"file": str(model)}), encoding="utf-8")
        argv = ("task", "run", "dashboard-hygiene", "--input", str(input_file))
        if options.missing_runtime:
            assert task["availability"]["status"] == "missing_dependency"
            missing = run(*argv, expected=2)
            assert any(e["code"] == "missing_dependency" for e in missing["errors"])
            assert missing["effect_outcome"] == "not_attempted"
            print(f"PASS: {count} schema-valid task results; missing runtime remains explicit.")
            return 0

        assert task["availability"]["status"] == "available"
        clean = run(*argv)
        validate(task["task"]["output_schema"], clean["data"])
        assert clean["execution"]["status"] == "succeeded"
        assert clean["data"]["findings"] == [] and clean["data"]["checked_panels"] == 1
        assert clean["data"]["input"]["digest"] == "sha256:" + hashlib.sha256(raw).hexdigest()
        assert clean["data"]["input"]["bytes"] == len(raw)
        assert clean["data"]["source"]["binding_digest"] == task["task"]["digest"]
        assert clean["data"]["source"]["checker_digest"] == (
            "sha256:8a109e921332b2da99885cda111ad6b6e7d22965291fd2bc4ddd0199876ec5b2"
        )
        assert clean["data"]["source"]["upstream_revision"] == "7d900fb999abee3b3712e1da881dcc7e80b1f6e7"

        bad = (ROOT / "tests/fixtures/dashboard-hygiene/findings.json").read_bytes()
        model.write_bytes(bad)
        findings = run(*argv)
        failing_policy = run(*argv, "--fail-on-findings", expected=1)
        for checked in (findings, failing_policy):
            assert checked["execution"]["status"] == "succeeded"
            assert [f["rule"] for f in checked["data"]["findings"]] == ["panel-description"]
            validate(task["task"]["output_schema"], checked["data"])
        assert model.read_bytes() == bad

        request = {
            "spec_version": "0.1", "request_id": "task-contract-001", "operation": "task.run",
            "operation_version": 1, "target": {"kind": "local", "id": "workstation"},
            "inputs": {"id": "dashboard-hygiene", "version": 1, "input": {"file": str(model)}},
            "limits": {"timeout_ms": 1000, "max_output_bytes": 1048576}, "record": "never",
        }
        request_file = scratch / "request.json"
        request_file.write_text(json.dumps(request), encoding="utf-8")
        called = run("call", "--request", str(request_file))
        for key in ("findings", "checked_panels", "input", "source", "task"):
            assert called["data"][key] == findings["data"][key], key
        for extra in ({"entrypoint": str(scratch / "evil.py")}, {"version": 999},
                      {"input": [str(model)]},
                      {"input": {"file": str(model), "runtime": "/bin/sh"}}):
            denied_request = copy.deepcopy(request)
            denied_request["inputs"].update(extra)
            request_file.write_text(json.dumps(denied_request), encoding="utf-8")
            denied = run("call", "--request", str(request_file), expected=2)
            assert denied["effect_outcome"] == "not_attempted"

        input_file.write_text(json.dumps([str(model)]), encoding="utf-8")
        denied = run(*argv, expected=2)
        assert denied["effect_outcome"] == "not_attempted"
        input_file.write_text(json.dumps({"file": str(model)}), encoding="utf-8")

        clean_model = json.loads(raw)
        for wrapped in ({"dashboard": clean_model, "meta": {}},
                        {"apiVersion": "dashboard.grafana.app/v1beta1", "kind": "Dashboard", "spec": clean_model}):
            model.write_text(json.dumps(wrapped), encoding="utf-8")
            assert run(*argv)["data"]["findings"] == []

        nonfinite = raw.rstrip().removesuffix(b"}") + b', "metadata": NaN}'
        for malformed in (b'{"panels":[]}', b'{"panels":[1]}', b'{"panels":null}', b'null', nonfinite,
                          b'{"panels":[],"panels":[]}', b'\xff', b'{not-json',
                          b'{"elements":{},"layout":{}}', b' ' * (2 * 1024 * 1024 + 1)):
            model.write_bytes(malformed)
            rejected = run(*argv, expected=(1, 2))
            assert rejected["execution"]["status"] != "succeeded"
            assert rejected["errors"] and rejected["coverage"]["state"] != "complete"
            assert model.read_bytes() == malformed

        model.write_bytes(raw)
        marker = scratch / "injected-module-ran"
        poison = f"from pathlib import Path\nPath({str(marker)!r}).write_text('bad')\nraise RuntimeError('workspace code ran')\n"
        for filename in ("json.py", "sitecustomize.py", "dashboard_hygiene.py"):
            (scratch / filename).write_text(poison, encoding="utf-8")
        poisoned_env = dict(os.environ, PYTHONPATH=str(scratch),
                            PYTHONSTARTUP=str(scratch / "sitecustomize.py"), TASK_SECRET="synthetic-secret-canary")
        safe = run(*argv, cwd=scratch, env=poisoned_env)
        assert not marker.exists() and safe["data"]["findings"] == []
        assert "synthetic-secret-canary" not in json.dumps(safe)
        assert model.read_bytes() == raw

    print(f"PASS: {count} schema-valid task results; fixtures, finding exits, call parity, "
          "admission, malformed/bounded input, source digest and workspace poisoning checks passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
