"""Frozen installed guided-runbook mission; use tools/test-sandbox.sh only."""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import threading
import time

from jsonschema import Draft202012Validator, FormatChecker
from referencing import Registry, Resource

ROOT = Path(__file__).resolve().parents[1]


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--expected-binary-sha256", required=True)
    parser.add_argument("--source-root", type=Path, default=ROOT)
    parser.add_argument("--source-manifest", type=Path, required=True)
    parser.add_argument("--expected-source-manifest-sha256", required=True)
    parser.add_argument("--evidence-dir", type=Path, required=True)
    parser.add_argument("--enospc-root", type=Path, help="Dedicated sandbox 64 KiB tmpfs only")
    options = parser.parse_args()
    assert os.environ.get("CARGO_HOME") == "/tmp/cargo" and Path("/toolchain").is_dir(), "Use tools/test-sandbox.sh"
    source = options.source_root.resolve(strict=True)
    helpers = load("runbook_evidence_helpers", source / "tools/verify-evidence.py")
    comparison = load("runbook_comparison_helpers", source / "tools/verify-comparison.py")
    certificates = load("runbook_certificate_helpers", source / "tools/verify-grafana.py")
    manifest = helpers.source_guard(source, options.source_manifest, options.expected_source_manifest_sha256)
    assert manifest["tools/verify-runbooks.py"] == digest(Path(__file__).read_bytes())
    binary = options.binary.resolve(strict=True)
    assert digest(binary.read_bytes()) == options.expected_binary_sha256
    evidence = options.evidence_dir.resolve()
    evidence.mkdir(mode=0o700, parents=True, exist_ok=False)
    schemas = {p.stem.removesuffix(".schema"): helpers.strict_json(p.read_bytes())
               for p in (source / "docs/sre-workbench/schemas").glob("*.schema.json")}
    registry = Registry().with_resources((s["$id"], Resource.from_contents(s)) for s in schemas.values())
    validators = {name: Draft202012Validator(schemas[name], registry=registry, format_checker=FormatChecker())
                  for name in ("result", "runbook-data", "evidence-collection-data", "evidence-manifest",
                               "evidence-collection-payload", "query-run-data", "evidence-comparison-data")}
    baseline_threads = set(threading.enumerate())
    commands, events, guards = [], [], []
    passed, fixture = False, None

    def retain(name, value):
        raw = value if isinstance(value, bytes) else (json.dumps(value, indent=2, sort_keys=True) + "\n").encode()
        assert comparison.TOKEN.encode() not in raw
        path = evidence / name
        path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        path.write_bytes(raw)

    def guard(stage):
        helpers.source_guard(source, options.source_manifest, options.expected_source_manifest_sha256)
        actual = digest(binary.read_bytes())
        assert actual == options.expected_binary_sha256
        guards.append({"stage": stage, "source_manifest_sha256": options.expected_source_manifest_sha256,
                       "source_files": len(manifest), "binary_sha256": actual})

    guard("before")
    try:
        with tempfile.TemporaryDirectory(prefix="installed-runbooks-") as directory:
            scratch = Path(directory)
            installed = scratch / "prefix/bin/save"
            installed.parent.mkdir(parents=True)
            shutil.copy2(binary, installed)
            installed.chmod(0o755)
            assert digest(installed.read_bytes()) == options.expected_binary_sha256
            cwd = scratch / "unrelated"
            cwd.mkdir()
            state, journal = scratch / "evidence-store", scratch / "runbook-journal"
            certs = certificates.certificates(scratch)
            fixture = comparison.Fixture(certs["server.pem"], certs["server.key"])
            config, context = scratch / "operator.json", scratch / "context.json"
            config.write_text(json.dumps({"spec_version": "0.1", "record": "never", "connections": {"fixture": {
                "origin": fixture.origin, "expected_org_id": 7, "api_profile": "grafana-legacy-v1",
                "credential_ref": "env:" + comparison.PROVIDER, "tls_ca_file": str(certs["ca.pem"])}}}))
            context.write_text(json.dumps({"schema_version": 1, "source": {"kind": "fixture-export", "id": "runbooks", "revision": "1"},
                "records": [{"service": "fixture-web", "team": "platform", "environment": "dev", "deployment": "blue",
                    "state": "active", "last_reviewed": datetime.now(timezone.utc).isoformat(),
                    "required_bindings": ["target", "environment", "organization"],
                    "bindings": {"target": "fixture", "environment": "dev", "organization": "7"}}]}))

            def run(name, arguments, expected=0, collect=False, record="never", journal_root=None):
                argv = [str(installed), "--json", "--state-dir", str(state), "--record", record]
                env = {"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8", "TZ": "UTC"}
                if collect:
                    argv += ["--config", str(config), "--context-source", str(context), "--context-max-age-days", "30"]
                    env[comparison.PROVIDER] = json.dumps({"origin": fixture.origin, "expected_org_id": 7,
                        "operations": ["grafana.query"], "auth": {"kind": "bearer", "token": comparison.TOKEN}})
                else:
                    argv += ["--runbook-state-dir", str(journal if journal_root is None else journal_root)]
                argv += arguments
                start = time.monotonic()
                output = subprocess.run(argv, cwd=cwd, env=env, capture_output=True, timeout=15, check=False)
                retain(f"commands/{name}.stdout.json", output.stdout)
                retain(f"commands/{name}.stderr", output.stderr)
                commands.append({"case": name, "argv": argv, "cwd": str(cwd), "exit_status": output.returncode,
                    "duration_ms": round((time.monotonic() - start) * 1000), "provider_available": collect})
                assert output.returncode == expected and not output.stderr, (name, output.returncode, output.stdout[:1800], output.stderr[:300])
                assert len(output.stdout) <= 2 * 1024 * 1024
                assert output.stdout.endswith(b"\n") and output.stdout.count(b"\n") == 1
                result = helpers.strict_json(output.stdout)
                comparison.assert_finite(result)
                validators["result"].validate(result)
                assert result["assessment"] == "not_assessed"
                if result["operation"] == "evidence.collect":
                    validators["evidence-collection-data"].validate(result["data"])
                if "journal" in result["data"]:
                    validators["runbook-data"].validate(result["data"])
                return result

            bundles = []
            try:
                for variant in range(3):
                    fixture.variant = variant
                    supplied = {"service": "fixture-web", "environment": "dev", "pack": {"id": "service-queries", "version": 1},
                        "window": {"kind": "absolute", "from": "2026-10-02T00:05:00Z" if variant == 2 else "2026-10-02T00:00:00Z",
                                   "to": "2026-10-02T00:10:00Z" if variant == 2 else "2026-10-02T00:05:00Z"},
                        "metrics": {"datasource": "metrics-1", "parameters": {"job": "fixture"}},
                        "logs": {"datasource": "logs-1", "parameters": {"job": "fixture"}}, "on_failure": "continue"}
                    path = scratch / "collection.json"
                    path.write_text(json.dumps(supplied))
                    retain(f"collection-input-{variant}.json", supplied)
                    value = run(f"collect-{variant}", ["evidence", "collect", "--target", "fixture", "--input", str(path)], collect=True, record="always")
                    assert value["data"]["capture"]["state"] == "complete"
                    bundles.append(value["data"]["bundle"])
            finally:
                fixture.close()
                events.extend(fixture.events)
            assert len(events) == 18 and all(e["auth_matches"] and e["organization_matches"] for e in events)
            assert not (set(threading.enumerate()) - baseline_threads), "Fixture workers did not join"
            config.unlink()
            context.unlink()
            before = helpers.private_files(state)
            retain("store-before-sha256.json", {name: digest(raw) for name, raw in before.items()})
            for name, raw in before.items():
                retain("store/" + name, raw)
                if name.endswith("manifest.json"):
                    value = helpers.strict_json(raw)
                    validators["evidence-manifest"].validate(value)
                    validators["evidence-collection-payload"].validate(value["payload"])
                if "/artifacts/" in name:
                    value = helpers.strict_json(raw)
                    validators["result"].validate(value)
                    validators["query-run-data"].validate(value["data"])

            def snapshot(label, root=None):
                files = helpers.private_files(journal if root is None else root)
                for name, raw in files.items():
                    retain(f"journal/{label}/{name}", raw)
                    if name.startswith("run-") and name.endswith(".json"):
                        saved = helpers.strict_json(raw)
                        for step in saved["steps"]:
                            receipt = step.get("receipt")
                            if receipt is not None:
                                validators["result"].validate(receipt)
                                assert receipt["assessment"] == "not_assessed"
                                assert receipt["record_mode"] == "never"
                                if receipt["operation"] == "evidence.compare":
                                    validators["evidence-comparison-data"].validate(receipt["data"])
                retain(f"journal/{label}-sha256.json", {name: digest(raw) for name, raw in files.items()})
                return files

            def supplied(right):
                return {"before": {"id": bundles[0]["id"], "expected_manifest_sha256": bundles[0]["digest"]},
                        "after": {"id": right["id"], "expected_manifest_sha256": right["digest"]},
                        "normalization": "exact-window-v1", "on_failure": "stop"}

            input_path = scratch / "review.json"
            original = supplied(bundles[1])
            input_path.write_text(json.dumps(original))
            retain("runbook-input.json", original)
            initial = run("run-waiting", ["runbook", "run", "evidence-review", "--input", str(input_path)], 1, record="always")
            data = initial["data"]
            assert data["workflow_state"] == "waiting" and data["pending_checkpoint"] == "review-complete"
            assert data["observation_state"] == "complete"
            assert data["checkpoint_instructions"]
            checks = {check["id"]: check for check in data["comparison"]["checks"]}
            assert checks["metrics"]["summary"]["samples_changed"] == 1
            assert checks["logs"]["summary"]["entries_added"] == 1
            assert checks["logs"]["summary"]["entries_removed"] == 2
            assert initial["record_mode"] == "always" and initial["effect_outcome"] == "succeeded"
            reference = data["journal"]
            waiting_bytes = snapshot("waiting")
            children = [s["receipt_summary"] for s in data["steps"] if s["receipt_summary"] is not None]
            assert len(children) == 3 and len({c["run_id"] for c in children}) == 3
            status = run("status-waiting", ["runbook", "status", reference["id"]])
            assert status["data"] == data
            assert status["record_mode"] == "never" and status["effect_outcome"] == "not_applicable"
            assert snapshot("after-status") == waiting_bytes

            def resume(name, revision, ack="review-complete", expected=2):
                return run(name, ["runbook", "resume", reference["id"], "--expected-revision", str(revision),
                    "--input", str(input_path), "--acknowledge", ack], expected, record="always")

            for name, revision, ack, code in (
                ("wrong-checkpoint", reference["revision"], "review-partial", "runbook_checkpoint_mismatch"),
                ("stale-revision", reference["revision"] - 1, "review-complete", "runbook_revision_mismatch")):
                refused = resume(name, revision, ack)
                assert code in {e["code"] for e in refused["errors"]}
                assert snapshot(name) == waiting_bytes
            input_path.write_text(json.dumps({**original, "on_failure": "continue"}))
            refused = resume("changed-input", reference["revision"])
            assert "runbook_input_mismatch" in {e["code"] for e in refused["errors"]}
            assert snapshot("changed-input") == waiting_bytes
            input_path.write_text(json.dumps(original))
            completed = resume("resume-completed", reference["revision"], expected=0)
            final = completed["data"]
            assert final["workflow_state"] == "completed" and final["pending_checkpoint"] is None
            assert final["observation_state"] == "complete"
            assert completed["record_mode"] == "always" and completed["effect_outcome"] == "succeeded"
            assert completed["coverage"]["state"] == "not_established"
            assert [s["receipt_summary"] for s in final["steps"] if s["receipt_summary"] is not None] == children
            completed_bytes = snapshot("completed")
            saved_name = reference["id"] + ".json"
            original_receipts = [s["receipt"] for s in helpers.strict_json(waiting_bytes[saved_name])["steps"]]
            resumed_receipts = [s["receipt"] for s in helpers.strict_json(completed_bytes[saved_name])["steps"]]
            assert resumed_receipts == original_receipts, "Resume rewrote retained child receipts"
            refused = resume("replayed-old-revision", reference["revision"])
            assert "runbook_revision_mismatch" in {e["code"] for e in refused["errors"]}
            assert snapshot("after-replayed-revision") == completed_bytes
            fresh = run("fresh-attempt", ["runbook", "run", "evidence-review", "--input", str(input_path),
                "--fresh-from", reference["id"], "--fresh-from-revision", str(final["journal"]["revision"])], 1, record="always")
            assert fresh["data"]["workflow_state"] == "waiting"
            assert fresh["data"]["journal"]["id"] != reference["id"]
            assert fresh["data"]["fresh_from"] == {"id": reference["id"], "expected_revision": final["journal"]["revision"]}
            assert {s["receipt_summary"]["run_id"] for s in fresh["data"]["steps"] if s["receipt_summary"]}.isdisjoint(c["run_id"] for c in children)
            fresh_bytes = snapshot("fresh")
            assert all(fresh_bytes[name] == raw for name, raw in completed_bytes.items())
            input_path.write_text(json.dumps(supplied(bundles[2])))
            retain("incompatible-input.json", supplied(bundles[2]))
            incompatible = run("incompatible-branch", ["runbook", "run", "evidence-review", "--input", str(input_path)], 1, record="always")
            assert incompatible["data"]["pending_checkpoint"] == "review-incompatible"
            assert incompatible["data"]["observation_state"] == "incompatible"
            snapshot("incompatible")
            incompatible_ref = incompatible["data"]["journal"]
            acknowledged = run("incompatible-acknowledged", ["runbook", "resume", incompatible_ref["id"],
                "--expected-revision", str(incompatible_ref["revision"]), "--input", str(input_path),
                "--acknowledge", "review-incompatible"], 1, record="always")
            assert acknowledged["data"]["workflow_state"] == "completed"
            assert acknowledged["data"]["observation_state"] == "incompatible"
            assert acknowledged["execution"]["status"] == "partial"
            assert acknowledged["coverage"]["state"] == "partial"
            snapshot("incompatible-acknowledged")
            if options.enospc_root is not None:
                small_root = options.enospc_root.resolve(strict=True)
                assert str(small_root) == "/tmp/evidence-small-disk", "Only the dedicated sandbox mount is accepted"
                mounts = Path("/proc/self/mountinfo").read_text().splitlines()
                assert any(line.split()[4] == str(small_root) and " - tmpfs " in line for line in mounts)
                stats = os.statvfs(small_root)
                capacity = stats.f_blocks * stats.f_frsize
                assert 0 < capacity <= 64 * 1024, "Disk-full test requires bounded private tmpfs"
                assert stats.f_frsize == 4096 and stats.f_bavail * stats.f_frsize == capacity
                assert not any(small_root.iterdir()), "Disk-full fixture must start empty"
                filler = small_root / "bounded-fixture-filler"
                try:
                    with filler.open("xb") as stream:
                        stream.write(b"F" * (capacity - stats.f_frsize))
                        stream.flush()
                        os.fsync(stream.fileno())
                    assert os.statvfs(small_root).f_bavail == 1
                    input_path.write_text(json.dumps(original))
                    small_journal = small_root / "journal"
                    failed = run("disk-full", ["runbook", "run", "evidence-review", "--input", str(input_path)],
                        1, record="always", journal_root=small_journal)
                    assert "store_no_space" in {e["code"] for e in failed["errors"]}
                    assert failed["record_mode"] == "always"
                    assert failed["effect_outcome"] in ("failed", "unknown")
                    assert failed["data"].get("workflow_state") != "waiting"
                    assert failed["data"].get("pending_checkpoint") is None
                    small_files = snapshot("disk-full", small_journal)
                    assert "format.json" in small_files, "Expected format header to persist before state ENOSPC"
                    assert not any(name.startswith("run-") for name in small_files)
                    retain("disk-full-capacity.json", {"filesystem_bytes": capacity,
                        "filler_bytes": filler.stat().st_size, "free_bytes_after": os.statvfs(small_root).f_bavail * stats.f_frsize})
                finally:
                    filler.unlink(missing_ok=True)
            after = helpers.private_files(state)
            assert after == before, "Runbook execution changed evidence bytes"
            retain("store-after-sha256.json", {name: digest(raw) for name, raw in after.items()})
            assert len(fixture.events) == 18
            assert digest(installed.read_bytes()) == options.expected_binary_sha256
        passed = True
    finally:
        if fixture is not None:
            fixture.close()
        guard("after")
        joined = not (set(threading.enumerate()) - baseline_threads)
        retain("commands.json", commands)
        retain("http-events.json", events)
        retain("source-binary-guards.json", guards)
        retain("verification-summary.json", {"passed": passed, "commands": len(commands), "http_requests": len(events),
            "fixture_threads_joined": joined, "disk_full_executed": options.enospc_root is not None, "offline_runbooks": "fixture closed, workers joined, configuration/context removed, provider absent",
            "source_manifest_sha256": options.expected_source_manifest_sha256, "binary_sha256": options.expected_binary_sha256})
        assert joined, "Fixture worker remained alive"
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
