"""Installed evidence-report acceptance; run only through tools/test-sandbox.sh."""
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
    helpers = load("report_evidence_helpers", ROOT / "tools/verify-evidence.py")
    fixtures = load("report_fixtures", ROOT / "tools/verify-comparison.py")
    certs = load("report_certificates", ROOT / "tools/verify-grafana.py")
    manifest = helpers.source_guard(ROOT, options.source_manifest, options.expected_source_manifest_sha256)
    assert manifest["tools/verify-reports.py"] == digest(Path(__file__).read_bytes())
    binary = options.binary.resolve(strict=True)
    assert digest(binary.read_bytes()) == options.expected_binary_sha256
    evidence = options.evidence_dir.resolve()
    evidence.mkdir(mode=0o700, parents=True, exist_ok=False)
    schemas = {p.stem.removesuffix(".schema"): helpers.strict_json(p.read_bytes())
               for p in (ROOT / "docs/sre-workbench/schemas").glob("*.schema.json")}
    registry = Registry().with_resources((s["$id"], Resource.from_contents(s)) for s in schemas.values())
    validators = {name: Draft202012Validator(schemas[name], registry=registry, format_checker=FormatChecker())
                  for name in ("result", "report-data")}
    commands = []
    baseline_threads = set(threading.enumerate())

    def retain(name, value):
        raw = value if isinstance(value, bytes) else (json.dumps(value, indent=2, sort_keys=True) + "\n").encode()
        assert fixtures.TOKEN.encode() not in raw
        path = evidence / name
        path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        path.write_bytes(raw)

    with tempfile.TemporaryDirectory(prefix="installed-report-") as directory:
        scratch = Path(directory)
        installed = scratch / "prefix/bin/save"
        installed.parent.mkdir(parents=True)
        shutil.copy2(binary, installed)
        installed.chmod(0o755)
        cwd = scratch / "unrelated"
        cwd.mkdir()
        state = scratch / "store"
        certificates = certs.certificates(scratch)
        fixture = fixtures.Fixture(certificates["server.pem"], certificates["server.key"])
        config, context = scratch / "operator.json", scratch / "context.json"
        config.write_text(json.dumps({"spec_version": "0.1", "record": "never", "connections": {"fixture": {
            "origin": fixture.origin, "expected_org_id": 7, "api_profile": "grafana-legacy-v1",
            "credential_ref": "env:" + fixtures.PROVIDER, "tls_ca_file": str(certificates["ca.pem"])}}}))
        context.write_text(json.dumps({"schema_version": 1, "source": {"kind": "fixture-export", "id": "reports", "revision": "1"},
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
            output = subprocess.run([*argv, *args], cwd=cwd, env=env, capture_output=True, timeout=20, check=False)
            retain(f"commands/{name}.stdout.json", output.stdout)
            retain(f"commands/{name}.stderr", output.stderr)
            commands.append({"case": name, "exit": output.returncode, "credential_available": collect})
            assert output.returncode == expected and not output.stderr, (name, output.returncode, output.stdout[:2000], output.stderr[:300])
            assert output.stdout.endswith(b"\n") and output.stdout.count(b"\n") == 1
            assert len(output.stdout) <= 2 * 1024 * 1024
            result = helpers.strict_json(output.stdout)
            fixtures.assert_finite(result)
            validators["result"].validate(result)
            assert result["assessment"] == "not_assessed"
            if result["operation"] == "report.create" and "report" in result["data"]:
                validators["report-data"].validate(result["data"])
            return result

        bundles = []
        try:
            path = scratch / "input.json"
            path.write_text(json.dumps({"service": "fixture-web", "environment": "dev", "pack": {"id": "service-queries", "version": 1},
                "window": {"kind": "absolute", "from": "2026-10-02T00:00:00Z", "to": "2026-10-02T00:05:00Z"},
                "metrics": {"datasource": "metrics-1", "parameters": {"job": "fixture"}},
                "logs": {"datasource": "logs-1", "parameters": {"job": "fixture"}}, "on_failure": "continue"}))
            for variant in range(3):
                fixture.variant = min(variant, 1)
                if variant == 2:
                    normal_logs = fixtures.logs
                    fixtures.logs = lambda shift, lines: normal_logs(shift, ["[REDACTED]"])
                collected = run(f"collect-{variant}", ["--record", "always", "evidence", "collect", "--target", "fixture", "--input", str(path)], collect=True)
                bundles.append(collected["data"]["bundle"])
        finally:
            fixture.close()
        assert len(fixture.events) == 18 and all(e["auth_matches"] and e["organization_matches"] for e in fixture.events)
        assert not (set(threading.enumerate()) - baseline_threads)
        config.unlink()
        context.unlink()
        before = helpers.private_files(state)

        def report(name, selection, format="markdown", expected=0, extra=(), proposals=None):
            inputs = {"bundles": [{"id": bundles[i]["id"], "expected_manifest_sha256": bundles[i]["digest"]} for i in selection], "format": format}
            if proposals is not None:
                inputs["proposals"] = proposals
            path = scratch / "report.json"
            path.write_text(json.dumps(inputs))
            return run(name, ["report", "create", "--input", str(path), *extra], expected)

        markdown = report("markdown", [0, 1])
        json_result = report("json", [0, 1], "json")
        assert markdown["data"]["report"]["comparisons"][0]["finding"] == "disagreement"
        assert json.loads(json_result["data"]["rendering"]["content"]) == json_result["data"]["report"]
        for result in (markdown, json_result):
            source = result["data"]["report"]["sources"][0]
            observation = result["data"]["report"]["observations"][0]
            assert source["bundle"] == bundles[0] and source["untrusted"] is True and source["evidence_state"] == "sourced"
            assert source["observed_at"] == observation["started_at"]
            assert observation["window"]["from"] == "2026-10-02T00:00:00Z"
        left, right = markdown["data"]["report"].copy(), json_result["data"]["report"].copy()
        left.pop("generated_at")
        right.pop("generated_at")
        assert left == right
        retain("handoff.md", markdown["data"]["rendering"]["content"].encode())
        partial = report("redacted", [0, 2], expected=1)
        assert partial["data"]["report"]["state"] == "partial"
        limited = report("limited", [0, 1], expected=1, extra=("--max-output-bytes", "1024"))
        assert limited["data"]["rendering"]["state"] == "omitted" and limited["data"]["rendering"]["content"] is None
        content = "# Approved\n<script>untrusted</script>\n[execute](javascript:alert(1))"
        proposed = report("proposals", [0], proposals=[{"kind": "runbook", "title": "Pending <b>proposal</b>", "content": content, "evidence": [bundles[0]["id"]]}])
        proposal = proposed["data"]["report"]["knowledge_proposals"][0]
        assert proposal["review_status"] == "pending" and proposal["provenance"] == "operator_proposed"
        assert proposal["content"] == content and proposal["evidence"] == [bundles[0]]
        for line in proposed["data"]["rendering"]["content"].splitlines():
            if "<script>" in line or "javascript:" in line:
                assert line.startswith("    "), "Untrusted Markdown escaped its code block"
        request = scratch / "request.json"
        request.write_text(json.dumps({"spec_version": "0.1", "request_id": "report-installed", "operation": "report.create",
            "operation_version": 1, "target": {"kind": "local", "id": "workstation"},
            "inputs": {"bundles": [{"id": b["id"]} for b in bundles[:2]], "format": "json"},
            "limits": {"timeout_ms": 30000, "max_output_bytes": 1048576}, "record": "never"}))
        structured = run("structured", ["call", "--request", str(request)])
        structured_report = structured["data"]["report"].copy()
        structured_report.pop("generated_at")
        assert structured_report == right
        assert before == helpers.private_files(state)
        retain("store-sha256.json", {name: digest(raw) for name, raw in before.items()})
        run_dir = state / "runs" / bundles[0]["id"]
        stored_manifest = helpers.strict_json((run_dir / "manifest.json").read_bytes())
        artifact = run_dir / stored_manifest["artifacts"]["metrics"]["path"]
        raw = artifact.read_bytes()
        artifact.write_bytes(b"[" + raw[1:])
        refused = report("tampered", [0], expected=1)
        assert refused["execution"]["status"] == "failed" and "report" not in refused["data"]
        artifact.write_bytes(raw)
        assert before == helpers.private_files(state)
        assert digest(installed.read_bytes()) == options.expected_binary_sha256
    helpers.source_guard(ROOT, options.source_manifest, options.expected_source_manifest_sha256)
    assert digest(binary.read_bytes()) == options.expected_binary_sha256
    retain("verification.json", {"passed": True, "commands": commands, "source_files": len(manifest),
        "source_manifest_sha256": options.expected_source_manifest_sha256, "binary_sha256": options.expected_binary_sha256,
        "fixture_requests": 18, "fixture_shutdown_before_reports": True, "store_unchanged": True})
    print(f"PASS: {len(commands)} installed commands, schema-valid reports, disagreements, proposals, bounds and tamper refusal")


if __name__ == "__main__":
    main()
