"""Frozen installed comparison mission; run only through tools/test-sandbox.sh."""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import http.server
import importlib.util
import json
import math
import os
from pathlib import Path
import shutil
import ssl
import subprocess
import tempfile
import threading
import time

from jsonschema import Draft202012Validator, FormatChecker
from referencing import Registry, Resource

ROOT = Path(__file__).resolve().parents[1]
TOKEN = "fixture-only-evidence-secret-e99e314b"
PROVIDER = "SAVE_GRAFANA_COMPARISON_FIXTURE"
START = 1790899200000


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def assert_finite(value):
    if isinstance(value, float):
        assert math.isfinite(value), "Non-finite decoded JSON number"
    elif isinstance(value, dict):
        for child in value.values():
            assert_finite(child)
    elif isinstance(value, list):
        for child in value:
            assert_finite(child)


def metric(instance, shift, values):
    return {"schema": {"refId": "A", "meta": {"type": "timeseries-multi", "typeVersion": [0, 1]},
            "fields": [{"name": "Time", "type": "time", "config": {"interval": 1000}},
                       {"name": "up", "type": "number", "labels": {"job": "fixture", "instance": instance},
                        "config": {"unit": "none"}}]},
            "data": {"values": [[START + shift, START + shift + 1000], values]}}


def logs(shift, lines):
    length = len(lines)
    return {"schema": {"meta": {"typeVersion": [0, 0], "custom": {"frameType": "LabeledTimeValues"}},
            "fields": [{"name": "labels", "type": "other"}, {"name": "Time", "type": "time"},
                       {"name": "Line", "type": "string"}, {"name": "tsNs", "type": "string"},
                       {"name": "id", "type": "string"}]},
            "data": {"values": [[{"job": "fixture"}] * length, [START + shift] * length, lines,
                                [str((START + shift) * 1000000 + 123)] * length,
                                [f"generated-{i}" for i in range(length)]],
                     "nanos": [None, [123] * length, None, None, None]}}


