"""Installed opt-in toil acceptance; run through tools/test-sandbox.sh."""
from __future__ import annotations

import argparse
from datetime import datetime, timedelta, timezone
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

from jsonschema import Draft202012Validator, FormatChecker
from referencing import Registry, Resource

ROOT = Path(__file__).resolve().parents[1]


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--expected-binary-sha256", required=True)
    parser.add_argument("--source-manifest", type=Path, required=True)
    parser.add_argument("--expected-source-manifest-sha256", required=True)
    parser.add_argument("--evidence-dir", type=Path, required=True)
    options = parser.parse_args()
    assert os.environ.get("CARGO_HOME") == "/tmp/cargo" and Path("/toolchain").is_dir(), "Use tools/test-sandbox.sh"
    spec = importlib.util.spec_from_file_location("toil_evidence_helpers", ROOT / "tools/verify-evidence.py")
    helpers = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helpers)
    manifest = helpers.source_guard(ROOT, options.source_manifest, options.expected_source_manifest_sha256)
    assert manifest["tools/verify-toil.py"] == digest(Path(__file__).read_bytes())
    binary = options.binary.resolve(strict=True)
    assert digest(binary.read_bytes()) == options.expected_binary_sha256
    evidence = options.evidence_dir.resolve()
    evidence.mkdir(mode=0o700, parents=True, exist_ok=False)
    schemas = {p.stem.removesuffix(".schema"): helpers.strict_json(p.read_bytes())
               for p in (ROOT / "docs/sre-workbench/schemas").glob("*.schema.json")}
    registry = Registry().with_resources((s["$id"], Resource.from_contents(s)) for s in schemas.values())
    validators = {name: Draft202012Validator(schemas[name], registry=registry, format_checker=FormatChecker())
                  for name in ("result", "toil-data")}
    commands = []

    def retain(name, value):
        raw = value if isinstance(value, bytes) else (json.dumps(value, indent=2, sort_keys=True) + "\n").encode()
        path = evidence / name
        path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        path.write_bytes(raw)

    with tempfile.TemporaryDirectory(prefix="installed-toil-") as directory:
        scratch = Path(directory)
        installed = scratch / "prefix/bin/save"
        installed.parent.mkdir(parents=True)
        shutil.copy2(binary, installed)
        installed.chmod(0o755)
        cwd = scratch / "unrelated"
        cwd.mkdir()
        state = scratch / "store"
        now = datetime.now(timezone.utc).replace(microsecond=0)

        def utc(value):
            return value.isoformat().replace("+00:00", "Z")

        def run(name, args, expected=0, write=False):
            argv = [str(installed), "--json", "--toil-state-dir", str(state)]
            if write:
                argv += ["--record", "always"]
            output = subprocess.run([*argv, *args], cwd=cwd,
                env={"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8", "TZ": "UTC"},
                capture_output=True, timeout=15, check=False)
            retain(f"commands/{name}.stdout.json", output.stdout)
            retain(f"commands/{name}.stderr", output.stderr)
            commands.append({"case": name, "exit": output.returncode})
            assert output.returncode == expected and not output.stderr, (name, output.returncode, output.stdout[:2000], output.stderr[:300])
            assert output.stdout.endswith(b"\n") and output.stdout.count(b"\n") == 1
            result = helpers.strict_json(output.stdout)
            validators["result"].validate(result)
            assert result["assessment"] == "not_assessed"
            if result["data"].get("format_version") == 1:
                validators["toil-data"].validate(result["data"])
            return result

        def invoke(name, operation, inputs, expected=0, write=True):
            path = scratch / "input.json"
            path.write_text(json.dumps(inputs))
            return run(name, ["toil", operation, "--input", str(path)], expected, write)

        def configure(name, enabled, days=30):
            return run(name, ["toil", "configure", "--enabled", str(enabled).lower(), "--keep-days", str(days)], write=True)

        def sample(cohort, effort, observed=None):
            return {"workflow": "dashboard-review", "cohort": cohort,
                "observed_at": utc(observed or now - timedelta(hours=1)),
                "measurement": "measured", "outcome": "succeeded", "elapsed_ms": effort + 200,
                "effort_ms": effort, "retries": 0, "manual_interventions": 1}

        summary = {"workflow": "dashboard-review", "from": utc(now - timedelta(days=7)),
            "to": utc(now + timedelta(days=1)),
            "costs": {"build_ms": 1000, "maintenance_ms": 200, "incident_ms": 300, "measurement": "measured"},
            "projection": {"runs_per_day": 10, "horizon_days": 30}}
        capture = {"workflow": "dashboard-review", "cohort": "candidate",
            "task": {"id": "error-budget", "version": 1, "input": {"slo": 99.912345, "bad_minutes": 0}}}
        configure("disabled-initial", False)
        before = helpers.private_files(state)
        denied = invoke("disabled-capture", "capture", capture, 2)
        assert denied["effect_outcome"] == "not_attempted" and "child" not in denied["data"]
        assert helpers.private_files(state) == before
        configure("enabled", True)
        ids = []
        for index, (cohort, effort) in enumerate((("baseline", 1000), ("baseline", 2000), ("candidate", 500), ("candidate", 1000))):
            ids.append(invoke(f"record-{index}", "record", sample(cohort, effort))["data"]["sample"]["id"])
        before = helpers.private_files(state)
        result = invoke("summary-arithmetic", "summary", summary, write=False)["data"]
        assert result["cohorts"]["baseline"]["frequency"]["runs_per_day"] == 0.25
        assert result["cohorts"]["baseline"]["effort"]["mean_ms"] == 1500
        assert result["cohorts"]["candidate"]["effort"]["mean_ms"] == 750
        assert result["costs"]["total_ms"] == 1500
        assert result["comparison"]["gross_per_run_ms"] == 750
        assert result["comparison"]["observed_gross_ms"] == 1500
        assert result["comparison"]["observed_net_ms"] == 0
        assert result["comparison"]["break_even_runs"] == 2
        assert result["projection"]["runs"] == 300 and result["projection"]["net_ms"] == 223500
        assert result["projection"]["measurement"] == "estimated" and result["limitations"]
        assert helpers.private_files(state) == before
        captured = invoke("capture", "capture", capture)["data"]
        validators["result"].validate(captured["child"])
        assert captured["child"]["execution"]["status"] == "succeeded"
        assert captured["recording"]["state"] == "complete" and not captured["child_payload_omitted"]
        row = captured["sample"]
        assert row["source"] == "task_capture" and row["elapsed_measurement"] == "measured" and row["effort_ms"] is None
        for raw in helpers.private_files(state).values():
            assert all(value not in raw for value in (b"99.912345", b"bad_minutes", b"stdout", b"stderr", b"calculation"))
        missing = invoke("summary-missing-effort", "summary", summary, write=False)["data"]
        assert missing["comparison"]["state"] == "missing_effort" and missing["comparison"]["observed_net_ms"] is None
        invoke("annotate", "annotate", {"id": row["id"], "effort_ms": 250, "measurement": "estimated"})
        estimated = invoke("summary-estimated", "summary", summary, write=False)["data"]
        assert estimated["cohorts"]["candidate"]["samples"] == 3 and estimated["cohorts"]["candidate"]["effort"]["estimated"] == 1
        assert estimated["comparison"]["input_measurement"] == "estimated"
        before = helpers.private_files(state)
        invalid = sample("baseline", 1000)
        invalid["command"] = "must-not-persist"
        invoke("raw-content-refused", "record", invalid, 2)
        invoke("record-mode-refused", "record", sample("baseline", 1000), 2, write=False)
        assert before == helpers.private_files(state)
        old = invoke("old-sample", "record", sample("baseline", 1000, now - timedelta(days=2)))["data"]["sample"]["id"]
        configure("opt-out-short-retention", False, 1)
        before = helpers.private_files(state)
        invoke("opt-out-record", "record", sample("baseline", 1000), 2)
        invoke("opt-out-annotate", "annotate", {"id": row["id"], "effort_ms": 0, "measurement": "measured"}, 2)
        invoke("opt-out-capture", "capture", capture, 2)
        retained = invoke("summary-retention", "summary", summary, write=False)["data"]
        assert retained["expired_samples_excluded"] == 1 and retained["cohorts"]["baseline"]["samples"] == 2
        assert helpers.private_files(state) == before
        retain("store-before-prune-sha256.json", {name: digest(raw) for name, raw in before.items()})
        pruned = run("prune-disabled", ["toil", "prune"], write=True)
        assert pruned["data"]["deleted"] == [old] and pruned["data"]["remaining"] == 5
        deleted = run("delete-disabled", ["toil", "delete", row["id"]], write=True)
        assert deleted["data"]["deleted"] == [row["id"]] and deleted["data"]["remaining"] == 4
        request = {"spec_version": "0.1", "request_id": "toil-installed", "operation": "toil.summary",
            "operation_version": 1, "target": {"kind": "local", "id": "workstation"}, "inputs": summary,
            "limits": {"timeout_ms": 30000, "max_output_bytes": 1048576}, "record": "never"}
        path = scratch / "request.json"
        path.write_text(json.dumps(request))
        structured = run("structured-summary", ["call", "--request", str(path)])["data"]
        assert structured["comparison"] == result["comparison"]
        assert structured["cohorts"] == result["cohorts"]
        assert digest(installed.read_bytes()) == options.expected_binary_sha256
    helpers.source_guard(ROOT, options.source_manifest, options.expected_source_manifest_sha256)
    assert digest(binary.read_bytes()) == options.expected_binary_sha256
    retain("verification.json", {"passed": True, "commands": commands, "source_files": len(manifest),
        "source_manifest_sha256": options.expected_source_manifest_sha256, "binary_sha256": options.expected_binary_sha256,
        "installed_copy_outside_checkout": True, "no_credentials_or_network": True,
        "read_and_refusal_store_unchanged": True, "raw_task_input_not_persisted": True})
    print(f"PASS: {len(commands)} installed commands; consent, capture, privacy, effort arithmetic, retention and schemas")


if __name__ == "__main__":
    main()
