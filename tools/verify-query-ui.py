"""Installed saved-query browser acceptance inside tools/test-ui-sandbox.sh."""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import selectors
import shutil
import signal
import subprocess
import tempfile
import threading
import time

from jsonschema import Draft202012Validator, FormatChecker
from referencing import Registry, Resource

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    options = parser.parse_args()
    binary = options.binary.resolve(strict=True)
    output = Path(os.environ["WORKBENCH_UI_OUTPUT"]) / "queries"
    output.mkdir(parents=True, exist_ok=True)
    schemas = [json.loads(path.read_text()) for path in (ROOT / "docs/sre-workbench/schemas").glob("*.schema.json")]
    registry = Registry().with_resources((schema["$id"], Resource.from_contents(schema)) for schema in schemas)
    result_schema = json.loads((ROOT / "docs/sre-workbench/schemas/result.schema.json").read_text())
    query_schema = json.loads((ROOT / "docs/sre-workbench/schemas/query-run-data.schema.json").read_text())
    result_validator = Draft202012Validator(result_schema, registry=registry, format_checker=FormatChecker())
    query_validator = Draft202012Validator(query_schema, registry=registry, format_checker=FormatChecker())
    source_paths = [*sorted((ROOT / "web/src").rglob("*")), *sorted((ROOT / "web/dist").rglob("*")),
                    ROOT / "web/tests/query-browser.mjs", ROOT / "tools/verify-query-ui.py", ROOT / "docs/ui/openapi.json"]
    sources = {str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest() for path in source_paths if path.is_file()}
    spec = importlib.util.spec_from_file_location("query_ui_grafana_fixture", ROOT / "tools/verify-grafana.py")
    fixture_module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(fixture_module)
    baseline = set(threading.enumerate())
    with tempfile.TemporaryDirectory(prefix="workbench-installed-query-ui-") as directory:
        scratch = Path(directory)
        installed = scratch / "bin/save"
        installed.parent.mkdir()
        shutil.copy2(binary, installed)
        installed.chmod(0o755)
        working = scratch / "unrelated"
        working.mkdir()
        cert = fixture_module.certificates(scratch)
        fixture = fixture_module.Fixture(cert["server.pem"], cert["server.key"])
        configuration = scratch / "config.json"
        configuration.write_text(json.dumps({"spec_version": "0.1", "record": "never", "connections": {"fixture": {
            "origin": fixture.origin, "expected_org_id": 7, "api_profile": "grafana-legacy-v1",
            "credential_ref": "env:" + fixture_module.PROVIDER, "tls_ca_file": str(cert["ca.pem"]),
        }}}))
        environment = {"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8", "TZ": "UTC", fixture_module.PROVIDER: json.dumps({
            "origin": fixture.origin, "expected_org_id": 7, "operations": ["grafana.query"],
            "auth": {"kind": "bearer", "token": fixture_module.TOKEN},
        })}
        server = subprocess.Popen([str(installed), "--config", str(configuration), "ui", "serve",
                                   "--allow", "queries.list", "--allow", "queries.describe", "--allow", "query.run",
                                   "--query-target", "fixture"], cwd=working, env=environment,
                                  stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, start_new_session=True)
        driver = None
        records = []
        try:
            with selectors.DefaultSelector() as selector:
                selector.register(server.stdout, selectors.EVENT_READ)
                assert selector.select(12), "UI launch timed out"
                launch = server.stdout.readline().strip()
            assert re.fullmatch(r"http://127\.0\.0\.1:\d+/#token=[A-Za-z0-9_-]{43}", launch), "UI did not produce its launch link"
            driver = subprocess.Popen(["node", str(ROOT / "web/tests/query-browser.mjs")], cwd=working,
                                      stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            driver.stdin.write(json.dumps({"launch": launch, "output": str(output)}) + "\n")
            driver.stdin.flush()
            deadline = time.monotonic() + 90
            with selectors.DefaultSelector() as selector:
                selector.register(driver.stdout, selectors.EVENT_READ)
                while driver.poll() is None:
                    assert time.monotonic() < deadline, "Query browser acceptance timed out"
                    if not selector.select(1):
                        continue
                    line = driver.stdout.readline()
                    if not line:
                        break
                    request = json.loads(line)
                    action = request["action"]
                    if action == "fixture":
                        fixture.reset(kind=request["kind"])
                        if request.get("hostile"):
                            response = json.loads((fixture_module.FIXTURES / "loki.json").read_text())
                            response["results"]["A"]["frames"][0]["data"]["values"][1][0] = '<img src="https://untrusted.invalid/pixel" onerror="window.__query_output_executed=1">'
                            fixture.overrides["query"] = (200, response, {})
                        if request.get("delay"):
                            fixture.delays["query"] = 2
                        result = {}
                    elif action == "ledger":
                        result = [{"method": event["method"], "path": event["path"], "body": json.loads(event["body"]) if event["body"] else None} for event in fixture.events]
                    elif action == "record":
                        receipt = request["receipt"]
                        result_validator.validate(receipt)
                        if receipt["operation"] == "query.run" and "definition" in receipt["data"]:
                            query_validator.validate(receipt["data"])
                        assert receipt["assessment"] == "not_assessed" and receipt["record_mode"] == "never"
                        assert fixture_module.TOKEN not in json.dumps(receipt)
                        records.append({"name": request["name"], "receipt": receipt})
                        result = {}
                    elif action == "done":
                        result = {}
                    else:
                        raise AssertionError("Unknown browser fixture action")
                    driver.stdin.write(json.dumps(result) + "\n")
                    driver.stdin.flush()
                    if action == "done":
                        driver.stdin.close()
                        break
            status = driver.wait(timeout=5)
            error = driver.stderr.read()
            assert status == 0, re.sub(r"#token=[A-Za-z0-9_-]+", "#token=[redacted]", error)
        finally:
            if driver and driver.poll() is None:
                driver.kill()
                driver.wait(timeout=5)
            if server.poll() is None:
                os.killpg(server.pid, signal.SIGTERM)
                server.wait(timeout=8)
            fixture.close()
            # Join any delayed synthetic HTTP request after client cancellation.
            for thread in set(threading.enumerate()) - baseline:
                thread.join(timeout=3)
                assert not thread.is_alive(), "Fixture thread did not terminate"
        assert len(records) == 6, "Missing installed browser receipts"
        assert sources == {str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest() for path in source_paths if path.is_file()}, "Browser source bytes changed during verification"
        result = {"status": "passed", "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                  "installed_copy_sha256": hashlib.sha256(installed.read_bytes()).hexdigest(),
                  "sources_sha256": sources, "records": records,
                  "validation": "All canonical result schemas and query-data schemas for returned definition data passed",
                  "cleanup": "UI process, browser driver and fixture threads joined"}
        (output / "browser-query-results.json").write_text(json.dumps(result, indent=2) + "\n")
        print(json.dumps({"status": "passed", "receipts": len(records), "evidence": str(output / "browser-query-results.json")}))


if __name__ == "__main__":
    main()