class Fixture(http.server.ThreadingHTTPServer):
    daemon_threads = False
    block_on_close = True

    def __init__(self, certificate, key):
        self.events = []
        self.closed = False
        self.variant = 0
        super().__init__(("127.0.0.1", 0), Handler)
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.load_cert_chain(certificate, key)
        self.socket = context.wrap_socket(self.socket, server_side=True)
        self.origin = f"https://127.0.0.1:{self.server_port}/grafana"
        self.thread = threading.Thread(target=self.serve_forever, kwargs={"poll_interval": 0.02})
        self.thread.start()

    def close(self):
        if self.closed:
            return
        self.shutdown()
        self.server_close()
        self.thread.join(timeout=2)
        assert not self.thread.is_alive(), "HTTPS listener did not join"
        self.closed = True


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_args):
        pass

    def do_GET(self):
        self.exchange()

    def do_POST(self):
        self.exchange()

    def exchange(self):
        self.connection.settimeout(5)
        size = int(self.headers.get("Content-Length", "0"))
        assert 0 <= size <= 128 * 1024
        body = self.rfile.read(size)
        fixture = self.server
        event = {"method": self.command, "path": self.path,
                 "body": json.loads(body) if body else None,
                 "auth_matches": self.headers.get_all("Authorization", []) == ["Bearer " + TOKEN],
                 "organization_matches": self.headers.get_all("X-Grafana-Org-Id", []) == ["7"]}
        fixture.events.append(event)
        status = 200
        if self.command == "GET" and self.path == "/grafana/api/org":
            response = {"id": 7, "name": "synthetic"}
        elif self.command == "GET" and self.path in ("/grafana/api/datasources/uid/metrics-1?ds_type=prometheus", "/grafana/api/datasources/uid/logs-1?ds_type=loki"):
            uid = self.path.split("?", 1)[0].rsplit("/", 1)[1]
            response = {"uid": uid, "type": "prometheus" if uid == "metrics-1" else "loki", "orgId": 7}
        elif self.command == "POST" and self.path == "/grafana/api/ds/query":
            query = event["body"]["queries"][0]
            shift = 300000 if fixture.variant == 2 else 0
            if query["datasource"]["type"] == "prometheus":
                frames = [metric("a", shift, [1, 1] if fixture.variant == 0 else [1, 0]),
                          metric("removed" if fixture.variant == 0 else "added", shift, [0, 1])]
            else:
                frames = [logs(shift, ["duplicate", "duplicate", "removed"] if fixture.variant == 0 else ["duplicate", "added"])]
            response = {"results": {"A": {"status": 200, "frames": frames}}}
        else:
            status, response = 404, {"error": "unexpected fixture route"}
        raw = json.dumps(response).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(raw)))
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(raw)
        self.close_connection = True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--expected-binary-sha256", required=True)
    parser.add_argument("--source-root", type=Path, default=ROOT)
    parser.add_argument("--source-manifest", type=Path, required=True)
    parser.add_argument("--expected-source-manifest-sha256", required=True)
    parser.add_argument("--evidence-dir", type=Path, required=True)
    options = parser.parse_args()
    assert os.environ.get("CARGO_HOME") == "/tmp/cargo" and Path("/toolchain").is_dir(), "Use tools/test-sandbox.sh"
    source = options.source_root.resolve(strict=True)
    helpers = load("comparison_evidence_helpers", source / "tools/verify-evidence.py")
    certificates = load("comparison_certificate_helpers", source / "tools/verify-grafana.py")
    manifest = helpers.source_guard(source, options.source_manifest, options.expected_source_manifest_sha256)
    assert manifest["tools/verify-comparison.py"] == digest(Path(__file__).read_bytes())
    binary = options.binary.resolve(strict=True)
    assert digest(binary.read_bytes()) == options.expected_binary_sha256
    evidence = options.evidence_dir.resolve()
    evidence.mkdir(mode=0o700, parents=True, exist_ok=False)
    schemas = {p.stem.removesuffix(".schema"): helpers.strict_json(p.read_bytes())
               for p in (source / "docs/sre-workbench/schemas").glob("*.schema.json")}
    registry = Registry().with_resources((s["$id"], Resource.from_contents(s)) for s in schemas.values())
    validators = {name: Draft202012Validator(schemas[name], registry=registry, format_checker=FormatChecker())
                  for name in ("result", "evidence-collection-data", "evidence-comparison-data", "evidence-manifest", "evidence-collection-payload", "query-run-data")}
    baseline_threads = set(threading.enumerate())
    commands, events, guards = [], [], []
    passed = False
    fixture = None

    def retain(name, value):
        raw = value if isinstance(value, bytes) else (json.dumps(value, indent=2, sort_keys=True) + "\n").encode()
        assert TOKEN.encode() not in raw
        path = evidence / name
        path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        path.write_bytes(raw)

    guards.append({"stage": "before", "source_manifest_sha256": options.expected_source_manifest_sha256,
                   "source_files": len(manifest), "binary_sha256": digest(binary.read_bytes())})
    try:
        with tempfile.TemporaryDirectory(prefix="installed-comparison-") as directory:
            scratch = Path(directory)
            installed = scratch / "prefix/bin/save"
            installed.parent.mkdir(parents=True)
            shutil.copy2(binary, installed)
            installed.chmod(0o755)
            assert digest(installed.read_bytes()) == options.expected_binary_sha256
            cwd = scratch / "unrelated"
            cwd.mkdir()
            state = scratch / "store"
            certs = certificates.certificates(scratch)
            fixture = Fixture(certs["server.pem"], certs["server.key"])
            config, context = scratch / "operator.json", scratch / "context.json"
            config.write_text(json.dumps({"spec_version": "0.1", "record": "never", "connections": {"fixture": {
                "origin": fixture.origin, "expected_org_id": 7, "api_profile": "grafana-legacy-v1",
                "credential_ref": "env:" + PROVIDER, "tls_ca_file": str(certs["ca.pem"])}}}))
            context.write_text(json.dumps({"schema_version": 1, "source": {"kind": "fixture-export", "id": "comparison", "revision": "1"},
                "records": [{"service": "fixture-web", "team": "platform", "environment": "dev", "deployment": "blue",
                    "state": "active", "last_reviewed": datetime.now(timezone.utc).isoformat(),
                    "required_bindings": ["target", "environment", "organization"],
                    "bindings": {"target": "fixture", "environment": "dev", "organization": "7"}}]}))

            def run(name, arguments, expected=0, collect=False):
                argv = [str(installed), "--json", "--state-dir", str(state)]
                env = {"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8", "TZ": "UTC"}
                if collect:
                    argv += ["--config", str(config), "--context-source", str(context), "--context-max-age-days", "30"]
                    env[PROVIDER] = json.dumps({"origin": fixture.origin, "expected_org_id": 7, "operations": ["grafana.query"], "auth": {"kind": "bearer", "token": TOKEN}})
                argv += arguments
                start = time.monotonic()
                output = subprocess.run(argv, cwd=cwd, env=env, capture_output=True, timeout=12, check=False)
                retain(f"commands/{name}.stdout.json", output.stdout)
                retain(f"commands/{name}.stderr", output.stderr)
                commands.append({"case": name, "argv": argv, "cwd": str(cwd), "exit_status": output.returncode,
                                 "duration_ms": round((time.monotonic() - start) * 1000), "provider_available": collect})
                assert output.returncode == expected and not output.stderr, (name, output.returncode, output.stdout[:1800], output.stderr[:300])
                assert len(output.stdout) <= 2 * 1024 * 1024
                assert output.stdout.endswith(b"\n") and output.stdout.count(b"\n") == 1, "Expected one canonical JSON line"
                result = helpers.strict_json(output.stdout)
                assert_finite(result)
                validators["result"].validate(result)
                assert result["assessment"] == "not_assessed"
                if result["operation"] == "evidence.collect":
                    validators["evidence-collection-data"].validate(result["data"])
                if result["operation"] == "evidence.compare" and "algorithm" in result["data"]:
                    validators["evidence-comparison-data"].validate(result["data"])
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
                    path = scratch / "input.json"
                    path.write_text(json.dumps(supplied))
                    retain(f"collection-input-{variant}.json", supplied)
                    value = run(f"collect-{variant}", ["--record", "always", "evidence", "collect", "--target", "fixture", "--input", str(path)], collect=True)
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
            hashes = {name: digest(raw) for name, raw in before.items()}
            retain("store-before-sha256.json", hashes)
            for name, raw in before.items():
                retain("store/" + name, raw)
                if name.endswith("manifest.json"):
                    value = helpers.strict_json(raw)
                    assert_finite(value)
                    validators["evidence-manifest"].validate(value)
                    validators["evidence-collection-payload"].validate(value["payload"])
                if "/artifacts/" in name:
                    value = helpers.strict_json(raw)
                    assert_finite(value)
                    validators["result"].validate(value)
                    validators["query-run-data"].validate(value["data"])

            def compare(name, left, right, policy="exact-window-v1", extra=(), expected=0):
                return run(name, ["evidence", "compare", left["id"], right["id"], "--normalization", policy, *extra], expected)

            changed = compare("changed", bundles[0], bundles[1], extra=("--before-manifest-sha256", bundles[0]["digest"], "--after-manifest-sha256", bundles[1]["digest"]))
            checks = {c["id"]: c for c in changed["data"]["checks"]}
            for field, expected in {"series_added": 1, "series_removed": 1, "samples_changed": 1, "samples_unchanged": 1, "samples_added": 2, "samples_removed": 2}.items():
                assert checks["metrics"]["summary"][field] == expected
            for field, expected in {"entries_added": 1, "entries_removed": 2, "entries_unchanged": 1}.items():
                assert checks["logs"]["summary"][field] == expected
            duplicate = next(d for d in checks["logs"]["details"] if d["line"] == "duplicate")
            assert duplicate["count"] == 1 and duplicate["timestamp_ns"] == "1790899200000000123"
            equal = compare("self-equal", bundles[0], bundles[0])
            assert all(not c["details"] for c in equal["data"]["checks"])
            assert equal["data"]["state"] == "complete"
            incompatible = compare("exact-shifted", bundles[0], bundles[2], expected=1)
            assert incompatible["data"]["state"] == "incompatible"
            offset = compare("offset-shifted", bundles[0], bundles[2], "window-offset-v1")
            assert offset["data"]["state"] == "complete"
            assert offset["data"]["checks"][0]["summary"] == checks["metrics"]["summary"]
            assert offset["data"]["before"]["window"] != offset["data"]["after"]["window"]
            shifted_duplicate = next(d for d in offset["data"]["checks"][1]["details"] if d["line"] == "duplicate")
            assert shifted_duplicate["timestamp_ns"] == "123"
            limited = compare("max-items", bundles[0], bundles[1], extra=("--max-items", "1"), expected=1)
            assert limited["data"]["state"] == "partial"
            assert sum(len(c["details"]) for c in limited["data"]["checks"]) == 1
            assert sum(c["omitted_details"] for c in limited["data"]["checks"]) > 0
            wrong = compare("wrong-pin", bundles[0], bundles[1], extra=("--before-manifest-sha256", "sha256:" + "0" * 64), expected=1)
            assert "manifest_digest_mismatch" in {e["code"] for e in wrong["errors"]}
            invalid = compare("path-reference", {"id": "../escape"}, bundles[1], expected=2)
            assert "invalid_evidence_reference" in {e["code"] for e in invalid["errors"]}
            after = helpers.private_files(state)
            assert after == before, "Comparison changed stored bytes"
            retain("store-after-sha256.json", {name: digest(raw) for name, raw in after.items()})
            assert len(fixture.events) == 18
            assert digest(installed.read_bytes()) == options.expected_binary_sha256
        passed = True
    finally:
        if fixture is not None:
            fixture.close()
        helpers.source_guard(source, options.source_manifest, options.expected_source_manifest_sha256)
        assert digest(binary.read_bytes()) == options.expected_binary_sha256
        joined = not (set(threading.enumerate()) - baseline_threads)
        guards.append({"stage": "after", "source_manifest_sha256": options.expected_source_manifest_sha256,
                       "source_files": len(manifest), "binary_sha256": digest(binary.read_bytes())})
        retain("commands.json", commands)
        retain("http-events.json", events)
        retain("source-binary-guards.json", guards)
        retain("verification-summary.json", {"passed": passed, "commands": len(commands), "http_requests": len(events),
               "fixture_threads_joined": joined, "offline_comparison": "fixture closed, workers joined, configuration/context removed, provider absent",
               "source_manifest_sha256": options.expected_source_manifest_sha256, "binary_sha256": options.expected_binary_sha256})
        assert joined, "Fixture worker remained alive"
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
