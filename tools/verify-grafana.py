"""Public Grafana acceptance against private HTTPS fixtures; run in the test sandbox."""

from __future__ import annotations

import argparse
import base64
import copy
from datetime import datetime
import gzip
import http.client
import http.server
import json
import math
import os
from pathlib import Path
import signal
import socket
import ssl
import subprocess
import tempfile
import threading
import time
from urllib.parse import quote, quote_plus


ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "tests/fixtures/grafana"
START = "2026-10-02T00:00:00Z"
END = "2026-10-02T00:01:00Z"
TOKEN = "fixture-only-grafana-token-6af894c2"
PROVIDER = "SAVE_GRAFANA_FIXTURE"


def certificates(directory: Path) -> dict[str, Path]:
    """Create only disposable synthetic test keys; never use machine credentials."""
    def openssl(*args: str) -> None:
        result = subprocess.run(["/usr/bin/openssl", *args], cwd=directory,
                                env={"PATH": "/usr/bin:/bin", "OPENSSL_CONF": "/dev/null"},
                                stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
                                timeout=20, check=False)
        if result.returncode:
            raise RuntimeError("Synthetic HTTPS certificate generation failed")

    openssl("req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2", "-sha256",
            "-subj", "/CN=Workbench fixture CA", "-keyout", "ca.key", "-out", "ca.pem",
            "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign,cRLSign",
            "-addext", "subjectKeyIdentifier=hash", "-config", "/dev/null")
    for name, sans in (("server", "DNS:localhost,IP:127.0.0.1"),
                       ("wrong", "DNS:wrong.fixture.invalid")):
        openssl("req", "-new", "-newkey", "rsa:2048", "-nodes", "-sha256", "-subj",
                "/CN=" + name, "-keyout", name + ".key", "-out", name + ".csr",
                "-config", "/dev/null")
        (directory / (name + ".ext")).write_text(
            "basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\n"
            "extendedKeyUsage=serverAuth\nsubjectKeyIdentifier=hash\nauthorityKeyIdentifier=keyid,issuer\n"
            "subjectAltName=" + sans + "\n", encoding="ascii")
        openssl("x509", "-req", "-in", name + ".csr", "-CA", "ca.pem", "-CAkey", "ca.key",
                "-CAcreateserial", "-days", "2", "-sha256", "-extfile", name + ".ext",
                "-out", name + ".pem")
    return {name: directory / name for name in ("ca.pem", "server.pem", "server.key", "wrong.pem", "wrong.key")}


class Fixture(http.server.ThreadingHTTPServer):
    daemon_threads = True
    block_on_close = False

    def __init__(self, cert: Path | None = None, key: Path | None = None):
        self.events: list[dict] = []
        self.overrides: dict[str, tuple[int, object, dict[str, str]]] = {}
        self.delays: dict[str, float] = {}
        self.slow_body: dict[str, float] = {}
        self.disconnect: set[str] = set()
        self.kind = "prometheus"
        super().__init__(("127.0.0.1", 0), Handler)
        if cert:
            context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
            context.load_cert_chain(cert, key)
            self.socket = context.wrap_socket(self.socket, server_side=True)
        self.origin = ("https" if cert else "http") + f"://127.0.0.1:{self.server_port}/grafana"
        self.thread = threading.Thread(target=self.serve_forever, kwargs={"poll_interval": 0.02}, daemon=True)
        self.thread.start()

    def reset(self, *, kind: str = "prometheus") -> None:
        self.events.clear()
        self.overrides = {}
        self.delays = {}
        self.slow_body = {}
        self.disconnect = set()
        self.kind = kind

    def close(self) -> None:
        self.shutdown()
        self.server_close()
        self.thread.join(timeout=2)
        assert not self.thread.is_alive()

    def handle_error(self, request, client_address) -> None:
        # Expected TLS disconnect/broken-pipe fixture paths never print request headers.
        pass


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args) -> None:
        pass

    def do_GET(self) -> None:
        self.exchange()

    def do_POST(self) -> None:
        self.exchange()

    def exchange(self) -> None:
        fixture: Fixture = self.server
        length = int(self.headers.get("Content-Length", "0"))
        if not 0 <= length <= 128 * 1024:
            self.send_error(413)
            return
        body = self.rfile.read(length)
        fixture.events.append({"method": self.command, "path": self.path,
                               "headers": {k.lower(): v for k, v in self.headers.items()},
                               "auth_headers": self.headers.get_all("Authorization", []),
                               "org_headers": self.headers.get_all("X-Grafana-Org-Id", []),
                               "body": body, "at": time.monotonic()})
        if self.path == "/grafana/api/org":
            stage, response = "organization", {"id": 7}
        elif self.path == "/grafana/api/dashboards/uid/board-1":
            stage, response = "dashboard", json.loads((FIXTURES / "dashboard.json").read_text())
        elif self.path == f"/grafana/api/datasources/uid/metrics-1?ds_type={fixture.kind}":
            stage, response = "datasource", {"uid": "metrics-1", "type": fixture.kind, "orgId": 7}
        elif self.path == "/grafana/api/ds/query":
            stage, response = "query", json.loads((FIXTURES / (fixture.kind + ".json")).read_text())
        else:
            stage, response = "unexpected", {"error": "unexpected fixture route"}
        status, response, headers = fixture.overrides.get(stage, (200, response, {}))
        delay = fixture.delays.get(stage, 0)
        body_delay = fixture.slow_body.get(stage, 0)
        disconnect = stage in fixture.disconnect
        if disconnect:
            self.close_connection = True
            self.connection.shutdown(socket.SHUT_RDWR)
            self.connection.close()
            return
        if delay:
            time.sleep(delay)
        raw = response if isinstance(response, bytes) else json.dumps(response).encode("utf-8")
        try:
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(raw)))
            for key, value in headers.items():
                self.send_header(key, value)
            self.end_headers()
            if body_delay and raw:
                self.wfile.write(raw[:1])
                self.wfile.flush()
                time.sleep(body_delay)
                raw = raw[1:]
            self.wfile.write(raw)
            self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError, ssl.SSLError):
            self.close_connection = True


