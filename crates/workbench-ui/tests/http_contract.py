"""Validate live HTTP responses against the authoritative OpenAPI; private sandbox only."""
from __future__ import annotations

import argparse
import http.client
import json
from pathlib import Path
import select
import subprocess
import time
from urllib.parse import urlsplit

from jsonschema import Draft202012Validator, FormatChecker


ROOT = Path(__file__).resolve().parents[3]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    specification = json.loads((ROOT / "docs/ui/openapi.json").read_text(encoding="utf-8"))
    checked = 0
    with subprocess.Popen([str(args.binary.resolve()), "ui", "serve", "--allow", "task.run"],
                          stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True) as server:
        try:
            assert server.stdout is not None
            ready, _, _ = select.select([server.stdout], [], [], 5)
            assert ready, "server launch deadline"
            launch = urlsplit(server.stdout.readline().strip())
            assert launch.hostname == "127.0.0.1" and launch.fragment.startswith("token=")
            token = launch.fragment.removeprefix("token=")
            origin = f"http://{launch.netloc}"

            def validate(name: str, value: object) -> None:
                nonlocal checked
                schema = {"$ref": f"#/components/schemas/{name}",
                          "components": specification["components"]}
                Draft202012Validator(schema, format_checker=FormatChecker()).validate(value)
                checked += 1

            def request(method: str, path: str, status: int, schema: str,
                        value: object | None = None, *, authenticated: bool = True) -> dict:
                connection = http.client.HTTPConnection(launch.hostname, launch.port, timeout=3)
                try:
                    headers = {"Origin": origin, "Content-Type": "application/json"}
                    if authenticated:
                        headers["Authorization"] = f"Bearer {token}"
                    body = None if value is None else json.dumps(value)
                    connection.request(method, path, body=body, headers=headers)
                    response = connection.getresponse()
                    payload = response.read(2_097_153)
                    assert response.status == status, (method, path, response.status, status)
                    assert len(payload) <= 2_097_152
                    expected_type = "application/problem+json" if status >= 400 else "application/json"
                    assert response.getheader("Content-Type") == expected_type
                    result = json.loads(payload)
                    validate(schema, result)
                    return result
                finally:
                    connection.close()

            request("GET", "/api/v1/session", 200, "Session")
            request("GET", "/api/v1/session", 401, "Problem", authenticated=False)
            submission = {"submission_id": "01234567-1234-4234-8234-0123456789ab",
                          "operation": "task.run", "operation_version": 1,
                          "inputs": {"id": "error-budget", "version": 1,
                                     "input": {"slo": 99.9, "bad_minutes": 20}},
                          "limits": {"timeout_ms": 30000, "max_output_bytes": 1048576}}
            validate("RunSubmission", submission)
            admitted = request("POST", "/api/v1/runs", 202, "RunSummary", submission)
            run_path = f"/api/v1/runs/{admitted['id']}"
            deadline = time.monotonic() + 5
            while True:
                summary = request("GET", run_path, 200, "RunSummary")
                if summary["state"] == "terminal":
                    break
                assert time.monotonic() < deadline, "receipt deadline"
                time.sleep(0.01)
            receipt = request("GET", summary["receipt_url"], 200, "CoreResult")
            assert receipt["execution"]["status"] == "succeeded"
            request("GET", "/api/v1/runs?limit=50&offset=0", 200, "RunList")
            request("POST", run_path + "/cancel", 200, "RunSummary", {})
            request("POST", "/api/v1/runs", 202, "RunSummary", submission)
            request("POST", "/api/v1/runs", 422, "Problem", {})
            request("GET", "/api/v1/runs/absent", 404, "Problem")
            request("DELETE", "/api/v1/runs", 200, "ClearResult", {})
            request("GET", summary["receipt_url"], 410, "Problem")
        finally:
            if server.poll() is None:
                server.terminate()
                try:
                    server.wait(timeout=6)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait(timeout=2)
                    raise
        assert server.returncode == 143, "server SIGTERM cleanup/exit contract failed"
    print(f"OpenAPI live response contract: {checked} validations passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
