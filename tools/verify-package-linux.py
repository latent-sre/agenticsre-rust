#!/usr/bin/env python3
"""Installed development-bundle acceptance; private test sandbox only.

Supply real current/prior accepted build archives and separately observed digests.
The driver uses only the standard library and the existing bounded raw MCP client.
It creates and removes a disposable installation; it never changes host installation.
"""
from __future__ import annotations

import argparse
import hashlib
import http.client
import importlib.util
import json
import math
import os
from pathlib import Path
import re
import select
import subprocess
import sys
import tempfile
import time
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parents[1]
PACKAGER = Path(__file__).with_name("package-linux.py")
MAX_RESULT = 2 * 1024 * 1024
EXPORT_RESULTS = False


def checksum(path: Path) -> str:
    result = hashlib.sha256()
    with path.open("rb") as stream:
        while block := stream.read(1024 * 1024):
            result.update(block)
    return result.hexdigest()


def export_receipt(label: str, transport: str, value: dict) -> None:
    if EXPORT_RESULTS:
        encoded = json.dumps({"label": label, "transport": transport, "receipt": value},
                             ensure_ascii=True, allow_nan=False, separators=(",", ":"))
        assert len(encoded.encode("utf-8")) <= MAX_RESULT + 1024, "receipt export exceeds bound"
        print("PACKAGE_RESULT " + encoded, flush=True)


def package(*arguments, refused: str | None = None) -> dict:
    result = subprocess.run([sys.executable, "-B", str(PACKAGER), *map(str, arguments)],
                            capture_output=True, timeout=45, check=False)
    assert result.returncode == (2 if refused is not None else 0), (
        "packager exit", result.returncode, result.stderr[:1000])
    assert len(result.stdout) <= 4 * 1024 * 1024 and len(result.stderr) <= 16 * 1024
    response = json.loads(result.stderr if refused is not None else result.stdout)
    if refused is not None:
        assert response["status"] == "refused" and refused in response["reason"], response
    return response


def product(binary: Path, cwd: Path, environment: dict, *arguments, expected: int = 0,
            receipt: bool = True, label: str = "current") -> dict | bytes:
    result = subprocess.run([str(binary), *map(str, arguments)], cwd=cwd, env=environment,
                            capture_output=True, timeout=12, check=False)
    assert result.returncode == expected, ("product exit", arguments[:3], result.returncode, result.stderr[:500])
    assert len(result.stdout) <= MAX_RESULT and len(result.stderr) <= 16 * 1024
    assert b"synthetic-package-secret" not in result.stdout + result.stderr
    if not receipt:
        assert result.stdout
        return result.stdout
    value = json.loads(result.stdout)
    assert value["assessment"] == "not_assessed" and value["record_mode"] == "never"
    export_receipt(label, "cli", value)
    return value


def basics(binary: Path, cwd: Path, environment: dict, missing_python: bool = False,
           label: str = "current") -> None:
    def run(*arguments, **options):
        return product(binary, cwd, environment, *arguments, label=label, **options)

    run("--help", receipt=False)
    run("--version", receipt=False)
    run("--json", "doctor")
    literal = "literal spaces ' café ; $(touch forbidden-shell-marker)"
    receipt = run("--json", "exec", "--cwd", cwd, "--", "/usr/bin/printf", "%s\n", literal)
    assert receipt["execution"]["status"] == "succeeded"
    assert receipt["output"]["stdout"] == literal + "\n"
    assert receipt["execution"]["child_exit_code"] == 0
    assert not (cwd / "forbidden-shell-marker").exists()
    listed = run("--json", "task", "list")
    assert {item["task"]["id"] for item in listed["data"]["tasks"]} == {"dashboard-hygiene", "error-budget"}
    for task in ("dashboard-hygiene", "error-budget"):
        value = run("--json", "task", "run", task,
                    "--input", cwd / f"{task}-input.json", expected=2 if missing_python else 0)
        if missing_python:
            assert value["effect_outcome"] == "not_attempted"
            assert any(error["code"] == "missing_dependency" for error in value["errors"])
            advertised = next(item for item in listed["data"]["tasks"] if item["task"]["id"] == task)
            assert advertised["availability"]["status"] == "missing_dependency"
        else:
            assert value["execution"]["status"] == "succeeded"
            if task == "dashboard-hygiene":
                assert value["data"]["findings"] == [] and value["data"]["checked_panels"] == 1
            else:
                assert value["data"]["calculation"]["status"]["state"] == "exhausted"
                assert math.isclose(value["data"]["calculation"]["status"]["budget"], 40.32, abs_tol=1e-10)
    assert not (cwd / "workspace-code-ran").exists()


