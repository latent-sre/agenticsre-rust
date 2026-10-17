"""Verify an installed saved-query binary and strict receipts inside tools/test-sandbox.sh."""

from __future__ import annotations

import argparse
import copy
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import threading
import time

from jsonschema import Draft202012Validator, FormatChecker
from referencing import Registry, Resource


ROOT = Path(__file__).resolve().parents[1]
MAX_RESULT_BYTES = 2 * 1024 * 1024
PROMETHEUS = "prometheus.up-by-job"
LOKI = "loki.logs-by-job"
WINDOW = {
    "kind": "absolute",
    "from": "2026-10-02T05:30:00.1230000000000+05:30",
    "to": "2026-10-02T00:01:00.123Z",
}
NORMALIZED_WINDOW = {
    "from": "2026-10-02T00:00:00.123Z",
    "to": "2026-10-02T00:01:00.123Z",
}


def load_fixture_module():
    spec = importlib.util.spec_from_file_location("query_grafana_fixture", ROOT / "tools/verify-grafana.py")
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def stable_receipt(result: dict) -> dict:
    result = copy.deepcopy(result)
    for field in ("request_id", "run_id", "started_at", "finished_at", "duration_ms"):
        del result[field]
    for source in result["sources"]:
        source.pop("observed_at", None)
    return result


