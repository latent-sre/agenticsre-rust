"""Real diagnostics HTTP/core contract checks; run only with tools/test-ui-sandbox.sh.

Uses the existing private DNS/HTTPS fixtures and an actual sleeping process. No host listener,
live endpoint or credential is needed. Keeps exact receipts and fixture witnesses in --output.
Failed attempts retain their evidence; every product/fixture process is joined before exit.
"""
from __future__ import annotations

import argparse
import base64
import copy
from contextlib import ExitStack
import hashlib
import http.client
import importlib.util
import json
import os
from pathlib import Path
import select
import shutil
import signal
import subprocess
import tempfile
import time
from urllib.parse import urlsplit
import uuid

from jsonschema import Draft202012Validator, FormatChecker
from referencing import Registry, Resource

ROOT = Path(__file__).resolve().parents[1]


def fixture_module():
    specification = importlib.util.spec_from_file_location("diagnostic_fixtures", ROOT / "tools/verify-diagnostics.py")
    module = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(module)
    return module


class Server:
    def __init__(self, binary: Path, config: Path, directory: Path, targets: tuple[str, ...]) -> None:
        command = [str(binary), "--diagnostics-config", str(config), "ui", "serve", "--allow", "diagnostics.run"]
        for target in targets:
            command += ["--target", target]
        self.child = subprocess.Popen(command, cwd=directory, stdin=subprocess.DEVNULL,
                                      stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            ready, _, _ = select.select([self.child.stdout], [], [], 5)
            assert ready, "HTTP server launch exceeded5s"
            line = self.child.stdout.readline().strip()
            launch = urlsplit(line)
            if not (launch.hostname == "127.0.0.1" and launch.fragment.startswith("token=")):
                self.child.wait(timeout=2)
                rest = self.child.stdout.read(2048)
                reason = "invalid_usage" if "invalid_usage" in line + rest else "no_launch_link"
                raise AssertionError(f"HTTP launch refused: exit={self.child.returncode}, reason={reason}; " + self.child.stderr.read(2048))
            self.authority = launch.netloc
            self.origin = f"http://{self.authority}"
            self.token = launch.fragment.removeprefix("token=")
            assert len(self.token) == 43
        except BaseException:
            self.close()
            raise

    def close(self, number: int = signal.SIGTERM) -> None:
        if self.child.poll() is None:
            self.child.send_signal(number)
            try:
                self.child.wait(timeout=6)
            except subprocess.TimeoutExpired:
                self.child.kill()
                self.child.wait(timeout=2)
                raise AssertionError("HTTP worker shutdown/cleanup exceeded6s") from None
        for stream in (self.child.stdout, self.child.stderr):
            if stream is not None:
                stream.close()

    def request(self, method: str, path: str, value: object | None = None,
                *, overrides: dict[str, str] | None = None) -> tuple[int, dict, bytes]:
        connection = http.client.HTTPConnection(self.authority, timeout=6)
        headers = {"Authorization": f"Bearer {self.token}"}
        if method in ("POST", "DELETE"):
            headers.update({"Origin": self.origin, "Content-Type": "application/json"})
        headers.update(overrides or {})
        try:
            body = None if value is None else json.dumps(value).encode()
            connection.request(method, path, body=body, headers=headers)
            response = connection.getresponse()
            payload = response.read(2_097_153)
            assert len(payload) <= 2_097_152, "HTTP receipt ceiling"
            expected_type = "application/problem+json" if response.status >= 400 else "application/json"
            assert response.getheader("Content-Type") == expected_type
            return response.status, json.loads(payload), payload
        finally:
            connection.close()


def wait_for(predicate, detail: str, timeout: float = 5) -> None:
    deadline = time.monotonic() + timeout
    while not predicate():
        assert time.monotonic() < deadline, detail
        time.sleep(0.01)


def stop_process(child: subprocess.Popen) -> None:
    if child.poll() is None:
        child.terminate()
    child.wait(timeout=2)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--output", type=Path, required=True, help="Retained evidence directory; must not already exist")
    parser.add_argument("--json-results", action="store_true", help="Also print exact canonical receipts")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    specification = json.loads((ROOT / "docs/ui/openapi.json").read_text(encoding="utf-8"))
    core_schemas = {path.name.removesuffix(".schema.json"): json.loads(path.read_text(encoding="utf-8"))
                    for path in (ROOT / "docs/sre-workbench/schemas").glob("*.schema.json")}
    registry = Registry().with_resources((schema["$id"], Resource.from_contents(schema)) for schema in core_schemas.values())
    core_validators = {name: Draft202012Validator(core_schemas[name], registry=registry, format_checker=FormatChecker())
                       for name in ("result", "diagnostics-data")}
    checked = 0
    receipts = []
    witnesses = {}
    fixture = fixture_module()

    def validate(name: str, value: object) -> None:
        nonlocal checked
        schema = {"$ref": f"#/components/schemas/{name}", "components": specification["components"]}
        Draft202012Validator(schema, format_checker=FormatChecker()).validate(value)
        checked += 1

    def receipt(payload: bytes) -> dict:
        value = json.loads(payload)
        validate("CoreResult", value)
        core_validators["result"].validate(value)
        if value["data"].get("pack") is not None:
            validate("CoreDiagnosticsData", value["data"])
            core_validators["diagnostics-data"].validate(value["data"])
        assert value["assessment"] == "not_assessed" and value["record_mode"] == "never"
        assert value["execution"]["child_exit_code"] is None and value["execution"]["signal"] is None
        receipts.append(value)
        with (args.output / "receipts.jsonl").open("ab") as stream:
            stream.write(payload.rstrip(b"\n") + b"\n")
        if args.json_results:
            print(payload.decode().strip())
        return value

    for name, schema in specification["components"]["schemas"].items():
        for example in schema.get("examples", []):
            validate(name, example)

    with tempfile.TemporaryDirectory(prefix="workbench-ui-diagnostics-") as temporary, ExitStack() as resources:
        directory = Path(temporary)
        installed = directory / "installed" / "save"
        installed.parent.mkdir()
        shutil.copyfile(args.binary.resolve(), installed)
        installed.chmod(0o700)
        binary_sha = hashlib.sha256(installed.read_bytes()).hexdigest()
        (args.output / "binary.json").write_text(json.dumps({"sha256": binary_sha, "size": installed.stat().st_size}) + "\n")
        certs = fixture.certificates(directory)
        dns = fixture.Dns()
        resources.callback(dns.close)
        https = fixture.Https(certs["server.pem"], certs["server.key"])
        resources.callback(https.close)
        process = subprocess.Popen(["/usr/bin/sleep", "300"], stdin=subprocess.DEVNULL,
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        resources.callback(stop_process, process)
        server = None
        try:
            target = {"id": "fixture-web", "host": "diagnostics.workbench.test", "port": https.port,
                      "path": "/probe", "allowed_ips": ["127.0.0.1"],
                      "resolver": {"kind": "explicit", "address": "127.0.0.1", "port": dns.port, "transport": "udp"},
                      "tls_ca_file": str(certs["ca.pem"]), "process": fixture.process_identity(process.pid)}
            config = directory / "targets.json"
            config_bytes = json.dumps({"schema_version": 1, "targets": [target, {**target, "id": "fixture-other"},
                                      {**target, "id": "hidden", "tls_ca_file": str(directory / "must-not-open.pem")}]}).encode()
            config.write_bytes(config_bytes)
            ca_bytes = certs["ca.pem"].read_bytes()

            def effects() -> tuple[int, int, int]:
                return len(dns.events), https.connections, len(https.events)

            def api(method: str, path: str, status: int, schema: str, value=None, **kwargs):
                code, result, raw = server.request(method, path, value, **kwargs)
                assert code == status, (method, path, code, status, result)
                validate(schema, result)
                return result, raw

            def submission(**changes) -> dict:
                value = {"submission_id": str(uuid.uuid4()), "operation": "diagnostics.run", "operation_version": 1,
                         "target_id": "fixture-web", "inputs": {"id": "connectivity", "version": 1, "input": {}},
                         "limits": {"timeout_ms": 30000, "max_output_bytes": 1048576}}
                value.update(changes)
                return value

            def finish(admitted: dict) -> tuple[dict, bytes]:
                run_path = f"/api/v1/runs/{admitted['id']}"
                summary = None

                def terminal() -> bool:
                    nonlocal summary
                    summary, _ = api("GET", run_path, 200, "RunSummary")
                    return summary["state"] == "terminal"

                wait_for(terminal, "real diagnostic worker did not return a terminal receipt")
                assert summary["target_id"] == "fixture-web" and summary["root_id"] is None
                _, raw = api("GET", summary["receipt_url"], 200, "CoreResult")
                return receipt(raw), raw

            def run(value: dict | None = None) -> tuple[dict, bytes]:
                value = submission() if value is None else value
                validate("RunSubmission", value)
                admitted, _ = api("POST", "/api/v1/runs", 202, "RunSummary", value)
                return finish(admitted)

            def refused_identity(code: str) -> None:
                before = effects()
                result, _ = run()
                assert result["execution"]["status"] == "denied" and result["effect_outcome"] == "not_attempted"
                assert result["errors"][0]["code"] == code and result["data"] == {}
                assert effects() == before, "identity refusal started DNS/service effects"

            before = effects()
            server = Server(installed, config, directory, ("fixture-web", "fixture-other"))
            resources.callback(server.close)
            for extra in [[], ["--allow", "diagnostics.run"], ["--target", "fixture-web"],
                          ["--allow", "diagnostics.run", "--target", "missing"],
                          ["--allow", "diagnostics.run", "--target", "fixture-web", "--target", "fixture-web"],
                          ["--allow", "diagnostics.run", "--allow", "diagnostics.run", "--target", "fixture-web"],
                          ["--allow", "diagnostics.run", "--target", "hidden"]]:
                failed = subprocess.run([str(installed), "--diagnostics-config", str(config), "ui", "serve", *extra],
                                        cwd=directory, capture_output=True, timeout=5)
                assert failed.returncode in (1, 2) and not failed.stdout, "invalid startup unexpectedly created a session"
            failed = subprocess.run([str(installed), "ui", "serve", "--allow", "diagnostics.run", "--target", "fixture-web"],
                                    cwd=directory, capture_output=True, timeout=5)
            assert failed.returncode == 1 and b"--diagnostics-config" in failed.stderr
            assert effects() == before

            session, _ = api("GET", "/api/v1/session", 200, "Session")
            assert session["roots"] == [] and session["targets"] == [
                {"id": "fixture-other", "label": "fixture-other"}, {"id": "fixture-web", "label": "fixture-web"}]
            assert session["operations"] == specification["components"]["schemas"]["Session"]["examples"][0]["operations"]
            assert "hidden" not in json.dumps(session) and str(directory) not in json.dumps(session)
            assert effects() == before, "session discovery probed a target"
            print("PASS: explicit startup grants and discovery reveal only selected aliases/fixed metadata")

            initial = submission()
            mission, original = run(initial)
            assert mission["execution"]["status"] == "succeeded" and mission["coverage"]["state"] == "complete"
            assert all(step["state"] == "succeeded" and step["attempted"] for step in mission["data"]["steps"])
            assert mission["effective_limits"]["concurrency"] == 2 and mission["effective_limits"]["max_output_bytes"] == 16384
            assert mission["data"]["binding"]["identity"]["configuration_sha256"] == "sha256:" + hashlib.sha256(config_bytes).hexdigest()
            assert mission["data"]["binding"]["identity"]["additional_ca_sha256"] == "sha256:" + hashlib.sha256(ca_bytes).hexdigest()
            assert {event["kind"] for event in dns.events} == {1, 28}
            assert https.connections == 3 and len(https.events) == 1
            assert https.events[0]["request"].startswith("GET /probe HTTP/1.1\r\n")
            assert mission["data"]["steps"][4]["observation"]["pid"] == process.pid
            listed, _ = api("GET", "/api/v1/runs?limit=50&offset=0", 200, "RunList")
            summary = listed["runs"][0]
            reopened, raw = api("GET", summary["receipt_url"], 200, "CoreResult")
            assert raw == original and reopened == mission, "history/download changed canonical receipt bytes"

            inputs = directory / "pack.json"
            inputs.write_text('{"capture_body":false,"on_failure":"continue"}')
            cli = subprocess.run([str(installed), "--json", "--diagnostics-config", str(config), "diagnose", "--target", "fixture-web",
                                  "--pack", "connectivity", "--input", str(inputs), "--timeout", "30s", "--max-output-bytes", "16384", "--concurrency", "2"],
                                 cwd=directory, capture_output=True, timeout=5)
            assert cli.returncode == 0 and not cli.stderr
            cli_result = receipt(cli.stdout)
            wire = directory / "request.json"
            wire.write_text(json.dumps({"spec_version": "0.1", "request_id": "ui-core-parity", "operation": "diagnostics.run", "operation_version": 1,
                                        "target": {"kind": "connection", "id": "fixture-web"}, "inputs": cli_result["data"]["input"],
                                        "limits": {"timeout_ms": 30000, "max_output_bytes": 16384, "concurrency": 2}, "record": "never"}))
            direct_request = json.loads(wire.read_text())
            direct_request["inputs"] = {"id": "connectivity", "version": 1, "input": direct_request["inputs"]}
            wire.write_text(json.dumps(direct_request))
            direct = subprocess.run([str(installed), "--json", "--diagnostics-config", str(config), "call", "--request", str(wire)],
                                    cwd=directory, capture_output=True, timeout=5)
            assert direct.returncode == 0 and not direct.stderr
            direct_result = receipt(direct.stdout)
            assert fixture.comparable(mission) == fixture.comparable(cli_result) == fixture.comparable(direct_result)
            print("PASS: installed real HTTP→core five-check mission, exact history/download, CLI/call parity")

            before = effects()
            explicit = copy.deepcopy(initial)
            explicit["inputs"]["input"] = {"capture_body": False, "on_failure": "continue"}
            explicit["limits"] = {"timeout_ms": 30000, "max_output_bytes": 16384, "concurrency": 2}
            retry, _ = api("POST", "/api/v1/runs", 202, "RunSummary", explicit)
            assert retry["id"] == summary["id"] and retry["state"] == "terminal"
            for changes in [{"target_id": "fixture-other"}, {"limits": {"timeout_ms": 30000, "max_output_bytes": 16384, "concurrency": 1}},
                            {"limits": {"timeout_ms": 29999, "max_output_bytes": 16384}}, {"limits": {"timeout_ms": 30000, "max_output_bytes": 1024}}]:
                conflict = {**explicit, **changes}
                problem, _ = api("POST", "/api/v1/runs", 409, "Problem", conflict)
                assert problem["type"].endswith("submission-conflict")
            for field, value in [("root_id", None), ("root_id", "checkout"), ("target_id", None), ("target_id", ["fixture-web"]),
                                 ("inputs", ["connectivity", 1, {}]), ("inputs", {"id": "connectivity", "version": 1, "input": []}),
                                 ("inputs", {"id": "connectivity", "version": 2, "input": {}}),
                                 ("limits", {"timeout_ms": 60001, "max_output_bytes": 16384}),
                                 ("limits", {"timeout_ms": 30000, "max_output_bytes": 1023}),
                                 ("limits", {"timeout_ms": 30000, "max_output_bytes": 2097153}),
                                 *[("limits", {"timeout_ms": 30000, "max_output_bytes": 16384, "concurrency": value}) for value in (None, 0, 3, True, "2")],
                                 *[("inputs", {"id": "connectivity", "version": 1, "input": {name: value}})
                                   for name, value in [("capture_body", None), ("capture_body", 1), ("on_failure", None), ("on_failure", "fallback"),
                                                       ("url", "https://private.invalid"), ("pid", 1), ("tls_ca_file", "/private")]]]:
                invalid = submission(**{field: value})
                api("POST", "/api/v1/runs", 422, "Problem", invalid)
            api("POST", "/api/v1/runs", 403, "Problem", submission(target_id="hidden"))
            api("POST", "/api/v1/runs", 403, "Problem", submission(operation="process.exec"))
            api("GET", "/api/v1/session", 401, "Problem", overrides={"Authorization": "Bearer invalid"})
            api("GET", "/api/v1/session", 403, "Problem", overrides={"Host": "attacker.invalid"})
            api("POST", "/api/v1/runs", 403, "Problem", submission(), overrides={"Origin": "null"})
            assert effects() == before, "dedup/boundary refusal started fixture effects"
            print("PASS: effective-default retry dedup, alias/concurrency conflicts, hostile input and HTTP boundary refusals")

            config.write_bytes(config_bytes + b" ")
            refused_identity("diagnostics_identity_changed")
            config.unlink()
            config.symlink_to(directory / "replacement.json")
            (directory / "replacement.json").write_bytes(config_bytes)
            refused_identity("diagnostics_file_read_failed")
            config.unlink()
            os.mkfifo(config)
            refused_identity("invalid_diagnostics_file")
            config.unlink()
            config.write_bytes(config_bytes)
            certs["ca.pem"].write_bytes(ca_bytes + b"\n")
            refused_identity("diagnostics_identity_changed")
            certs["ca.pem"].unlink()
            certs["ca.pem"].symlink_to(directory / "replacement.pem")
            (directory / "replacement.pem").write_bytes(ca_bytes)
            refused_identity("diagnostics_file_read_failed")
            certs["ca.pem"].unlink()
            os.mkfifo(certs["ca.pem"])
            refused_identity("invalid_diagnostics_file")
            certs["ca.pem"].unlink()
            (directory / "replacement.json").replace(config)
            (directory / "replacement.pem").replace(certs["ca.pem"])
            unchanged, _ = run()
            assert unchanged["execution"]["status"] == "succeeded"
            assert unchanged["data"]["binding"]["identity"] == mission["data"]["binding"]["identity"]
            print("PASS: changed config/CA bytes and invalid replacements refuse with no effects; identical regular replacements preserve identity")

            dns.addresses = ["127.0.0.2"]
            partial, _ = run()
            assert partial["execution"]["status"] == "partial"
            assert partial["data"]["steps"][0]["reason"] == "dns_address_denied"
            assert [step["state"] for step in partial["data"]["steps"]][1:4] == ["blocked"] * 3
            assert partial["data"]["steps"][4]["state"] == "succeeded"
            stop, _ = run(submission(inputs={"id": "connectivity", "version": 1, "input": {"on_failure": "stop"}},
                                     limits={"timeout_ms": 30000, "max_output_bytes": 16384, "concurrency": 1}))
            assert stop["data"]["steps"][4]["state"] == "blocked" and not stop["data"]["steps"][4]["attempted"]
            assert stop["data"]["steps"][4]["reason"] == "stopped_after_failure"
            dns.addresses = ["127.0.0.1"]
            hostile = b'<script>fetch("https://private.invalid")</script>\x1b[31m'
            https.response = b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: " + str(len(hostile)).encode() + b"\r\n\r\n" + hostile
            captured, _ = run(submission(inputs={"id": "connectivity", "version": 1, "input": {"capture_body": True}}))
            body = captured["data"]["steps"][3]["observation"]["body_capture"]
            assert base64.b64decode(body["content"]) == hostile and body["classification"] == "untrusted_content"
            assert body["encoding"] == "base64" and not body["redaction_applied"]
            https.response = b"HTTP/1.1 200 OK\r\nContent-Length: 17000\r\n\r\n" + b"a" * 17000
            bounded, _ = run(submission(inputs={"id": "connectivity", "version": 1, "input": {"capture_body": True}},
                                        limits={"timeout_ms": 30000, "max_output_bytes": 1024}))
            assert bounded["data"]["steps"][3]["state"] == "partial" and bounded["data"]["steps"][3]["observation"]["body_capture"]["truncated"]
            print("PASS: actual partial/dependency/stop results, lowered concurrency, untrusted body opt-in and output truncation")

            https.response = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello"
            https.delay = 5
            timed, _ = run(submission(limits={"timeout_ms": 1000, "max_output_bytes": 16384}))
            assert timed["execution"]["status"] == "timed_out"
            assert timed["data"]["steps"][3]["attempted"] and timed["data"]["steps"][3]["reason"] == "parent_deadline_exceeded"
            slow = submission()
            events_before = len(https.events)
            active, _ = api("POST", "/api/v1/runs", 202, "RunSummary", slow)
            wait_for(lambda: len(https.events) > events_before, "cancellation prerequisite: no actual in-flight HTTP GET observed")
            before = effects()
            same, _ = api("POST", "/api/v1/runs", 202, "RunSummary", slow)
            assert same["id"] == active["id"]
            busy, _ = api("POST", "/api/v1/runs", 409, "Problem", submission())
            assert busy["type"].endswith("run-in-progress")
            assert effects() == before
            cleared, _ = api("DELETE", "/api/v1/runs", 200, "ClearResult", {})
            assert cleared["removed"] > 0 and cleared["active_preserved"]
            api("GET", summary["receipt_url"], 410, "Problem")
            tombstone, _ = api("POST", "/api/v1/runs", 202, "RunSummary", explicit)
            assert tombstone["id"] == summary["id"] and tombstone["target_id"] == "fixture-web"
            api("POST", f"/api/v1/runs/{active['id']}/cancel", 202, "RunSummary", {})
            cancelled, _ = finish(active)
            assert cancelled["execution"]["status"] == "cancelled"
            assert cancelled["data"]["cancellation_signal"] == 2
            assert cancelled["data"]["steps"][3]["attempted"] and cancelled["data"]["steps"][3]["state"] == "cancelled"
            api("POST", f"/api/v1/runs/{active['id']}/cancel", 200, "RunSummary", {})
            events_before = len(https.events)
            active, _ = api("POST", "/api/v1/runs", 202, "RunSummary", submission())
            wait_for(lambda: len(https.events) > events_before, "shutdown prerequisite: no actual in-flight HTTP GET observed")
            server.close()
            assert server.child.returncode == 143
            server = None
            print("PASS: real parent timeout, active-preserving clear/tombstones, witnessed HTTP cancellation, busy/dedup and joined SIGTERM shutdown")
            witnesses.update({"dns": dns.events, "https": https.events, "service_connections": https.connections,
                              "process": fixture.process_identity(process.pid), "binary_sha256": binary_sha, "validations": checked})
        finally:
            witnesses.update({"dns": dns.events, "https": https.events, "service_connections": https.connections})
            (args.output / "witnesses.json").write_text(json.dumps(witnesses, indent=2) + "\n")
    print(f"PASS: {checked} schema validations; {len(receipts)} exact receipts; all private fixtures/workers joined")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