def mcp(binary: Path, cwd: Path, environment: dict) -> None:
    specification = importlib.util.spec_from_file_location("package_raw_mcp", ROOT / "tools/verify-mcp.py")
    assert specification is not None and specification.loader is not None
    module = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(module)
    previous_cwd = Path.cwd()
    try:
        os.chdir(cwd)
        with module.Client(binary, ("mcp", "serve", "--allow", "task.run"), env=environment) as client:
            tools = module.successful(client.request("tools/list"))["tools"]
            assert [item["name"] for item in tools] == ["workbench_task_run"]
            result = module.successful(client.request("tools/call", module.task_arguments(
                {"slo": 99.9, "bad_minutes": 40.32})))
            assert result["isError"] is False
            receipt = result["structuredContent"]
            export_receipt("current", "mcp", receipt)
            assert receipt["execution"]["status"] == "succeeded" and receipt["record_mode"] == "never"
            assert receipt["data"]["calculation"]["status"]["state"] == "exhausted"
            client.close_input()
            assert client.wait() == 0, "installed MCP did not stop cleanly at EOF"
            assert len(client.diagnostics) <= 16384 and b"synthetic-package-secret" not in client.diagnostics
            process = client.process
        assert process.poll() == 0, "installed MCP process remains"
    finally:
        os.chdir(previous_cwd)


def ui(binary: Path, cwd: Path, environment: dict) -> None:
    with subprocess.Popen([str(binary), "ui", "serve", "--allow", "task.run"], cwd=cwd, env=environment,
                          stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, bufsize=0) as server:
        try:
            assert server.stdout is not None
            os.set_blocking(server.stdout.fileno(), False)
            deadline = time.monotonic() + 5
            line = bytearray()
            while b"\n" not in line:
                remaining = deadline - time.monotonic()
                assert remaining > 0, "installed UI launch deadline"
                ready, _, _ = select.select([server.stdout], [], [], remaining)
                assert ready, "installed UI launch deadline"
                block = os.read(server.stdout.fileno(), 4096)
                assert block, "installed UI exited before launch"
                line.extend(block)
                assert len(line) <= 4096, "installed UI launch exceeds bound"
            launch = urlsplit(line.split(b"\n", 1)[0].decode("utf-8"))
            assert launch.hostname == "127.0.0.1" and launch.fragment.startswith("token=")
            token = launch.fragment.removeprefix("token=")
            origin = f"http://{launch.netloc}"

            def request(method: str, path: str, status: int, value: object = None,
                        *, authenticated: bool = True, data: bool = True):
                connection = http.client.HTTPConnection(launch.hostname, launch.port, timeout=3)
                try:
                    headers = {"Origin": origin, "Content-Type": "application/json"}
                    if authenticated:
                        headers["Authorization"] = f"Bearer {token}"
                    connection.request(method, path, body=None if value is None else json.dumps(value), headers=headers)
                    response = connection.getresponse()
                    payload = response.read(MAX_RESULT + 1)
                    assert len(payload) <= MAX_RESULT and response.status == status, (method, path, response.status)
                    return json.loads(payload) if data else payload
                finally:
                    connection.close()

            page = request("GET", "/", 200, authenticated=False, data=False)
            assert b"<div id=\"root\"></div>" in page
            assets = re.findall(rb'(?:src|href)="(/assets/[0-9A-Za-z_.-]+)"', page)
            assert assets, "installed UI did not advertise embedded assets"
            for asset in assets:
                assert request("GET", asset.decode("ascii"), 200, authenticated=False, data=False)
            request("GET", "/bootstrap.js", 200, authenticated=False, data=False)
            request("GET", "/api/v1/session", 401, authenticated=False)
            request("GET", "/api/v1/session", 200)
            submission = {"submission_id": "01234567-1234-4234-8234-0123456789ab",
                          "operation": "task.run", "operation_version": 1,
                          "inputs": {"id": "error-budget", "version": 1,
                                     "input": {"slo": 99.9, "bad_minutes": 40.32}},
                          "limits": {"timeout_ms": 30000, "max_output_bytes": 1048576}}
            admitted = request("POST", "/api/v1/runs", 202, submission)
            path = f"/api/v1/runs/{admitted['id']}"
            deadline = time.monotonic() + 7
            while True:
                summary = request("GET", path, 200)
                if summary["state"] == "terminal":
                    break
                assert time.monotonic() < deadline, "installed UI receipt deadline"
                time.sleep(0.01)
            receipt = request("GET", summary["receipt_url"], 200)
            export_receipt("current", "ui", receipt)
            assert receipt["execution"]["status"] == "succeeded" and receipt["record_mode"] == "never"
            assert receipt["data"]["calculation"]["status"]["state"] == "exhausted"
        finally:
            if server.poll() is None:
                server.terminate()
                try:
                    server.wait(timeout=6)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait(timeout=2)
                    raise AssertionError("installed UI exceeded cleanup deadline") from None
        assert server.returncode == 143, "installed UI SIGTERM cleanup contract"
    assert server.poll() == 143, "installed UI process remains"


