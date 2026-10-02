"""Black-box PoC/schema acceptance. Run inside tools/test-sandbox.sh on Linux."""

from __future__ import annotations

import argparse
import copy
import json
import subprocess
import tempfile
from pathlib import Path

from jsonschema import Draft202012Validator, FormatChecker
from referencing import Registry, Resource


ROOT = Path(__file__).resolve().parents[1]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    schemas = [json.loads(path.read_text(encoding="utf-8"))
               for path in (ROOT / "docs/sre-workbench/schemas").glob("*.schema.json")]
    registry = Registry().with_resources((s["$id"], Resource.from_contents(s)) for s in schemas)
    schema = next(s for s in schemas if s["$id"] == "urn:sre-workbench:spec:result:0.1")
    validator = Draft202012Validator(schema, registry=registry, format_checker=FormatChecker())
    checked = 0

    def invoke(*argv: str, expected: int = 0) -> dict:
        nonlocal checked
        run = subprocess.run([str(binary), "--json", *argv], capture_output=True, timeout=8, check=False)
        if run.returncode != expected:
            raise RuntimeError(f"{argv[:2]}: exit {run.returncode}, expected {expected}; "
                               f"stderr={run.stderr[:500]!r}")
        if len(run.stdout) > 2 * 1024 * 1024:
            raise RuntimeError("Encoded result exceeds 2 MiB")
        result = json.loads(run.stdout)
        validator.validate(result)
        if result["assessment"] != "not_assessed" or result["record_mode"] != "never":
            raise RuntimeError("PoC claims assessment or recording")
        checked += 1
        return result

    with tempfile.TemporaryDirectory(prefix="workbench-contract-") as directory:
        scratch = Path(directory)
        marker = scratch / "shell-marker"
        literal = f"hello workbench; $(touch {marker}) | * ' \" café"
        mission = invoke("exec", "--cwd", str(scratch), "--", "/usr/bin/printf", "%s\\n", literal)
        assert mission["output"]["stdout"] == literal + "\n"
        assert mission["execution"]["child_exit_code"] == 0
        assert mission["execution"]["status"] == "succeeded"
        assert mission["effect_outcome"] == "unknown"
        assert Path(mission["resolved_target"]["cwd"]) == scratch
        assert not marker.exists()
        text_run = subprocess.run(
            [str(binary), "exec", "--cwd", str(scratch), "--", "/usr/bin/printf", "%s\\n", literal],
            capture_output=True, text=True, timeout=8, check=False,
        )
        assert text_run.returncode == 0 and literal in text_run.stdout

        request = {
            "spec_version": "0.1", "request_id": "contract-mission-001", "operation": "process.exec",
            "operation_version": 1, "target": {"kind": "local", "id": "workstation"},
            "inputs": {"program": "/usr/bin/printf", "args": ["%s\\n", literal], "cwd": str(scratch)},
            "limits": {"timeout_ms": 1000, "max_output_bytes": 1024}, "record": "never",
        }
        request_file = scratch / "request.json"
        request_file.write_text(json.dumps(request), encoding="utf-8")
        structured = invoke("call", "--request", str(request_file))
        for key in ("operation", "output", "execution", "assessment", "effect_outcome"):
            assert structured[key] == mission[key], key

        for changes in ({"role": "admin"}, {"operation_version": 999}, {"record": "always"},
                        {"target": {"kind": "runner", "id": "elsewhere"}},
                        {"inputs": {"program": "/usr/bin/touch", "args": [str(marker)], "cwd": "."}}):
            denied_request = copy.deepcopy(request)
            denied_request.update(changes)
            request_file.write_text(json.dumps(denied_request), encoding="utf-8")
            denied = invoke("call", "--request", str(request_file), expected=2)
            assert denied["effect_outcome"] == "not_attempted"
            assert not marker.exists()

        marker_request = copy.deepcopy(request)
        marker_request["inputs"] = {"program": "/usr/bin/touch", "args": [str(marker)], "cwd": str(scratch)}
        request_file.write_text(json.dumps(marker_request), encoding="utf-8")
        invoke("call", "--request", str(request_file))
        assert marker.exists(), "Positive control must prove the marker command can run"
        marker.unlink()
        malformed_shapes = [list(marker_request.values())]
        for field, sequence in (("target", ["local", "workstation"]),
                                ("limits", [1000, 1024]),
                                ("inputs", ["/usr/bin/touch", [str(marker)], str(scratch)])):
            malformed = copy.deepcopy(marker_request)
            malformed[field] = sequence
            malformed_shapes.append(malformed)
        for malformed in malformed_shapes:
            request_file.write_text(json.dumps(malformed), encoding="utf-8")
            denied = invoke("call", "--request", str(request_file), expected=2)
            assert denied["effect_outcome"] == "not_attempted"
            assert not marker.exists(), "Sequence-encoded object must not dispatch a child"

        inspected = invoke("command", "inspect", "--cwd", str(scratch), "--", "/usr/bin/touch", str(marker))
        assert inspected["effect_outcome"] == "not_applicable"
        assert inspected["data"]["execution_performed"] is False and not marker.exists()
        failure = invoke("exec", "--cwd", str(scratch), "--", "/usr/bin/false", expected=1)
        assert failure["execution"]["status"] == "failed"
        assert failure["execution"]["child_exit_code"] == 1
        noisy = invoke("exec", "--cwd", str(scratch), "--max-output-bytes", "1024",
                       "--", "/usr/bin/printf", "%4096s", "x", expected=1)
        assert noisy["execution"]["status"] == "partial"
        assert noisy["output"]["stdout_truncated"]
        assert len(noisy["output"]["stdout"].encode()) <= 1024
        timed = invoke("exec", "--cwd", str(scratch), "--timeout", "100ms", "--", "/usr/bin/sleep", "5", expected=1)
        assert timed["execution"]["status"] == "timed_out"
        for argv in (("capabilities", "list"), ("capabilities", "describe", "process.exec"), ("doctor",)):
            invoke(*argv)

    print(f"PASS: {checked} real CLI results validate against the draft result schema; "
          "literal arguments, text/call parity, denial, inspection, failure, limits and discovery passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
