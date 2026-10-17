"""Installed evidence mission; execute only inside tools/test-sandbox.sh.

The supplied source manifest and ordinary binary digest identify the tested bytes.
This checker does not establish how the binary was built or authorize live access.
"""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import http.server
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import signal
import ssl
import stat
import subprocess
import sys
import tempfile
import threading
import time

from jsonschema import Draft202012Validator, FormatChecker
from referencing import Registry, Resource


ROOT = Path(__file__).resolve().parents[1]
MAX_RESULT_BYTES = 2 * 1024 * 1024
TOKEN = "fixture-only-evidence-secret-e99e314b"
PROVIDER = "SAVE_GRAFANA_EVIDENCE_FIXTURE"
PACK = "service-queries"


def sha256(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def strict_json(raw: bytes) -> object:
    def object_pairs(pairs):
        result = {}
        for key, value in pairs:
            assert key not in result, f"Duplicate JSON member: {key}"
            result[key] = value
        return result

    def invalid_constant(value):
        raise AssertionError(f"Non-finite JSON constant: {value}")

    return json.loads(raw, object_pairs_hook=object_pairs, parse_constant=invalid_constant)


def milliseconds(timestamp: str) -> int:
    duration = datetime.fromisoformat(timestamp) - datetime(1970, 1, 1, tzinfo=timezone.utc)
    return (duration.days * 86400 + duration.seconds) * 1000 + duration.microseconds // 1000


def assert_no_canary(raw: bytes) -> None:
    assert TOKEN.encode() not in raw, "Known credential survived in emitted or retained bytes"


class Fixture(http.server.ThreadingHTTPServer):
    """Sequential query routes with explicit worker shutdown, without daemon threads."""

    daemon_threads = False
    block_on_close = True

    def __init__(self, certificate: Path, key: Path):
        self.events = []
        self.condition = threading.Condition()
        self.release = threading.Event()
        self.fail_kind = None
        self.block_event = None
        self.large_logs = False
        super().__init__(("127.0.0.1", 0), Handler)
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.minimum_version = ssl.TLSVersion.TLSv1_2
        context.load_cert_chain(certificate, key)
        self.socket = context.wrap_socket(self.socket, server_side=True)
        self.origin = f"https://127.0.0.1:{self.server_port}/grafana"
        self.thread = threading.Thread(target=self.serve_forever, kwargs={"poll_interval": 0.02})
        self.thread.start()

    def wait_count(self, count: int) -> None:
        with self.condition:
            assert self.condition.wait_for(lambda: len(self.events) >= count, timeout=5), (
                f"Fixture did not receive request {count}", self.events
            )

    def close(self) -> None:
        self.release.set()
        self.shutdown()
        self.server_close()
        self.thread.join(timeout=2)
        assert not self.thread.is_alive(), "HTTPS listener did not join"

    def handle_error(self, request, client_address) -> None:
        # Deliberate cancellation may break an HTTPS response; never print headers.
        pass


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_arguments) -> None:
        pass

    def do_GET(self) -> None:
        self.exchange()

    def do_POST(self) -> None:
        self.exchange()

    def exchange(self) -> None:
        fixture: Fixture = self.server
        self.connection.settimeout(5)
        length = int(self.headers.get("Content-Length", "0"))
        assert 0 <= length <= 128 * 1024
        body = self.rfile.read(length)
        with fixture.condition:
            fixture.events.append({
                "method": self.command, "path": self.path, "body": body,
                "at_monotonic_ns": time.monotonic_ns(),
                "auth_matches": self.headers.get_all("Authorization", []) == ["Bearer " + TOKEN],
                "organization_matches": self.headers.get_all("X-Grafana-Org-Id", []) == ["7"],
            })
            index = len(fixture.events)
            fixture.condition.notify_all()
        if fixture.block_event == index:
            fixture.release.wait(timeout=8)
        status = 200
        if self.command == "GET" and self.path == "/grafana/api/org":
            payload = {"id": 7}
        elif self.command == "GET" and self.path in {
            "/grafana/api/datasources/uid/metrics-1?ds_type=prometheus",
            "/grafana/api/datasources/uid/logs-1?ds_type=loki",
        }:
            uid, kind = ("metrics-1", "prometheus") if "metrics-1" in self.path else ("logs-1", "loki")
            payload = {"uid": uid, "type": kind, "orgId": 7}
        elif self.command == "POST" and self.path == "/grafana/api/ds/query":
            request = strict_json(body)
            kind = request["queries"][0]["datasource"]["type"]
            if fixture.fail_kind == kind:
                payload = {"results": {"A": {"error": "synthetic failure " + TOKEN, "frames": []}}}
            else:
                fields = ([{"name": "Time", "type": "time"}, {"name": "Value", "type": "number", "labels": {"job": "fixture"}}]
                          if kind == "prometheus" else [{"name": "Time", "type": "time"}, {"name": "Line", "type": "string"}])
                value = 1 if kind == "prometheus" else ("x" * 131072 if fixture.large_logs else "fixture log")
                payload = {"results": {"A": {"frames": [{
                    "schema": {"refId": "A", "fields": fields, "meta": {"notice": TOKEN}},
                    "data": {"values": [[1790899200000], [value]]},
                }]}}}
        else:
            status, payload = 404, {"error": "unexpected fixture route"}
        raw = json.dumps(payload, allow_nan=False).encode()
        try:
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(raw)))
            self.send_header("Connection", "close")
            self.end_headers()
            self.wfile.write(raw)
            self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError, ssl.SSLError, TimeoutError):
            pass
        finally:
            self.close_connection = True