def main() -> int:
    global EXPORT_RESULTS
    assert __debug__, "verification requires assertions enabled"
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("bundle", type=Path)
    parser.add_argument("--expected-sha256", required=True)
    parser.add_argument("--previous-bundle", type=Path)
    parser.add_argument("--previous-expected-sha256")
    parser.add_argument("--json-results", action="store_true",
                        help="export each exact decoded product receipt as a bounded PACKAGE_RESULT JSON line")
    parser.add_argument("--missing-python-only", action="store_true",
                        help="only installed offline behavior and missing-runtime refusals; not full acceptance")
    options = parser.parse_args()
    EXPORT_RESULTS = options.json_results
    if not options.missing_python_only and (
            options.previous_bundle is None or options.previous_expected_sha256 is None):
        parser.error("full acceptance needs --previous-bundle and --previous-expected-sha256")
    bundle = options.bundle.resolve(strict=True)
    current = package("verify", bundle, "--expected-sha256", options.expected_sha256)
    prior = None
    if not options.missing_python_only:
        previous_bundle = options.previous_bundle.resolve(strict=True)
        prior = package("verify", previous_bundle, "--expected-sha256", options.previous_expected_sha256)
        assert prior["manifest"]["source"]["sha256"] != current["manifest"]["source"]["sha256"], "need distinct real sources"
        assert prior["manifest"]["binary"]["sha256"] != current["manifest"]["binary"]["sha256"], "need distinct real binaries"
    count = 0

    def passed(message: str) -> None:
        nonlocal count
        count += 1
        print(f"PASS {count}: {message}", flush=True)

    with tempfile.TemporaryDirectory(prefix="workbench-installed-bundle-") as temporary:
        scratch = Path(temporary)
        install_root = scratch / "install"
        hostile = scratch / "hostile-cwd"
        hostile.mkdir()
        marker = hostile / "workspace-code-ran"
        poison = f"from pathlib import Path\nPath({str(marker)!r}).write_text('bad')\nraise RuntimeError('workspace code ran')\n"
        for filename in ("json.py", "decimal.py", "sitecustomize.py", "error_budget.py", "dashboard_hygiene.py"):
            (hostile / filename).write_text(poison, encoding="utf-8")
        model = hostile / "dashboard.json"
        model.write_bytes((ROOT / "tests/fixtures/dashboard-hygiene/clean.json").read_bytes())
        (hostile / "dashboard-hygiene-input.json").write_text(json.dumps({"file": str(model)}), encoding="utf-8")
        (hostile / "error-budget-input.json").write_text('{"slo":99.9,"bad_minutes":40.32}\n', encoding="utf-8")
        canaries = {scratch / "external-config.json": b'{"synthetic":"external config"}\n',
                    scratch / "external-evidence.json": b'{"synthetic":"external evidence"}\n',
                    model: model.read_bytes(),
                    hostile / "dashboard-hygiene-input.json": (hostile / "dashboard-hygiene-input.json").read_bytes(),
                    hostile / "error-budget-input.json": (hostile / "error-budget-input.json").read_bytes()}
        for path, data in canaries.items():
            path.write_bytes(data)
        environment = {"PATH": str(hostile), "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8", "TZ": "UTC",
                       "PYTHONPATH": str(hostile), "PYTHONSTARTUP": str(hostile / "sitecustomize.py"),
                       "PACKAGE_SECRET": "synthetic-package-secret"}
        corrupt = scratch / "corrupt.tar"
        # A real changed archive with the original observed digest must be refused.
        with bundle.open("rb") as source, corrupt.open("wb") as target:
            first = source.read(1)
            target.write(bytes([first[0] ^ 1]))
            while block := source.read(1024 * 1024):
                target.write(block)
        package("install", corrupt, "--expected-sha256", options.expected_sha256, "--root", install_root,
                refused="archive SHA-256 mismatch")
        assert not install_root.exists()
        passed("corruption refused before installation state")

        if prior is not None:
            installed_prior = package("install", previous_bundle, "--expected-sha256",
                                      options.previous_expected_sha256, "--root", install_root)
            prior_binary = Path(installed_prior["binary"])
            prior_hash = checksum(prior_binary)
            assert prior_hash == prior["manifest"]["binary"]["sha256"]
            package("activate", prior["artifact_id"], "--root", install_root)
            basics(install_root / "current/bin/save", hostile, environment, label="prior")
            passed("prior real artifact installed and mission/tasks executed away from checkout")
        installed_current = package("install", bundle, "--expected-sha256", options.expected_sha256, "--root", install_root)
        current_binary = Path(installed_current["binary"])
        assert checksum(current_binary) == current["manifest"]["binary"]["sha256"]
        assert current_binary.stat().st_mode & 0o7777 == 0o755
        repeated = package("install", bundle, "--expected-sha256", options.expected_sha256, "--root", install_root)
        assert repeated["status"] == "already_present"
        if prior is not None:
            assert os.readlink(install_root / "current") == "releases/" + prior["artifact_id"]
        else:
            assert not (install_root / "current").is_symlink()
        package("activate", current["artifact_id"], "--root", install_root)
        assert os.readlink(install_root / "current") == "releases/" + current["artifact_id"]
        basics(install_root / "current/bin/save", hostile, environment, options.missing_python_only)
        passed("fresh/repeated install, explicit selection, modes, digest and installed offline mission/tasks")
        if prior is not None:
            mcp(install_root / "current/bin/save", hostile, environment)
            passed("installed numerical stdio MCP receipt with clean EOF shutdown")
            ui(install_root / "current/bin/save", hostile, environment)
            passed("embedded UI assets and authenticated numerical API receipt with SIGTERM cleanup")
            package("rollback", prior["artifact_id"], "--root", install_root)
            assert os.readlink(install_root / "current") == "releases/" + prior["artifact_id"]
            assert checksum(prior_binary) == prior_hash
            basics(install_root / "current/bin/save", hostile, environment, label="rollback")
            assert checksum(current_binary) == current["manifest"]["binary"]["sha256"]
            assert len(list((install_root / "releases").iterdir())) == 2
            passed("rollback selects retained prior bytes and repeats installed mission/tasks")
        for path, original in canaries.items():
            assert path.read_bytes() == original, "external/config/model/input canary changed"
        assert not marker.exists() and not list(install_root.glob(".current-*"))
        assert not list((install_root / "releases").glob(".staging-*"))
        passed("external canaries unchanged; hostile workspace code absent; no partial install/selection remains")
        scratch_path = scratch
    assert not scratch_path.exists(), "disposable runtime/installation was not removed"
    passed("disposable runtime and installation cleanup confirmed")
    print(json.dumps({"scope": "missing-python-only" if options.missing_python_only else "Linux development bundle",
                      "current_artifact_id": current["artifact_id"], "archive_sha256": options.expected_sha256,
                      "previous_artifact_id": None if prior is None else prior["artifact_id"], "checks": count,
                      "previous_archive_sha256": None if prior is None else options.previous_expected_sha256,
                      "missing_python_checked": options.missing_python_only}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
