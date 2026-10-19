"""Installed evidence collection browser acceptance inside tools/test-ui-sandbox.sh."""
from __future__ import annotations

from datetime import datetime, timezone
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
    assert os.environ.get("CARGO_HOME") == "/tmp/cargo" and Path("/toolchain").is_dir(), "Use test-ui-sandbox.sh"
    binary = options.binary.resolve(strict=True)
    output = Path(os.environ["WORKBENCH_UI_OUTPUT"]) / "collection"
    output.mkdir(parents=True, exist_ok=True)
    schemas = [json.loads(path.read_text()) for path in (ROOT / "docs/sre-workbench/schemas").glob("*.schema.json")]
    registry = Registry().with_resources((schema["$id"], Resource.from_contents(schema)) for schema in schemas)
    result_schema = json.loads((ROOT / "docs/sre-workbench/schemas/result.schema.json").read_text())
    query_schema = json.loads((ROOT / "docs/sre-workbench/schemas/evidence-collection-data.schema.json").read_text())
    result_validator = Draft202012Validator(result_schema, registry=registry, format_checker=FormatChecker())
    query_validator = Draft202012Validator(query_schema, registry=registry, format_checker=FormatChecker())
    specification = json.loads((ROOT / "docs/ui/openapi.json").read_bytes())
    contract_count = 0
    source_paths = [*sorted((ROOT / "web/src").rglob("*")), *sorted((ROOT / "web/dist").rglob("*")),
                    ROOT / "web/tests/collection-browser.mjs", ROOT / "tools/verify-evidence-ui.py", ROOT / "docs/ui/openapi.json", ROOT / "tools/verify-evidence.py", ROOT / "tools/verify-grafana.py", *sorted((ROOT / "crates").rglob("*.rs")), *sorted((ROOT / "docs/sre-workbench/schemas").glob("*.json"))]
    sources = {str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest() for path in source_paths if path.is_file()}
    spec = importlib.util.spec_from_file_location("collection_grafana_fixture", ROOT / "tools/verify-grafana.py")
    fixture_module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(fixture_module)
    spec = importlib.util.spec_from_file_location("collection_evidence_fixture", ROOT / "tools/verify-evidence.py")
    evidence = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(evidence)
    baseline = set(threading.enumerate())
    with tempfile.TemporaryDirectory(prefix="workbench-installed-collection-ui-") as directory:
        scratch = Path(directory)
        installed = scratch / "bin/save"
        installed.parent.mkdir()
        shutil.copy2(binary, installed)
        installed.chmod(0o755)
        working = scratch / "unrelated"
        working.mkdir()
        cert = fixture_module.certificates(scratch)
        fixture = evidence.Fixture(cert["server.pem"], cert["server.key"])
        configuration = scratch / "config.json"
        configuration.write_text(json.dumps({"spec_version": "0.1", "record": "never", "connections": {"fixture": {
            "origin": fixture.origin, "expected_org_id": 7, "api_profile": "grafana-legacy-v1",
            "credential_ref": "env:" + evidence.PROVIDER, "tls_ca_file": str(cert["ca.pem"]),
        }}}))
        environment = {"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8", "TZ": "UTC", evidence.PROVIDER: json.dumps({
            "origin": fixture.origin, "expected_org_id": 7, "operations": ["grafana.query"],
            "auth": {"kind": "bearer", "token": evidence.TOKEN},
        })}
        store, source = scratch / "store", scratch / "context.json"
        store.mkdir(mode=0o700); (store / "runs").mkdir(mode=0o700)
        for name, raw in (("format.json", '{"format_version":1}\n'), ("lock", "")):
            path = store / name; path.write_text(raw); path.chmod(0o600)
        context = json.loads((ROOT / "tests/fixtures/context/unique.json").read_bytes())
        context["records"][0].update(service="fixture-web", last_reviewed=datetime.now(timezone.utc).isoformat(),
            bindings={"target":"fixture","environment":"dev","organization":"7"})
        source.write_text(json.dumps(context))
        server = subprocess.Popen([str(installed), "--config", str(configuration), "--context-source", str(source),
            "--context-max-age-days", "30", "--state-dir", str(store), "ui", "serve",
            "--allow", "evidence.packs.list", "--allow", "evidence.packs.describe", "--allow", "evidence.collect",
            "--allow", "evidence.open", "--collection-target", "fixture"], cwd=working, env=environment,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, start_new_session=True)
        driver = None
        records = []
        try:
            with selectors.DefaultSelector() as selector:
                selector.register(server.stdout, selectors.EVENT_READ)
                assert selector.select(12), "UI launch timed out"
                launch = server.stdout.readline().strip()
            assert re.fullmatch(r"http://127\.0\.0\.1:\d+/#token=[A-Za-z0-9_-]{43}", launch), "UI did not produce its launch link"
            driver = subprocess.Popen(["node", str(ROOT / "web/tests/collection-browser.mjs")], cwd=working,
                                      stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            driver.stdin.write(json.dumps({"launch": launch, "output": str(output)}) + "\n")
            driver.stdin.flush()
            deadline = time.monotonic() + 90
            with selectors.DefaultSelector() as selector:
                selector.register(driver.stdout, selectors.EVENT_READ)
                while driver.poll() is None:
                    assert time.monotonic() < deadline, "Collection browser acceptance timed out"
                    if not selector.select(1):
                        continue
                    line = driver.stdout.readline()
                    if not line:
                        break
                    request = json.loads(line)
                    action = request["action"]
                    if action == "contract":
                        schema = {"$ref": "#/components/schemas/" + request["schema"], "components": specification["components"]}
                        Draft202012Validator(schema, registry=registry, format_checker=FormatChecker()).validate(request["value"])
                        contract_count += 1
                        result = {}
                    elif action == "fixture":
                        fixture.fail_kind = request.get("fail_kind")
                        fixture.block_event = len(fixture.events) + 3 if request.get("block") else None
                        fixture.release.clear()
                        result = {}
                    elif action == "ledger":
                        result = [{"method": event["method"], "path": event["path"], "body": json.loads(event["body"]) if event["body"] else None} for event in fixture.events]
                    elif action == "record":
                        receipt = request["receipt"]
                        result_validator.validate(receipt)
                        if receipt["operation"] == "evidence.collect" and "pack" in receipt["data"]:
                            query_validator.validate(receipt["data"])
                        assert receipt["assessment"] == "not_assessed"
                        assert receipt["record_mode"] == ("always" if receipt["operation"] == "evidence.collect" else "never")
                        ref = receipt["data"].get("bundle") if receipt["operation"] == "evidence.collect" else None
                        if ref:
                            manifest_bytes = (store / "runs" / ref["id"] / "manifest.json").read_bytes()
                            assert "sha256:" + hashlib.sha256(manifest_bytes).hexdigest() == ref["digest"]
                            for artifact in json.loads(manifest_bytes)["artifacts"].values():
                                raw = (store / "runs" / ref["id"] / artifact["path"]).read_bytes()
                                assert "sha256:" + hashlib.sha256(raw).hexdigest() == artifact["digest"]
                                evidence.assert_no_canary(raw)
                                result_validator.validate(json.loads(raw))
                        assert evidence.TOKEN not in json.dumps(receipt)
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
                  "sources_sha256": sources, "records": records, "http_contract_validations": contract_count,
                  "validation": "Canonical receipts, collection data, sealed manifest and artifact digests validated",
                  "cleanup": "UI process, browser driver and fixture threads joined"}
        (output / "browser-collection-results.json").write_text(json.dumps(result, indent=2) + "\n")
        print(json.dumps({"status": "passed", "receipts": len(records), "evidence": str(output / "browser-collection-results.json")}))


if __name__ == "__main__":
    main()
