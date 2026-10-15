"""Installed offline-evidence acceptance; run only through tools/test-sandbox.sh."""
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
    parser.add_argument("--source-manifest", type=Path, required=True)
    parser.add_argument("--expected-source-manifest-sha256", required=True)
    parser.add_argument("--evidence-dir", type=Path, required=True)
    options = parser.parse_args()
    assert os.environ.get("CARGO_HOME") == "/tmp/cargo" and Path("/toolchain").is_dir(), "Use tools/test-sandbox.sh"
    helpers = load("offline_evidence_helpers", ROOT / "tools/verify-evidence.py")
    fixtures = load("offline_fixtures", ROOT / "tools/verify-comparison.py")
    certs = load("offline_certificates", ROOT / "tools/verify-grafana.py")
    manifest = helpers.source_guard(ROOT, options.source_manifest, options.expected_source_manifest_sha256)
    assert manifest["tools/verify-offline.py"] == digest(Path(__file__).read_bytes())
    binary = options.binary.resolve(strict=True)
    assert digest(binary.read_bytes()) == options.expected_binary_sha256
    evidence = options.evidence_dir.resolve()
    evidence.mkdir(mode=0o700, parents=True, exist_ok=False)
    schemas = {p.stem.removesuffix(".schema"): helpers.strict_json(p.read_bytes())
               for p in (ROOT / "docs/sre-workbench/schemas").glob("*.schema.json")}
    registry = Registry().with_resources((s["$id"], Resource.from_contents(s)) for s in schemas.values())
    validators = {name: Draft202012Validator(schemas[name], registry=registry, format_checker=FormatChecker())
                  for name in ("result", "evidence-offline-data")}
    commands = []
    baseline_threads = set(threading.enumerate())

    def retain(name, value):
        raw = value if isinstance(value, bytes) else (json.dumps(value, indent=2, sort_keys=True) + "\n").encode()
        assert fixtures.TOKEN.encode() not in raw
        path = evidence / name
        path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        path.write_bytes(raw)

    with tempfile.TemporaryDirectory(prefix="installed-offline-") as directory:
        scratch = Path(directory)
        installed = scratch / "prefix/bin/save"
        installed.parent.mkdir(parents=True)
        shutil.copy2(binary, installed)
        installed.chmod(0o755)
        assert digest(installed.read_bytes()) == options.expected_binary_sha256
        cwd = scratch / "unrelated"
        cwd.mkdir()
        state = scratch / "store"
        certificates = certs.certificates(scratch)
        fixture = fixtures.Fixture(certificates["server.pem"], certificates["server.key"])
        config, context = scratch / "operator.json", scratch / "context.json"
        config.write_text(json.dumps({"spec_version": "0.1", "record": "never", "connections": {"fixture": {
            "origin": fixture.origin, "expected_org_id": 7, "api_profile": "grafana-legacy-v1",
            "credential_ref": "env:" + fixtures.PROVIDER, "tls_ca_file": str(certificates["ca.pem"])}}}))
        context.write_text(json.dumps({"schema_version": 1, "source": {"kind": "fixture-export", "id": "offline", "revision": "1"},
            "records": [{"service": "fixture-web", "team": "platform", "environment": "dev", "deployment": "blue",
                "state": "active", "last_reviewed": datetime.now(timezone.utc).isoformat(),
                "required_bindings": ["target", "environment", "organization"],
                "bindings": {"target": "fixture", "environment": "dev", "organization": "7"}}]}))

        def run(name, args, expected=0, collect=False):
            argv = [str(installed), "--json", "--state-dir", str(state)]
            env = {"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8", "TZ": "UTC"}
            if collect:
                argv += ["--config", str(config), "--context-source", str(context), "--context-max-age-days", "30"]
                env[fixtures.PROVIDER] = json.dumps({"origin": fixture.origin, "expected_org_id": 7, "operations": ["grafana.query"],
                                                    "auth": {"kind": "bearer", "token": fixtures.TOKEN}})
            output = subprocess.run([*argv, *args], cwd=cwd, env=env, capture_output=True, timeout=15, check=False)
            retain(f"commands/{name}.stdout.json", output.stdout)
            retain(f"commands/{name}.stderr", output.stderr)
            commands.append({"case": name, "exit": output.returncode, "credential_available": collect})
            assert output.returncode == expected and not output.stderr, (name, output.returncode, output.stdout[:1600], output.stderr[:300])
            assert output.stdout.endswith(b"\n") and output.stdout.count(b"\n") == 1
            assert len(output.stdout) <= 2 * 1024 * 1024
            result = helpers.strict_json(output.stdout)
            fixtures.assert_finite(result)
            validators["result"].validate(result)
            assert result["assessment"] == "not_assessed"
            if result["operation"] in ("evidence.open", "evidence.replay") and "algorithm" in result["data"]:
                validators["evidence-offline-data"].validate(result["data"])
            return result

        try:
            path = scratch / "input.json"
            path.write_text(json.dumps({"service": "fixture-web", "environment": "dev", "pack": {"id": "service-queries", "version": 1},
                "window": {"kind": "absolute", "from": "2026-10-02T00:00:00Z", "to": "2026-10-02T00:05:00Z"},
                "metrics": {"datasource": "metrics-1", "parameters": {"job": "fixture"}},
                "logs": {"datasource": "logs-1", "parameters": {"job": "fixture"}}, "on_failure": "continue"}))
            collected = run("collect", ["--record", "always", "evidence", "collect", "--target", "fixture", "--input", str(path)], collect=True)
            # A real second collection retains an explicitly redacted observation.
            # The original receipt remains inspectable, but analysis is limited.
            normal_logs = fixtures.logs
            fixtures.logs = lambda shift, lines: normal_logs(shift, ["[REDACTED]"])
            redacted = run("collect-redacted", ["--record", "always", "evidence", "collect", "--target", "fixture", "--input", str(path)], collect=True)
        finally:
            fixture.close()
        assert len(fixture.events) == 12
        assert all(e["auth_matches"] and e["organization_matches"] for e in fixture.events)
        assert not (set(threading.enumerate()) - baseline_threads)
        config.unlink()
        context.unlink()
        before = helpers.private_files(state)
        bundle = collected["data"]["bundle"]
        select = [bundle["id"], "--expected-manifest-sha256", bundle["digest"]]
        opened = run("open", ["evidence", "open", *select])
        replay = run("replay", ["evidence", "replay", *select])
        repeated = run("repeat", ["evidence", "replay", *select])
        assert replay["data"]["checks"] == repeated["data"]["checks"]
        assert replay["data"]["observation"] == opened["data"]["observation"]
        assert replay["data"]["observation"]["window"]["from"] == "2026-10-02T00:00:00Z"
        assert replay["data"]["analysis"]["profile"] == "linux-offline-v1"
        checks = {c["id"]: c for c in replay["data"]["checks"]}
        assert checks["metrics"]["summary"]["samples"] == 4
        assert checks["metrics"]["summary"]["finite"] == 4
        assert checks["logs"]["summary"]["entries"] == 3
        assert checks["logs"]["summary"]["duplicate_entries"] == 1
        duplicate = next(d for d in checks["logs"]["details"] if d["line"] == "duplicate")
        assert duplicate["count"] == 2 and duplicate["timestamp_ns"] == "1790899200000000123"
        assert replay["sources"][0]["observed_at"] == opened["data"]["manifest"]["payload"]["started_at"]
        assert replay["sources"][0]["untrusted"] is True
        for check in opened["data"]["checks"]:
            original = opened["data"]["manifest"]["artifacts"][check["id"]]["path"]
            assert check["details"][0]["receipt"] == helpers.strict_json(before[f"runs/{bundle['id']}/{original}"])
        request = scratch / "request.json"
        request.write_text(json.dumps({"spec_version": "0.1", "request_id": "offline-installed", "operation": "evidence.replay",
            "operation_version": 1, "target": {"kind": "local", "id": "workstation"}, "inputs": {"id": bundle["id"]},
            "limits": {"timeout_ms": 30000, "max_output_bytes": 1048576, "max_items": 100}, "record": "never"}))
        structured = run("structured", ["call", "--request", str(request)])
        assert structured["data"]["checks"] == replay["data"]["checks"]
        for mode in ("open", "replay"):
            limited = run(mode + "-limited", ["evidence", mode, *select, "--max-items", "1"], 1)
            assert limited["data"]["state"] == "partial" and limited["data"]["omitted_details"] > 0
            assert sum(len(c["details"]) for c in limited["data"]["checks"]) == 1
            limited_source = run(mode + "-redacted", ["evidence", mode, redacted["data"]["bundle"]["id"]], 1)
            logs = next(c for c in limited_source["data"]["checks"] if c["id"] == "logs")
            assert logs["state"] == "incompatible" and "redacted_observation" in logs["reasons"]
            if mode == "open":
                assert len(logs["details"]) == 1 and logs["details"][0]["type"] == "receipt"
        run("wrong-pin", ["evidence", "replay", bundle["id"], "--expected-manifest-sha256", "sha256:" + "0" * 64], 1)
        assert before == helpers.private_files(state)
        retain("store-sha256.json", {name: digest(raw) for name, raw in before.items()})
        artifact = state / "runs" / bundle["id"] / opened["data"]["manifest"]["artifacts"]["metrics"]["path"]
        raw = artifact.read_bytes()
        artifact.write_bytes(b"[" + raw[1:])
        for mode in ("open", "replay"):
            refused = run(mode + "-tampered", ["evidence", mode, *select], 1)
            assert refused["execution"]["status"] == "failed" and "checks" not in refused["data"]
        artifact.write_bytes(raw)
        assert before == helpers.private_files(state)
        assert digest(installed.read_bytes()) == options.expected_binary_sha256
    helpers.source_guard(ROOT, options.source_manifest, options.expected_source_manifest_sha256)
    assert digest(binary.read_bytes()) == options.expected_binary_sha256
    retain("verification.json", {"passed": True, "commands": commands, "source_files": len(manifest),
        "source_manifest_sha256": options.expected_source_manifest_sha256, "binary_sha256": options.expected_binary_sha256,
        "fixture_requests": 12, "fixture_shutdown_before_analysis": True, "store_unchanged": True})
    print(f"PASS: {len(commands)} installed commands, canonical/schema-valid receipts, offline determinism, limits and tamper refusal")


if __name__ == "__main__":
    main()