def epoch_milliseconds(timestamp: str) -> int:
    value = datetime.fromisoformat(timestamp)
    epoch = datetime(1970, 1, 1, tzinfo=timezone.utc)
    duration = value - epoch
    return (duration.days * 86400 + duration.seconds) * 1000 + duration.microseconds // 1000


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--evidence-dir", type=Path)
    options = parser.parse_args()
    binary = options.binary.resolve(strict=True)
    source_hash = hashlib.sha256(binary.read_bytes()).hexdigest()
    schemas = {
        path.stem.removesuffix(".schema"): json.loads(path.read_text())
        for path in (ROOT / "docs/sre-workbench/schemas").glob("*.schema.json")
    }
    registry = Registry().with_resources(
        (schema["$id"], Resource.from_contents(schema)) for schema in schemas.values()
    )
    validators = {
        name: Draft202012Validator(schemas[name], registry=registry, format_checker=FormatChecker())
        for name in ("result", "request", "query-run-input", "query-run-data")
    }
    fixtures = load_fixture_module()
    received = []
    raw_receipts = []
    thread_baseline = set(threading.enumerate())
    query_count = 0

    with tempfile.TemporaryDirectory(prefix="workbench-installed-queries-") as directory:
        scratch = Path(directory)
        installed = scratch / "prefix/bin/save"
        installed.parent.mkdir(parents=True)
        shutil.copy2(binary, installed)
        installed.chmod(0o755)
        working = scratch / "unrelated-working-directory"
        working.mkdir()
        certificates = fixtures.certificates(scratch)
        fixture = fixtures.Fixture(certificates["server.pem"], certificates["server.key"])
        config_file = scratch / "operator.json"
        request_file = scratch / "request.json"
        parameters_file = scratch / "parameters.json"
        window_file = scratch / "window.json"
        provider = {
            "origin": fixture.origin,
            "expected_org_id": 7,
            "operations": ["grafana.query"],
            "auth": {"kind": "bearer", "token": fixtures.TOKEN},
        }
        config_file.write_text(json.dumps({
            "spec_version": "0.1", "record": "never", "connections": {"fixture": {
                "origin": fixture.origin, "expected_org_id": 7, "api_profile": "grafana-legacy-v1",
                "credential_ref": "env:" + fixtures.PROVIDER, "tls_ca_file": str(certificates["ca.pem"]),
            }},
        }))

        def receipt(raw: bytes) -> dict:
            nonlocal query_count
            assert len(raw) <= MAX_RESULT_BYTES
            result = json.loads(raw)
            fixtures.assert_finite(result)
            validators["result"].validate(result)
            assert result["assessment"] == "not_assessed" and result["record_mode"] == "never"
            if result["operation"] == "query.run":
                validators["query-run-data"].validate(result["data"])
                query_count += 1
                assert result["execution"]["child_exit_code"] is None
            raw_receipts.append(raw)
            return result

        def run(*arguments: str, expected: int = 0, discovery: bool = False) -> dict:
            environment = {"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8", "TZ": "UTC"}
            if not discovery:
                environment[fixtures.PROVIDER] = json.dumps(provider)
            completed = subprocess.run(
                [str(installed), "--json", "--config", str(scratch / "absent.json" if discovery else config_file), *arguments],
                cwd=working, env=environment, capture_output=True, timeout=8, check=False,
            )
            assert completed.returncode == expected, (arguments[:3], completed.returncode, completed.stdout[:2000], completed.stderr[:500])
            assert not completed.stderr
            return receipt(completed.stdout)

        def query(id: str = PROMETHEUS, *, job: str = "fixture", window: dict = WINDOW,
                  expected: int = 0, extra: tuple[str, ...] = ()) -> dict:
            parameters_file.write_text(json.dumps({"job": job}))
            window_file.write_text(json.dumps(window))
            return run("queries", "run", id, "--version", "1", "--target", "fixture",
                       "--datasource", "metrics-1", "--parameters", str(parameters_file),
                       "--window", str(window_file), "--timeout", "60s", *extra, expected=expected)

        def supplied(id: str = PROMETHEUS, job: str = "fixture") -> dict:
            return {"id": id, "version": 1, "datasource": "metrics-1", "parameters": {"job": job}, "window": copy.deepcopy(WINDOW)}

        def call(inputs: dict, *, expected: int = 0) -> dict:
            request = {
                "spec_version": "0.1", "request_id": "installed-query-fixture", "operation": "query.run",
                "operation_version": 1, "target": {"kind": "connection", "id": "fixture"},
                "inputs": inputs, "limits": {"timeout_ms": 60000, "max_output_bytes": MAX_RESULT_BYTES},
                "record": "never",
            }
            validators["request"].validate(request)
            if expected == 0:
                validators["query-run-input"].validate(inputs)
            request_file.write_text(json.dumps(request))
            return run("call", "--request", str(request_file), expected=expected)

        def retain_ledger(case: str) -> None:
            received.append({"case": case, "requests": [
                {"method": event["method"], "path": event["path"], "body": json.loads(event["body"]) if event["body"] else None}
                for event in fixture.events
            ]})

        try:
            listed = run("queries", "list", discovery=True)
            catalog = {entry["id"]: entry for entry in listed["data"]["queries"]}
            assert set(catalog) == {PROMETHEUS, LOKI}
            for id in (PROMETHEUS, LOKI):
                described = run("queries", "describe", id, "--version", "1", discovery=True)
                assert described["data"]["query"] == catalog[id]
                definition = (ROOT / "crates/workbench-core/resources/queries" / (id + ".v1.json")).read_bytes()
                assert catalog[id] == dict(json.loads(definition), definition_sha256=hashlib.sha256(definition).hexdigest())
            assert not fixture.events

            job = 'café\\worker"},other="value"} or vector(1) #'
            literal = r'"café\\worker\"},other=\"value\"} or vector(1) #"'
            for id, kind, dialect, metric in ((PROMETHEUS, "prometheus", "promql", "up"), (LOKI, "loki", "logql", "")):
                fixture.reset(kind=kind)
                digest = catalog[id]["definition_sha256"]
                direct = query(id, job=job, extra=("--expected-definition-sha256", digest))
                input_value = supplied(id, job)
                input_value["expected_definition_sha256"] = digest
                called = call(input_value)
                assert stable_receipt(direct) == stable_receipt(called)
                assert direct["data"]["definition"] == {"id": id, "version": 1, "dialect": dialect, "sha256": digest}
                assert direct["data"]["parameters"] == {"job": job}
                assert direct["data"]["window"] == NORMALIZED_WINDOW
                assert direct["coverage"]["state"] == "not_established"
                expression = metric + "{job=" + literal + "}"
                expected_query = {"refId": "A", "datasource": {"uid": "metrics-1", "type": kind},
                                  "expr": expression, "maxDataPoints": 1000, "intervalMs": 1000}
                expected_query.update({"range": True, "instant": False, "format": "time_series"}
                                      if kind == "prometheus" else {"queryType": "range", "maxLines": 500})
                expected_body = {"from": "1790899200123", "to": "1790899260123", "queries": [expected_query]}
                expected_paths = [("GET", "/grafana/api/org"), ("GET", f"/grafana/api/datasources/uid/metrics-1?ds_type={kind}"), ("POST", "/grafana/api/ds/query")]
                assert [(event["method"], event["path"]) for event in fixture.events] == expected_paths * 2
                assert json.loads(fixture.events[2]["body"]) == expected_body
                assert json.loads(fixture.events[5]["body"]) == expected_body
                assert all(event["auth_headers"] == ["Bearer " + fixtures.TOKEN] and event["org_headers"] == ["7"] for event in fixture.events)
                retain_ledger(id + ":direct-and-structured")

            fixture.reset()
            fixture.delays = {"organization": 0.06, "datasource": 0.06}
            relative = query(window={"kind": "relative", "lookback_seconds": 300})
            anchor = epoch_milliseconds(relative["started_at"])
            assert epoch_milliseconds(relative["data"]["window"]["to"]) == anchor
            assert epoch_milliseconds(relative["data"]["window"]["from"]) == anchor - 300000
            body = json.loads(fixture.events[-1]["body"])
            assert body["to"] == str(anchor) and body["from"] == str(anchor - 300000)
            retain_ledger("one-parent-relative-window")

            for payload in ({"results": {"A": {"frames": []}}}, {"results": {"A": {"status": 206, "frames": [{"schema": {"meta": {"notice": "partial fixture result"}}}]}}}):
                fixture.reset()
                fixture.overrides["query"] = (200, payload, {})
                result = query()
                assert result["data"]["response"] == payload and result["coverage"]["state"] == "not_established"

            for token in (fixtures.TOKEN, 'synthetic"token\\secret', "2026-10-02", PROMETHEUS):
                fixture.reset()
                provider["auth"]["token"] = token
                compiled = "up{job=" + json.dumps(token, ensure_ascii=False) + "}"
                fixture.overrides["query"] = (200, {"results": {"A": {"frames": [{"echo": token, "schema": {"meta": {"executedQueryString": compiled}}}]}}}, {})
                result = query(job=token)
                assert result["data"]["definition"]["id"] == PROMETHEUS
                assert result["data"]["parameters"] == {"job": {"redacted": True}}
                assert result["data"]["request"]["expr"] == {"redacted": True}
                frame = result["data"]["response"]["results"]["A"]["frames"][0]
                assert frame["echo"] == "[REDACTED]"
                echoed = frame["schema"]["meta"]["executedQueryString"]
                assert "[REDACTED]" in echoed and json.dumps(token)[1:-1] not in echoed
                if token == "2026-10-02":
                    assert result["data"]["window"] == {"from": {"redacted": True}, "to": {"redacted": True}}
            provider["auth"]["token"] = fixtures.TOKEN

            for patch in ({"expected_definition_sha256": "0" * 64}, {"expected_definition_sha256": None},
                          {"parameters": {"job": {"type": "string"}}}, {"parameters": {"job": "$job"}},
                          {"parameters": {"job": fixtures.TOKEN, "unknown": fixtures.TOKEN}},
                          {"window": {"kind": "relative", "lookback_seconds": 86401}}, {"expression": "up"}):
                fixture.reset()
                result = call(dict(supplied(), **patch), expected=2)
                assert result["effect_outcome"] == "not_attempted" and not fixture.events
                assert fixtures.TOKEN not in json.dumps(result)

            for stage, payload, count in (("datasource", {"uid": "other", "type": "prometheus", "orgId": 7}, 2),
                                          ("query", {"results": {"A": {"error": fixtures.TOKEN, "frames": []}}}, 3)):
                fixture.reset()
                fixture.overrides[stage] = (200, payload, {})
                result = query(expected=1)
                assert result["data"]["response"] is None and result["data"]["definition"]["id"] == PROMETHEUS
                assert len(fixture.events) == count and fixtures.TOKEN not in json.dumps(result)
                retain_ledger(stage + ":failure")

            fixture.reset()
            fixture.overrides["query"] = (200, {"results": {"A": {"frames": [{"padding": "z" * 5000}]}}}, {})
            result = query(expected=1, extra=("--max-output-bytes", "4096"))
            assert result["data"]["response"] is None and result["data"]["definition"]["id"] == PROMETHEUS
            assert result["effective_limits"]["max_output_bytes"] == 4096
            assert len(fixture.events) == 3
            assert hashlib.sha256(installed.read_bytes()).hexdigest() == source_hash
        finally:
            fixture.close()
            until = time.monotonic() + 2
            while set(threading.enumerate()) - thread_baseline and time.monotonic() < until:
                time.sleep(0.01)
            assert not set(threading.enumerate()) - thread_baseline, "fixture worker did not join"

    summary = {"result": "passed", "binary_sha256": source_hash, "canonical_receipts": len(raw_receipts),
               "strict_query_data_receipts": query_count, "catalog_definitions": 2,
               "installed_copy_outside_checkout": True, "fixture_threads_joined": True,
               "remaining_scope": ["live compatibility", "hosted query grants", "native non-Linux acceptance"]}
    if options.evidence_dir:
        options.evidence_dir.mkdir(parents=True, exist_ok=True)
        (options.evidence_dir / "installed-query-receipts.jsonl").write_bytes(b"".join(raw_receipts))
        (options.evidence_dir / "installed-query-ledgers.json").write_text(json.dumps(received, ensure_ascii=False, indent=2) + "\n")
        (options.evidence_dir / "installed-query-summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps(summary, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