def source_guard(root: Path, manifest_file: Path, expected: str) -> dict[str, str]:
    raw = manifest_file.read_bytes()
    assert sha256(raw) == expected, "Source manifest changed"
    manifest = strict_json(raw)
    assert isinstance(manifest, dict) and manifest, "Expected path-to-SHA256 source manifest"
    for name, digest in manifest.items():
        path = Path(name)
        assert not path.is_absolute() and ".." not in path.parts
        assert isinstance(digest, str) and re.fullmatch(r"[a-f0-9]{64}", digest)
        actual = root / path
        assert actual.is_file() and not actual.is_symlink(), name
        assert sha256(actual.read_bytes()) == digest, f"Source changed: {name}"
    assert manifest["tools/verify-evidence.py"] == sha256(Path(__file__).read_bytes()), "Checker differs from source identity"
    return manifest


def private_files(root: Path) -> dict[str, bytes]:
    result = {}
    if not root.exists():
        return result
    for directory, subdirectories, files in os.walk(root, followlinks=False):
        folder = Path(directory)
        metadata = folder.lstat()
        assert stat.S_ISDIR(metadata.st_mode) and stat.S_IMODE(metadata.st_mode) == 0o700
        assert metadata.st_uid == os.getuid()
        for name in subdirectories:
            assert not (folder / name).is_symlink()
        for name in files:
            path = folder / name
            metadata = path.lstat()
            assert stat.S_ISREG(metadata.st_mode) and stat.S_IMODE(metadata.st_mode) == 0o600
            assert metadata.st_uid == os.getuid() and metadata.st_nlink == 1
            raw = path.read_bytes()
            assert_no_canary(raw)
            result[str(path.relative_to(root))] = raw
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--expected-binary-sha256", required=True)
    parser.add_argument("--source-root", type=Path, default=ROOT)
    parser.add_argument("--source-manifest", type=Path, required=True)
    parser.add_argument("--expected-source-manifest-sha256", required=True)
    parser.add_argument("--base-head", help="Provenance only; not the changed source identity")
    parser.add_argument("--evidence-dir", type=Path, required=True)
    parser.add_argument("--enospc-root", type=Path, help="Sandbox-owned 64KiB tmpfs; never use a host directory")
    options = parser.parse_args()
    source = options.source_root.resolve(strict=True)
    binary = options.binary.resolve(strict=True)
    manifest_file = options.source_manifest.resolve(strict=True)
    manifest = source_guard(source, manifest_file, options.expected_source_manifest_sha256)
    assert sha256(binary.read_bytes()) == options.expected_binary_sha256, "Binary does not match supplied identity"
    evidence = options.evidence_dir.resolve()
    evidence.mkdir(mode=0o700, parents=True, exist_ok=False)
    schemas = {path.stem.removesuffix(".schema"): strict_json(path.read_bytes())
               for path in (source / "docs/sre-workbench/schemas").glob("*.schema.json")}
    registry = Registry().with_resources((schema["$id"], Resource.from_contents(schema)) for schema in schemas.values())
    validators = {name: Draft202012Validator(schemas[name], registry=registry, format_checker=FormatChecker())
                  for name in ("result", "request", "query-run-data", "evidence-collect-input",
                               "evidence-collection-data", "evidence-collection-payload", "evidence-manifest")}
    schema_ids = {schema["$id"] for schema in schemas.values()}
    module_spec = importlib.util.spec_from_file_location("evidence_certificate_fixture", source / "tools/verify-grafana.py")
    assert module_spec is not None and module_spec.loader is not None
    fixture_helpers = importlib.util.module_from_spec(module_spec)
    module_spec.loader.exec_module(fixture_helpers)
    thread_baseline = set(threading.enumerate())
    receipts = []
    ledger = []
    commands = []
    artifacts = []
    completed_cases = []
    query_receipts = 0
    collection_receipts = 0
    checked_manifests = 0
    summary = {"result": "failed", "binary_sha256": options.expected_binary_sha256,
               "source_manifest_sha256": options.expected_source_manifest_sha256,
               "base_head": options.base_head, "source_identity": "manifest-frozen working-tree snapshot",
               "source_files": len(manifest), "python": sys.version, "platform": sys.platform,
               "remaining_scope": ["live Grafana compatibility", "native non-Linux acceptance", "comparison semantics",
                                   "build provenance must be established by the caller"]}

    def retain(name: str, raw: bytes) -> None:
        path = evidence / name
        path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        path.write_bytes(raw)

    def receipt(raw: bytes, *, child: bool = False) -> dict:
        nonlocal query_receipts, collection_receipts
        assert 0 < len(raw) <= MAX_RESULT_BYTES
        assert raw.endswith(b"\n") and b"\n" not in raw[:-1], "Receipt must be one complete JSON line"
        assert_no_canary(raw)
        value = strict_json(raw)
        fixture_helpers.assert_finite(value)
        validators["result"].validate(value)
        assert value["assessment"] == "not_assessed"
        assert milliseconds(value["finished_at"]) >= milliseconds(value["started_at"])
        if child:
            assert value["operation"] == "query.run" and value["record_mode"] == "never"
            validators["query-run-data"].validate(value["data"])
            query_receipts += 1
        elif value["operation"] == "evidence.collect":
            assert value["record_mode"] == "always"
            if "format_version" in value["data"]:
                validators["evidence-collection-data"].validate(value["data"])
                collection_receipts += 1
        else:
            assert value["record_mode"] == "never"
        receipts.append(raw)
        return value

    try:
        with tempfile.TemporaryDirectory(prefix="workbench-installed-evidence-") as directory:
            scratch = Path(directory)
            installed = scratch / "prefix/bin/save"
            installed.parent.mkdir(parents=True)
            shutil.copy2(binary, installed)
            installed.chmod(0o755)
            assert sha256(installed.read_bytes()) == options.expected_binary_sha256
            working = scratch / "unrelated-working-directory"
            working.mkdir()
            certificates = fixture_helpers.certificates(scratch)

            def command(arguments, *, state=None, configuration=None, context=None, provider=None):
                argv = [str(installed), "--json"]
                if state is not None:
                    argv += ["--state-dir", str(state)]
                if configuration is not None:
                    argv += ["--config", str(configuration)]
                if context is not None:
                    argv += ["--context-source", str(context), "--context-max-age-days", "30"]
                environment = {"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8", "TZ": "UTC"}
                if provider is not None:
                    environment[PROVIDER] = json.dumps(provider)
                return argv + list(arguments), environment

            def finish(case, argv, output, *, expected, elapsed):
                number = len(commands)
                retain(f"commands/{number:03d}-{case}.stdout.json", output.stdout)
                retain(f"commands/{number:03d}-{case}.stderr", output.stderr)
                commands.append({"case": case, "argv": argv, "cwd": str(working),
                                 "exit_status": output.returncode, "duration_ms": round(elapsed * 1000),
                                 "stdout_sha256": sha256(output.stdout), "stderr_sha256": sha256(output.stderr)})
                assert output.returncode == expected, (case, output.returncode, output.stdout[:2000], output.stderr[:500])
                assert not output.stderr, (case, output.stderr[:500])
                return receipt(output.stdout)

            def run(case, arguments, *, expected=0, **settings):
                argv, environment = command(arguments, **settings)
                start = time.monotonic()
                output = subprocess.run(argv, cwd=working, env=environment, capture_output=True, timeout=8, check=False)
                return finish(case, argv, output, expected=expected, elapsed=time.monotonic() - start)

            def create_case(name, *, enospc=False):
                case = scratch / name
                case.mkdir(mode=0o700)
                state = (options.enospc_root / "evidence-store") if enospc else case / "store"
                fixture = Fixture(certificates["server.pem"], certificates["server.key"])
                configuration = case / "operator.json"
                configuration.write_text(json.dumps({"spec_version": "0.1", "record": "never", "connections": {"fixture": {
                    "origin": fixture.origin, "expected_org_id": 7, "api_profile": "grafana-legacy-v1",
                    "credential_ref": "env:" + PROVIDER, "tls_ca_file": str(certificates["ca.pem"]),
                }}}))
                context = case / "context.json"
                context.write_text(json.dumps({"schema_version": 1,
                    "source": {"kind": "fixture-export", "id": "installed-evidence-fixture", "revision": "fixture-1"},
                    "records": [{"service": "fixture-web", "team": "platform", "environment": "dev", "deployment": "blue",
                                 "state": "active", "last_reviewed": datetime.now(timezone.utc).isoformat(),
                                 "required_bindings": ["target", "environment", "organization"],
                                 "bindings": {"target": "fixture", "environment": "dev", "organization": "7"}}]}))
                settings = {"state": state, "configuration": configuration, "context": context,
                            "provider": {"origin": fixture.origin, "expected_org_id": 7, "operations": ["grafana.query"],
                                         "auth": {"kind": "bearer", "token": TOKEN}}}
                return case, fixture, settings

            def collect_arguments(case, inputs, *, structured=False):
                if structured:
                    request = {"spec_version": "0.1", "request_id": "installed-evidence-fixture",
                               "operation": "evidence.collect", "operation_version": 1,
                               "target": {"kind": "connection", "id": "fixture"}, "inputs": inputs,
                               "limits": {"timeout_ms": 60000, "max_output_bytes": MAX_RESULT_BYTES}, "record": "always"}
                    validators["request"].validate(request)
                    path = case / "request.json"
                    path.write_text(json.dumps(request))
                    return ("call", "--request", str(path))
                path = case / "input.json"
                path.write_text(json.dumps(inputs))
                return ("--record", "always", "evidence", "collect", "--target", "fixture", "--input", str(path))

            def save_case(name, fixture, state):
                fixture.close()
                for index, event in enumerate(fixture.events):
                    body = event["body"]
                    assert_no_canary(body)
                    body_path = f"http-bodies/{name}-{index:03d}.body"
                    retain(body_path, body)
                    ledger.append(dict(event, case=name, body=body.decode(), body_path=body_path, body_sha256=sha256(body)))
                for name_in_store, raw in private_files(state).items():
                    retained_path = f"stores/{name}/{name_in_store}"
                    retain(retained_path, raw)
                    artifacts.append({"case": name, "path": name_in_store, "retained_path": retained_path,
                                      "bytes": len(raw), "sha256": sha256(raw)})

            def verify_manifest(state, reference, *, data=None, interrupted=False):
                nonlocal checked_manifests
                path = state / "runs" / reference["id"]
                raw = (path / "manifest.json").read_bytes()
                assert "sha256:" + sha256(raw) == reference["digest"]
                value = strict_json(raw)
                validators["evidence-manifest"].validate(value)
                validators["evidence-collection-payload"].validate(value["payload"])
                assert value["state"] == ("interrupted" if interrupted else "sealed")
                assert value["id"] == reference["id"] == value["payload"]["run_id"]
                if data is not None:
                    assert value["payload"] == {key: field for key, field in data.items()
                                                if key not in {"bundle", "delivery_cancellation_signal"}}
                child_results = {}
                for step, artifact in value["artifacts"].items():
                    digest = artifact["digest"]
                    assert artifact["path"] == "artifacts/" + digest.removeprefix("sha256:") + ".json"
                    assert artifact["classification"] == "restricted" and artifact["media_type"] == "application/json"
                    raw_child = (path / artifact["path"]).read_bytes()
                    assert len(raw_child) == artifact["bytes"]
                    assert "sha256:" + sha256(raw_child) == digest
                    child_results[step] = receipt(raw_child, child=True)
                checked_manifests += 1
                private_files(state)
                return value, child_results

            def assert_requests(fixture, count=6):
                expected = [("GET", "/grafana/api/org"),
                            ("GET", "/grafana/api/datasources/uid/metrics-1?ds_type=prometheus"),
                            ("POST", "/grafana/api/ds/query"),
                            ("GET", "/grafana/api/org"),
                            ("GET", "/grafana/api/datasources/uid/logs-1?ds_type=loki"),
                            ("POST", "/grafana/api/ds/query")]
                assert [(event["method"], event["path"]) for event in fixture.events] == expected[:count]
                assert all(event["auth_matches"] and event["organization_matches"] for event in fixture.events)

            def inputs():
                return {"service": "fixture-web", "environment": "dev", "pack": {"id": PACK, "version": 1},
                        "window": {"kind": "relative", "lookback_seconds": 300},
                        "metrics": {"datasource": "metrics-1", "parameters": {"job": "fixture"}},
                        "logs": {"datasource": "logs-1", "parameters": {"job": "fixture"}}, "on_failure": "continue"}

            listed = run("packs-list", ("evidence", "packs", "list"), configuration=scratch / "absent.json")
            assert len(listed["data"]["packs"]) == 1
            pack = listed["data"]["packs"][0]
            described = run("packs-describe", ("evidence", "packs", "describe", PACK, "--version", "1"))
            definition_bytes = (source / "crates/workbench-core/resources/evidence/service-queries.v1.json").read_bytes()
            assert pack == described["data"]["pack"] == dict(strict_json(definition_bytes), sha256=sha256(definition_bytes))
            assert pack["input_schema"]["$ref"] in schema_ids and pack["output_schema"]["$ref"] in schema_ids
            for step in pack["steps"]:
                definition = source / "crates/workbench-core/resources/queries" / (step["query"]["id"] + ".v1.json")
                assert step["query"]["sha256"] == sha256(definition.read_bytes())
            completed_cases.append("installed-pack-discovery-and-schema-identities")

            case, fixture, settings = create_case("complete")
            state = settings["state"]
            try:
                bad = inputs()
                bad["expected_pack_sha256"] = "0" * 64
                rejected = run("wrong-pack-pin", collect_arguments(case, bad), expected=2, **settings)
                assert rejected["effect_outcome"] == "not_attempted" and not fixture.events and not state.exists()
                supplied = inputs()
                supplied["expected_pack_sha256"] = pack["sha256"]
                validators["evidence-collect-input"].validate(supplied)
                collected = run("complete-collect", collect_arguments(case, supplied), **settings)
                data = collected["data"]
                assert data["capture"] == {"state": "complete", "expected_steps": 2, "retained_receipts": 2}
                assert collected["record_mode"] == "always" and data["bundle"] is not None
                anchor = milliseconds(collected["started_at"])
                assert milliseconds(data["window"]["to"]) == anchor
                assert milliseconds(data["window"]["from"]) == anchor - 300000
                _, children = verify_manifest(state, data["bundle"], data=data)
                assert_requests(fixture)
                for step, index, kind, expression in (("metrics", 2, "prometheus", 'up{job="fixture"}'),
                                                      ("logs", 5, "loki", '{job="fixture"}')):
                    child = children[step]
                    assert child["execution"]["status"] == "succeeded"
                    assert child["data"]["window"] == data["window"]
                    assert child["coverage"]["state"] == "not_established"
                    assert milliseconds(child["started_at"]) >= anchor
                    assert milliseconds(child["finished_at"]) <= milliseconds(collected["finished_at"])
                    request = strict_json(fixture.events[index]["body"])
                    assert request["from"] == str(anchor - 300000) and request["to"] == str(anchor)
                    assert request["queries"][0]["expr"] == expression
                    assert request["queries"][0]["datasource"]["type"] == kind
                # Manage with a fresh process, no provider, no source or Grafana configuration.
                reference = data["bundle"]
                show_args = ("evidence", "show", reference["id"], "--expected-manifest-sha256", reference["digest"])
                before = private_files(state)
                shown = run("show-after-restart", show_args, state=state)
                assert shown["data"]["manifest"]["payload"] == {
                    key: value for key, value in data.items()
                    if key not in {"bundle", "delivery_cancellation_signal"}
                }
                assert private_files(state) == before
                listed = run("list-after-restart", ("evidence", "list"), state=state)
                assert [(row["id"], row["state"]) for row in listed["data"]["runs"]] == [(reference["id"], "sealed")]
                rejected = run("wrong-manifest-pin", ("evidence", "delete", reference["id"], "--expected-manifest-sha256", "sha256:" + "0" * 64), expected=1, state=state)
                assert "manifest_digest_mismatch" in {error["code"] for error in rejected["errors"]}
                assert private_files(state) == before
                protected = run("protect", ("evidence", "protect", reference["id"]), state=state)
                assert protected["data"]["protected"] is True
                run("delete-protected", ("evidence", "delete", reference["id"]), expected=1, state=state)
                assert (state / "runs" / reference["id"] / "manifest.json").read_bytes() == before["runs/" + reference["id"] + "/manifest.json"]
                # Preserve the accepted store before explicit deletion.
                for name, raw in private_files(state).items():
                    retained_path = "complete-before-delete/" + name
                    retain(retained_path, raw)
                    artifacts.append({"case": "complete-before-delete", "path": name,
                                      "retained_path": retained_path, "bytes": len(raw), "sha256": sha256(raw)})
                run("unprotect", ("evidence", "unprotect", reference["id"]), state=state)
                deleted = run("delete", ("evidence", "delete", reference["id"]), state=state)
                assert deleted["data"]["deleted"] is True and not (state / "runs" / reference["id"]).exists()
                assert run("list-after-delete", ("evidence", "list"), state=state)["data"]["runs"] == []
                assert_requests(fixture)
                completed_cases.append("real-common-window-collection-durable-restart-reference-management")
            finally:
                save_case("complete", fixture, state)

            for name in ("structured-redaction", "partial", "cancellation", "interrupted"):
                case, fixture, settings = create_case(name)
                state = settings["state"]
                child = None
                try:
                    supplied = inputs()
                    if name == "structured-redaction":
                        context = strict_json(settings["context"].read_bytes())
                        context["source"]["revision"] = TOKEN
                        context["records"][0]["team"] = TOKEN
                        settings["context"].write_text(json.dumps(context))
                        supplied["team"] = TOKEN
                        collected = run(name, collect_arguments(case, supplied, structured=True), **settings)
                        assert collected["data"]["selection"]["team"] == {"redacted": True}
                        assert collected["data"]["provenance"]["context"]["source"]["revision"] == {"redacted": True}
                        assert collected["data"]["capture"]["state"] == "complete"
                        verify_manifest(state, collected["data"]["bundle"], data=collected["data"])
                        assert_requests(fixture)
                    elif name == "partial":
                        fixture.fail_kind = "loki"
                        collected = run(name, collect_arguments(case, supplied), expected=1, **settings)
                        assert collected["data"]["capture"] == {"state": "partial", "expected_steps": 2, "retained_receipts": 2}
                        _, children = verify_manifest(state, collected["data"]["bundle"], data=collected["data"])
                        assert children["metrics"]["execution"]["status"] == "succeeded"
                        assert children["logs"]["execution"]["status"] == "failed"
                        assert children["logs"]["data"]["response"] is None
                        assert_requests(fixture)
                    else:
                        fixture.block_event = 6 if name == "cancellation" else 4
                        argv, environment = command(collect_arguments(case, supplied), **settings)
                        started = time.monotonic()
                        child = subprocess.Popen(argv, cwd=working, env=environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                        fixture.wait_count(fixture.block_event)
                        stopped = time.monotonic()
                        child.send_signal(signal.SIGTERM if name == "cancellation" else signal.SIGKILL)
                        stdout, stderr = child.communicate(timeout=4)
                        assert time.monotonic() - stopped < 4, "Terminal process exceeded cleanup allowance"
                        output = subprocess.CompletedProcess(argv, child.returncode, stdout, stderr)
                        if name == "cancellation":
                            collected = finish(name, argv, output, expected=143, elapsed=time.monotonic() - started)
                            assert collected["execution"]["status"] == "cancelled"
                            assert collected["data"]["cancellation_signal"] == signal.SIGTERM
                            assert collected["data"]["capture"]["state"] == "partial"
                            _, children = verify_manifest(state, collected["data"]["bundle"], data=collected["data"])
                            assert children["metrics"]["execution"]["status"] == "succeeded"
                            assert children["logs"]["execution"]["status"] == "cancelled"
                            assert_requests(fixture)
                        else:
                            assert output.returncode == -signal.SIGKILL and not stdout and not stderr
                            commands.append({"case": name, "argv": argv, "cwd": str(working), "exit_status": output.returncode,
                                             "duration_ms": round((time.monotonic() - started) * 1000),
                                             "stdout_sha256": sha256(stdout), "stderr_sha256": sha256(stderr)})
                            listed = run("list-incomplete", ("evidence", "list"), state=state)
                            assert len(listed["data"]["runs"]) == 1 and listed["data"]["runs"][0]["state"] == "incomplete"
                            id = listed["data"]["runs"][0]["id"]
                            before = private_files(state)
                            run("show-incomplete", ("evidence", "show", id), expected=1, state=state)
                            assert private_files(state) == before, "Read implicitly recovered incomplete state"
                            recovered = run("recover", ("evidence", "recover"), state=state)
                            assert len(recovered["data"]["recovered"]) == 1
                            reference = recovered["data"]["recovered"][0]
                            value, children = verify_manifest(state, reference, interrupted=True)
                            assert set(children) == {"metrics"} and children["metrics"]["execution"]["status"] == "succeeded"
                            retained = value["artifacts"]["metrics"]["path"]
                            assert (state / "runs" / id / retained).read_bytes() == before["runs/" + id + "/" + retained]
                            shown = run("show-recovered", ("evidence", "show", id), state=state)
                            assert shown["data"]["effective_capture"] == {"state": "partial", "reason": "interrupted"}
                            before = private_files(state)
                            run("recover-idempotent", ("evidence", "recover"), state=state)
                            run("prune-interrupted", ("evidence", "prune"), state=state)
                            assert private_files(state) == before
                            assert_requests(fixture, count=4)
                    completed_cases.append(name)
                finally:
                    if child is not None and child.poll() is None:
                        child.kill()
                        child.communicate(timeout=3)
                    save_case(name, fixture, state)

            if options.enospc_root is not None:
                small_root = options.enospc_root.resolve(strict=True)
                assert str(small_root) == "/tmp/evidence-small-disk", "Only the dedicated sandbox mount is accepted"
                mounts = Path("/proc/self/mountinfo").read_text().splitlines()
                assert any(line.split()[4] == str(small_root) and " - tmpfs " in line for line in mounts)
                stats = os.statvfs(small_root)
                assert 0 < stats.f_blocks * stats.f_frsize <= 64 * 1024, "Disk-full mission requires bounded private tmpfs"
                assert not any(small_root.iterdir()), "Disk-full fixture must start empty"
                case, fixture, settings = create_case("disk-full", enospc=True)
                state = settings["state"]
                fixture.large_logs = True
                try:
                    collected = run("disk-full", collect_arguments(case, inputs()), expected=1, **settings)
                    assert_requests(fixture)
                    assert collected["data"]["bundle"] is None and collected["data"]["capture"]["state"] == "partial"
                    steps = collected["data"]["steps"]
                    assert steps[0]["artifact"] is not None and steps[1]["artifact"] is None
                    assert "store_no_space" in {error["code"] for error in collected["errors"]}, "Expected the actual filesystem ENOSPC failure"
                    run_path = state / "runs" / collected["run_id"]
                    assert not (run_path / "manifest.json").exists()
                    raw = (run_path / steps[0]["artifact"]["path"]).read_bytes()
                    assert "sha256:" + sha256(raw) == steps[0]["artifact"]["digest"]
                    assert receipt(raw, child=True)["execution"]["status"] == "succeeded"
                    completed_cases.append("actual-private-tmpfs-disk-full-preserves-confirmed-receipt")
                finally:
                    save_case("disk-full", fixture, state)
                    if state.exists():
                        shutil.rmtree(state)

            assert sha256(installed.read_bytes()) == options.expected_binary_sha256
        assert not set(threading.enumerate()) - thread_baseline, "Fixture workers did not join"
        source_guard(source, manifest_file, options.expected_source_manifest_sha256)
        assert sha256(binary.read_bytes()) == options.expected_binary_sha256
        summary.update({"result": "passed", "canonical_receipts": len(receipts),
                        "strict_query_receipts": query_receipts, "strict_collection_receipts": collection_receipts,
                        "strict_manifests": checked_manifests, "installed_copy_outside_checkout": True,
                        "source_and_binary_before_after_guards": True, "fixture_threads_joined": True,
                        "disk_full_executed": options.enospc_root is not None})
    finally:
        summary["completed_cases"] = completed_cases
        retain("installed-evidence-receipts.jsonl", b"".join(receipts))
        retain("installed-evidence-ledger.json", (json.dumps(ledger, indent=2) + "\n").encode())
        retain("installed-evidence-commands.json", (json.dumps(commands, indent=2) + "\n").encode())
        retain("installed-evidence-artifacts.json", (json.dumps(artifacts, indent=2) + "\n").encode())
        retain("installed-evidence-summary.json", (json.dumps(summary, indent=2) + "\n").encode())
    print(json.dumps(summary, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