def assert_finite(value: object) -> None:
    if isinstance(value, dict):
        for item in value.values():
            assert_finite(item)
    elif isinstance(value, list):
        for item in value:
            assert_finite(item)
    elif isinstance(value, float):
        assert math.isfinite(value)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--no-schema", action="store_true", help="For the Cally stdlib-only lane; disclose skipped schema checks")
    options = parser.parse_args()
    binary = options.binary.resolve(strict=True)
    schemas = {p.stem.removesuffix(".schema"): json.loads(p.read_text())
               for p in (ROOT / "docs/sre-workbench/schemas").glob("*.schema.json")}
    if not options.no_schema:
        from jsonschema import Draft202012Validator, FormatChecker
        from referencing import Registry, Resource
        registry = Registry().with_resources((s["$id"], Resource.from_contents(s)) for s in schemas.values())

    def validate(name: str, value: object) -> None:
        if not options.no_schema:
            Draft202012Validator(schemas[name], registry=registry, format_checker=FormatChecker()).validate(value)

    count = 0
    with tempfile.TemporaryDirectory(prefix="workbench-grafana-contract-") as directory:
        scratch = Path(directory)
        cert = certificates(scratch)
        fixture = Fixture(cert["server.pem"], cert["server.key"])
        proxy = Fixture()
        wrong_hostname = Fixture(cert["wrong.pem"], cert["wrong.key"])
        try:
            probe = http.client.HTTPSConnection("127.0.0.1", fixture.server_port,
                context=ssl.create_default_context(cafile=str(cert["ca.pem"])), timeout=3)
            try:
                probe.request("GET", "/grafana/api/org")
                observed = probe.getresponse()
                assert observed.status == 200 and json.loads(observed.read()) == {"id": 7}
            finally:
                probe.close()
            fixture.reset()
            config_file = scratch / "operator.json"
            request_file = scratch / "request.json"
            config = {"spec_version": "0.1", "record": "never", "connections": {"fixture": {
                "origin": fixture.origin, "expected_org_id": 7, "api_profile": "grafana-legacy-v1",
                "credential_ref": "env:" + PROVIDER, "tls_ca_file": str(cert["ca.pem"])}}}
            provider = {"origin": fixture.origin, "expected_org_id": 7,
                        "operations": ["grafana.dashboard.get", "grafana.query"],
                        "auth": {"kind": "bearer", "token": TOKEN}}

            def configure(configuration: object = config, credentials: object = provider) -> dict[str, str]:
                config_file.write_text(json.dumps(configuration), encoding="utf-8")
                return dict(os.environ, **{PROVIDER: json.dumps(credentials)})

            env = configure()

            def receipt(raw: bytes) -> dict:
                nonlocal count
                assert len(raw) <= 2 * 1024 * 1024
                result = json.loads(raw)
                assert_finite(result)
                validate("result", result)
                assert result["assessment"] == "not_assessed" and result["record_mode"] == "never"
                if result["operation"] in {"grafana.dashboard.get", "grafana.query"}:
                    if result["data"] == {}:
                        # Shared envelope admission occurs before an adapter is dispatched.
                        assert result["effect_outcome"] == "not_attempted" and result["resolved_target"] == {}
                        assert result["execution"]["status"] in {"denied", "unsupported"}
                        assert {e["code"] for e in result["errors"]} == {"unsupported_target"}
                        count += 1
                        return result
                    validate("grafana-dashboard-data" if result["operation"].endswith(".get") else "grafana-query-data", result["data"])
                    adapter = result["data"]["adapter"]
                    assert adapter["id"] == "grafana" and adapter["version"] == 1
                    assert adapter["api_profile"] == "grafana-legacy-v1"
                    assert adapter["compatibility_evidence"] == "fixture_only"
                    assert adapter["upstream_digest"] == "sha256:1c1711d8bd1c70e49e5715813506a0a4389021041f457d9d6b1a95a39f8fc3f7"
                    assert result["execution"]["child_exit_code"] is None
                count += 1
                return result

            def run(*argv: str, expected: int = 0, environment: dict | None = None) -> dict:
                completed = subprocess.run([str(binary), "--config", str(config_file), "--json", *argv],
                                           env=env if environment is None else environment,
                                           capture_output=True, timeout=8, check=False)
                assert completed.returncode == expected, (argv[:3], completed.returncode,
                                                          completed.stdout[:1000], completed.stderr[:500])
                return receipt(completed.stdout)

            dashboard = ("grafana", "dashboard", "get", "--target", "fixture", "--uid", "board-1")
            query = ("grafana", "query", "--target", "fixture", "--datasource", "metrics-1", "--kind", "prometheus",
                     "--from", START, "--to", END, "--expr", "up")

            result = run(*dashboard)
            assert result["execution"]["status"] == "succeeded" and result["effect_outcome"] == "not_applicable"
            assert result["data"]["response"] == json.loads((FIXTURES / "dashboard.json").read_text())
            assert result["data"]["binding"] == {"origin": fixture.origin, "organization_id": 7, "organization_verified": True}
            assert [(e["method"], e["path"]) for e in fixture.events] == [
                ("GET", "/grafana/api/org"), ("GET", "/grafana/api/dashboards/uid/board-1")]
            for event in fixture.events:
                assert event["headers"]["authorization"] == "Bearer " + TOKEN
                assert event["headers"]["x-grafana-org-id"] == "7"
                assert event["auth_headers"] == ["Bearer " + TOKEN] and event["org_headers"] == ["7"]
                assert not event["body"]
            assert result["data"]["transport"]["requests_started"] == 2
            assert result["data"]["redaction"] == {"applied": False, "replacements": 0}

            for kind in ("prometheus", "loki"):
                fixture.reset(kind=kind)
                arguments = tuple(kind if arg == "prometheus" else arg for arg in query)
                observed = run(*arguments)
                assert observed["execution"]["status"] == "succeeded"
                assert observed["coverage"]["state"] == "not_established"
                assert observed["data"]["response"] == json.loads((FIXTURES / (kind + ".json")).read_text())
                assert [(e["method"], e["path"]) for e in fixture.events] == [
                    ("GET", "/grafana/api/org"), ("GET", f"/grafana/api/datasources/uid/metrics-1?ds_type={kind}"),
                    ("POST", "/grafana/api/ds/query")]
                body = json.loads(fixture.events[-1]["body"])
                expected_query = {"refId": "A", "datasource": {"uid": "metrics-1", "type": kind},
                                  "expr": "up", "maxDataPoints": 1000, "intervalMs": 1000}
                expected_query.update({"range": True, "instant": False, "format": "time_series"}
                                      if kind == "prometheus" else {"queryType": "range", "maxLines": 500})
                assert body == {"from": "1790899200000", "to": "1790899260000", "queries": [expected_query]}
                assert all(e["headers"]["authorization"] == "Bearer " + TOKEN and
                           e["headers"]["x-grafana-org-id"] == "7" for e in fixture.events)

            fixture.reset()
            fixture.overrides["query"] = (200, {"results": {"A": {"frames": []}}}, {})
            empty = run(*query)
            assert empty["execution"]["status"] == "succeeded" and empty["coverage"]["state"] == "not_established"
            assert empty["data"]["response"]["results"]["A"]["frames"] == []
            request = {"spec_version": "0.1", "request_id": "grafana-contract-001", "operation": "grafana.query",
                       "operation_version": 1, "target": {"kind": "connection", "id": "fixture"},
                       "inputs": {"datasource": "metrics-1", "kind": "prometheus", "from": START, "to": END, "expr": "up"},
                       "limits": {"timeout_ms": 3000, "max_output_bytes": 1048576}, "record": "never"}
            request_file.write_text(json.dumps(request))
            called = run("call", "--request", str(request_file))
            for key in ("adapter", "binding", "request", "response", "redaction"):
                assert called["data"][key] == empty["data"][key], key

            for stage, payload, expected_requests in (
                ("organization", {"id": 8}, 1), ("organization", {"id": True}, 1),
                ("organization", {"id": "7"}, 1),
                ("datasource", {"uid": "other", "type": "prometheus", "orgId": 7}, 2),
                ("datasource", {"uid": "metrics-1", "type": "loki", "orgId": 7}, 2),
                ("datasource", {"uid": "metrics-1", "type": "prometheus", "orgId": 8}, 2),
                ("query", {"results": {"A": {"status": 500, "error": TOKEN, "frames": []}}}, 3),
                ("query", {"error": TOKEN}, 3), ("query", {"results": {}}, 3),
                ("query", {"results": {"A": {}, "B": {"frames": []}}}, 3)):
                fixture.reset()
                fixture.overrides[stage] = (200, payload, {})
                failed = run(*query, expected=1)
                assert failed["execution"]["status"] != "succeeded" and failed["errors"]
                assert failed["data"]["response"] is None and len(fixture.events) == expected_requests
                assert TOKEN not in json.dumps(failed)

            for payload in (b'not-json', b'{"id":7,"id":7}', b'{"id":7,"value":NaN}',
                            b'{"id":7,"value":1e999}', b'null', b'{"id":7,"deep":' + b'[' * 65 + b'0' + b']' * 65 + b'}'):
                fixture.reset()
                fixture.overrides["organization"] = (200, payload, {})
                failed = run(*dashboard, expected=1)
                assert failed["errors"] and len(fixture.events) == 1

            for status, headers in ((401, {}), (503, {}), (302, {"Location": fixture.origin + "/redirected"})):
                fixture.reset()
                fixture.overrides["organization"] = (status, {"error": TOKEN}, headers)
                failed = run(*dashboard, expected=1)
                assert TOKEN not in json.dumps(failed) and len(fixture.events) == 1
            fixture.reset()
            fixture.disconnect = {"organization"}
            failed = run(*dashboard, expected=1)
            assert failed["errors"] and len(fixture.events) == 1, "No retry after the server closes a dispatched request"

            fixture.reset()
            hostile = dict(env, HTTP_PROXY=proxy.origin, HTTPS_PROXY=proxy.origin, ALL_PROXY=proxy.origin,
                           http_proxy=proxy.origin, https_proxy=proxy.origin, all_proxy=proxy.origin,
                           NO_PROXY="", no_proxy="", SSLKEYLOGFILE=str(scratch / "tls-keylog"))
            run(*dashboard, environment=hostile)
            assert not proxy.events and not (scratch / "tls-keylog").exists()
            for change in ("no_ca", "wrong_hostname"):
                fixture.reset()
                modified = copy.deepcopy(config)
                bound = copy.deepcopy(provider)
                if change == "no_ca":
                    modified["connections"]["fixture"].pop("tls_ca_file")
                else:
                    modified["connections"]["fixture"]["origin"] = wrong_hostname.origin
                    bound["origin"] = wrong_hostname.origin
                isolated_env = configure(modified, bound)
                # Supplying the correct CA through ambient variables must not override explicit trust.
                isolated_env.update(SSL_CERT_FILE=str(cert["ca.pem"]), SSL_CERT_DIR=str(scratch))
                failed = run(*dashboard, expected=1, environment=isolated_env)
                assert failed["errors"] and not fixture.events and not wrong_hostname.events
            env = configure()

            for credential in ("true", "false", "null", "741829"):
                fixture.reset()
                bound = copy.deepcopy(provider)
                bound["auth"]["token"] = credential
                primitive = json.loads(credential)
                fixture.overrides["dashboard"] = (200, {"dashboard": {"uid": "board-1", "echo": primitive}, "meta": {}}, {})
                primitive_env = configure(config, bound)
                redacted = run(*dashboard, environment=primitive_env)
                assert redacted["execution"]["status"] == "succeeded"
                assert redacted["data"]["response"]["dashboard"]["echo"] == {"redacted": True}
                assert redacted["data"]["redaction"]["applied"] and redacted["data"]["redaction"]["replacements"] >= 1
            env = configure()

            fixture.reset()
            user, password = "fixture-person", "fixture p@ss/+= café"
            pair = user + ":" + password
            encoded = base64.b64encode(pair.encode()).decode()
            secrets = [user, password, pair, encoded, "Basic " + encoded]
            variants = sorted({value for secret in secrets for value in
                               (secret, quote(secret, safe=""), quote_plus(secret), base64.b64encode(secret.encode()).decode())})
            bound = copy.deepcopy(provider)
            bound["auth"] = {"kind": "basic", "username": user, "password": password}
            fixture.overrides["dashboard"] = (200, {"dashboard": {"uid": "board-1", "values": variants,
                password: "echo", "Authorization": "arbitrary-private-header", "link": "https://private-person:private-pass@host.invalid/a"}, "meta": {}}, {})
            basic = run(*dashboard, environment=configure(config, bound))
            rendered = json.dumps(basic, ensure_ascii=False)
            assert all(secret not in rendered for secret in variants)
            assert "arbitrary-private-header" not in rendered and "private-person" not in rendered and "private-pass" not in rendered
            assert all(e["headers"]["authorization"] == "Basic " + encoded for e in fixture.events)
            env = configure()
            fixture.reset()
            fixture.overrides["dashboard"] = (200, {"dashboard": {"uid": "board-1", TOKEN: 1, "[REDACTED]": 2}, "meta": {}}, {})
            collision = run(*dashboard, expected=1)
            assert collision["data"]["response"] is None and TOKEN not in json.dumps(collision)

            fixture.reset()
            modified = copy.deepcopy(config)
            bound = copy.deepcopy(provider)
            expanded_origin = fixture.origin + "/" + "é" * 400
            modified["connections"]["fixture"]["origin"] = expanded_origin
            bound["origin"] = expanded_origin
            denied = run(*dashboard, expected=2, environment=configure(modified, bound))
            assert denied["effect_outcome"] == "not_attempted" and not fixture.events
            env = configure()

            for key, value in (("origin", "http://127.0.0.1:1"), ("api_profile", "unknown-profile"),
                               ("expected_org_id", 0), ("tls_ca_file", "relative.pem"), ("insecure_tls", True)):
                fixture.reset()
                modified = copy.deepcopy(config)
                modified["connections"]["fixture"][key] = value
                denied = run(*dashboard, expected=2, environment=configure(modified))
                assert denied["effect_outcome"] == "not_attempted" and not fixture.events
            for origin in (fixture.origin.replace("https:", "http:"),
                           fixture.origin.replace("https://", "https://fixture-user:fixture-pass@"),
                           fixture.origin + "?token=synthetic", fixture.origin + "#fragment",
                           fixture.origin + "/%2e%2e", fixture.origin + "/../elsewhere",
                           fixture.origin + "/\\elsewhere", fixture.origin + " "):
                fixture.reset()
                modified, bound = copy.deepcopy(config), copy.deepcopy(provider)
                modified["connections"]["fixture"]["origin"] = origin
                bound["origin"] = origin
                denied = run(*dashboard, expected=2, environment=configure(modified, bound))
                assert denied["effect_outcome"] == "not_attempted" and not fixture.events
            for change in ({"origin": fixture.origin + "/other"}, {"expected_org_id": 8},
                           {"operations": ["grafana.query"]}, {"auth": {"kind": "bearer", "token": "bad\nheader"}}):
                fixture.reset()
                bound = copy.deepcopy(provider)
                bound.update(change)
                denied = run(*dashboard, expected=2, environment=configure(config, bound))
                assert denied["effect_outcome"] == "not_attempted" and not fixture.events
            env = configure()
            fixture.reset()
            for raw in ('{"spec_version":"0.1","spec_version":"0.1"}', '["0.1","never",{}]', 'null'):
                config_file.write_text(raw)
                denied = run(*dashboard, expected=2)
                assert denied["effect_outcome"] == "not_attempted" and not fixture.events
            env = configure()
            for raw in ('{"origin":"one","origin":"two"}', '["origin",7,[],{}]', 'null'):
                denied = run(*dashboard, expected=2, environment={**env, PROVIDER: raw})
                assert denied["effect_outcome"] == "not_attempted" and not fixture.events
            config_file.unlink()
            os.mkfifo(config_file)
            try:
                denied = run(*dashboard, expected=2)
                assert denied["effect_outcome"] == "not_attempted" and not fixture.events
            finally:
                config_file.unlink()
            env = configure()
            for change in ({"expr": "$__rate_interval"}, {"expr": "[[server]]"}, {"expr": "up\n"},
                           {"expr": " "}, {"expr": "x" * 16001}, {"datasource": "../other"},
                           {"from": "2026-10-02T00:00:00.000001Z"}, {"to": "2026-10-03T00:00:00.001Z"},
                           {"from": "2026-10-02T00:00:00.0000000001Z"},
                           {"to": "2026-10-02T00:01:00.0010000000001Z"},
                           {"from": "2026-10-02.00:00:00.0000000001Z"},
                           {"from": "2026-10-02X00:00:00Z"},
                           {"from": "2026-10-02 00:00:00Z"},
                           {"origin": fixture.origin}, {"credential_ref": "env:OTHER"}):
                fixture.reset()
                malformed = copy.deepcopy(request)
                malformed["inputs"].update(change)
                request_file.write_text(json.dumps(malformed))
                denied = run("call", "--request", str(request_file), expected=2)
                assert denied["effect_outcome"] == "not_attempted" and not fixture.events

            for timestamp in ("2026-10-02T00:00:00.000000000000Z", "2026-10-02t00:00:00.000000000000z"):
                fixture.reset()
                precise = copy.deepcopy(request)
                precise["inputs"]["from"] = timestamp
                request_file.write_text(json.dumps(precise))
                accepted = run("call", "--request", str(request_file))
                assert accepted["execution"]["status"] == "succeeded"
                assert json.loads(fixture.events[-1]["body"])["from"] == "1790899200000"

            fixture.reset()
            marker = scratch / "wrong-target-child-ran"
            process_request = {**request, "operation": "process.exec",
                               "target": {"kind": "local", "id": "workstation"},
                               "inputs": {"program": "/usr/bin/touch", "args": [str(marker)], "cwd": str(scratch)}}
            request_file.write_text(json.dumps(process_request))
            run("call", "--request", str(request_file))
            assert marker.exists() and not fixture.events
            marker.unlink()
            process_request["target"] = {"kind": "connection", "id": "fixture"}
            task_request = {**process_request, "operation": "task.run",
                            "inputs": {"id": "error-budget", "version": 1, "input": {"slo": 99.9}}}
            grafana_local = {**request, "target": {"kind": "local", "id": "workstation"}}
            for wrong_target in (process_request, task_request, grafana_local):
                request_file.write_text(json.dumps(wrong_target))
                denied = run("call", "--request", str(request_file), expected=2)
                assert denied["effect_outcome"] == "not_attempted" and not marker.exists() and not fixture.events

            fixture.reset()
            offset = copy.deepcopy(request)
            offset["inputs"]["from"] = "2026-10-01T19:00:00-05:00"
            offset["inputs"]["to"] = "2026-10-01T19:01:00-05:00"
            request_file.write_text(json.dumps(offset))
            normalized = run("call", "--request", str(request_file))
            assert datetime.fromisoformat(normalized["data"]["request"]["from"]) == datetime.fromisoformat(START)
            assert json.loads(fixture.events[-1]["body"])["from"] == "1790899200000"

            fixture.reset()
            for arguments in (("capabilities", "list"), ("capabilities", "describe", "grafana.query"), ("doctor",)):
                discovery = run(*arguments)
                assert TOKEN not in json.dumps(discovery) and not fixture.events

            fixture.reset()
            fixture.overrides["dashboard"] = (200, {"dashboard": {"uid": "board-1", "pad": "x" * 2048}, "meta": {}}, {})
            oversized = run(*dashboard, "--max-output-bytes", "1024", expected=1)
            assert oversized["execution"]["status"] != "succeeded" and oversized["data"]["response"] is None
            fixture.reset()
            fixture.overrides["dashboard"] = (200, {"dashboard": {"uid": "board-1", "pad": "x" * (2 * 1024 * 1024 - 256)}, "meta": {}}, {})
            encoded_limit = run(*dashboard, "--max-output-bytes", "2097152", expected=1)
            assert encoded_limit["execution"]["status"] != "succeeded" and encoded_limit["data"]["response"] is None
            assert encoded_limit["data"]["transport"]["response_limit_bytes"] == 2097152
            assert encoded_limit["effective_limits"]["max_output_bytes"] == 2097152
            assert {error["code"] for error in encoded_limit["errors"]} == {"structured_response_too_large"}

            # Probe both sides of the fully finalized envelope boundary. The empty-pad
            # receipt supplies its actual overhead, including sources and final timing.
            fixture.reset()
            fixture.overrides["dashboard"] = (200, {"dashboard": {"uid": "board-1", "pad": ""}, "meta": {}}, {})
            empty_pad = run(*dashboard)
            overhead = len(json.dumps(empty_pad, ensure_ascii=False, separators=(",", ":")).encode()) + 1
            for margin in (32, 0, -32, -128):
                fixture.reset()
                padding = 2 * 1024 * 1024 - overhead - margin
                fixture.overrides["dashboard"] = (200, {"dashboard": {"uid": "board-1", "pad": "x" * padding}, "meta": {}}, {})
                edge = subprocess.run([str(binary), "--config", str(config_file), "--json", *dashboard],
                                      env=env, capture_output=True, timeout=8, check=False)
                bounded = receipt(edge.stdout)
                if bounded["data"]["response"] is None:
                    assert edge.returncode == 1 and bounded["execution"]["status"] == "failed"
                    assert {error["code"] for error in bounded["errors"]} == {"structured_response_too_large"}
                else:
                    assert edge.returncode == 0 and bounded["execution"]["status"] == "succeeded"
                    assert len(bounded["data"]["response"]["dashboard"]["pad"]) == padding
            fixture.reset()
            compressed = gzip.compress(json.dumps({"id": 7}).encode())
            fixture.overrides["organization"] = (200, compressed, {"Content-Encoding": "gzip"})
            failed = run(*dashboard, expected=1)
            assert failed["errors"] and len(fixture.events) == 1

            fixture.reset()
            fixture.delays = {"organization": 0.20, "datasource": 0.20}
            started = time.monotonic()
            timed = run(*query, "--timeout", "300ms", expected=1)
            assert time.monotonic() - started < 3
            assert timed["execution"]["status"] == "timed_out" and timed["data"]["response"] is None
            assert not any(e["method"] == "POST" for e in fixture.events)
            fixture.reset()
            fixture.slow_body = {"dashboard": 0.5}
            timed = run(*dashboard, "--timeout", "150ms", expected=1)
            assert timed["execution"]["status"] == "timed_out" and timed["data"]["response"] is None

            for sig, expected_code in ((signal.SIGINT, 130), (signal.SIGTERM, 143)):
                fixture.reset()
                fixture.delays = {"organization": 0.6}
                process = subprocess.Popen([str(binary), "--config", str(config_file), "--json", *dashboard],
                                           env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                try:
                    until = time.monotonic() + 3
                    while not fixture.events and process.poll() is None and time.monotonic() < until:
                        time.sleep(0.005)
                    assert fixture.events and process.poll() is None, "Establish active HTTP phase before cancelling"
                    process.send_signal(sig)
                    output, errors = process.communicate(timeout=3)
                    assert process.returncode == expected_code, (process.returncode, errors)
                    cancelled = receipt(output)
                    assert cancelled["execution"]["status"] == "cancelled"
                    assert cancelled["data"]["cancellation_signal"] == sig
                    assert cancelled["data"]["response"] is None
                    time.sleep(0.05)
                    assert len(fixture.events) == 1
                finally:
                    if process.poll() is None:
                        process.kill()
                        process.communicate(timeout=3)
        finally:
            fixture.close()
            proxy.close()
            wrong_hostname.close()

    print(f"PASS: {count} public Grafana results; HTTPS exchanges, binding, auth/redaction, admission, "
          "bounds, cancellation and existing interfaces. " +
          ("Schema checks explicitly skipped in stdlib-only lane." if options.no_schema else "All receipts/data schema-valid."))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
