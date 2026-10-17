"""Installed MCP evidence collection mission; run only inside tools/test-sandbox.sh."""
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

from jsonschema import Draft202012Validator, FormatChecker
from referencing import Registry, Resource

ROOT = Path(__file__).resolve().parents[1]


def module(name):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), ROOT / "tools" / (name + ".py"))
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def source_snapshot():
    paths = [path for folder in ("crates", "docs/sre-workbench/schemas") for path in (ROOT / folder).rglob("*") if path.is_file()]
    paths.extend(ROOT / name for name in ("Cargo.toml", "Cargo.lock", "tools/verify-evidence-mcp.py", "tools/verify-mcp.py", "tools/verify-evidence.py", "tools/verify-grafana.py"))
    return {str(path.relative_to(ROOT)): digest(path.read_bytes()) for path in sorted(paths)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    assert os.environ.get("CARGO_HOME") == "/tmp/cargo" and Path("/toolchain").is_dir(), "Use test-sandbox.sh"
    binary = args.binary.resolve(strict=True)
    output = args.output.resolve()
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    mcp, evidence, grafana = (module(name) for name in ("verify-mcp", "verify-evidence", "verify-grafana"))
    before = source_snapshot()
    binary_hash = digest(binary.read_bytes())
    schemas = [json.loads(path.read_bytes()) for path in (ROOT / "docs/sre-workbench/schemas").glob("*.schema.json")]
    registry = Registry().with_resources((schema["$id"], Resource.from_contents(schema)) for schema in schemas)
    validators = {schema["$id"]: Draft202012Validator(schema, registry=registry, format_checker=FormatChecker()) for schema in schemas}
    checks, receipts = [], []

    def retain(name, value):
        raw = value if isinstance(value, bytes) else (json.dumps(value, indent=2, ensure_ascii=False) + "\n").encode()
        evidence.assert_no_canary(raw)
        (output / name).write_bytes(raw)
        return digest(raw)

    def passed(message):
        checks.append(message)
        print("PASS " + message, flush=True)

    with tempfile.TemporaryDirectory(prefix="evidence-mcp-") as temporary:
        scratch = Path(temporary)
        installed = scratch / "bin/save"
        installed.parent.mkdir()
        shutil.copy2(binary, installed)
        certificates = grafana.certificates(scratch)
        fixture = evidence.Fixture(certificates["server.pem"], certificates["server.key"])
        try:
            store, source, config = (scratch / name for name in ("store", "context.json", "config.json"))
            config.write_text(json.dumps({"spec_version":"0.1", "record":"never", "connections":{"fixture":{
                "origin":fixture.origin,"expected_org_id":7,"api_profile":"grafana-legacy-v1",
                "credential_ref":"env:" + evidence.PROVIDER,"tls_ca_file":str(certificates["ca.pem"])}}}))
            context = json.loads((ROOT / "tests/fixtures/context/unique.json").read_bytes())
            context["records"][0].update(service="fixture-web", last_reviewed=datetime.now(timezone.utc).isoformat(),
                bindings={"target":"fixture","environment":"dev","organization":"7"})
            source.write_text(json.dumps(context))
            env = {"PATH":"/usr/bin:/bin", evidence.PROVIDER:json.dumps({"origin":fixture.origin,"expected_org_id":7,
                "operations":["grafana.query"],"auth":{"kind":"bearer","token":evidence.TOKEN}})}
            globals_ = ["--config",str(config),"--context-source",str(source),"--context-max-age-days","30","--state-dir",str(store)]
            inputs = {"service":"fixture-web","environment":"dev","pack":{"id":"service-queries","version":1},
                "window":{"kind":"absolute","from":"2026-10-02T00:00:00Z","to":"2026-10-02T00:05:00Z"},
                "metrics":{"datasource":"metrics-1","parameters":{"job":"fixture"}},
                "logs":{"datasource":"logs-1","parameters":{"job":"fixture"}},"on_failure":"continue"}
            argv = [*globals_,"mcp","serve","--allow","evidence.collect","--target","fixture"]
            denied = subprocess.run([str(installed),*argv], input=b"", env=env, capture_output=True, timeout=10)
            assert denied.returncode != 0 and not store.exists() and not fixture.events
            passed("Collection startup refuses a missing store without initialization or HTTP")
            # Ordinary operator collection establishes the store through the production writer.
            input_path = scratch / "input.json"
            input_path.write_text(json.dumps(inputs))
            bootstrap = subprocess.run([str(installed),"--json",*globals_,"--record","always","evidence","collect",
                "--target","fixture","--input",str(input_path)], env=env, capture_output=True, timeout=15)
            assert bootstrap.returncode == 0 and not bootstrap.stderr, bootstrap.stdout[:1000]
            retain("bootstrap.json", bootstrap.stdout)
            assert len(fixture.events) == 6
            tools = {}

            def exchange(client, method, params, modern=True):
                client.send(mcp.request("mission", method, params, modern=modern))
                return client.read()

            def call(client, name, arguments, case, modern=True, status="succeeded"):
                answer = mcp.successful(exchange(client,"tools/call",{"name":name,"arguments":arguments},modern), modern)
                result = answer["structuredContent"]
                assert result["execution"]["status"] == status, result
                assert answer["isError"] is (status != "succeeded")
                assert result["assessment"] == "not_assessed"
                validators["urn:sre-workbench:spec:result:0.1"].validate(result)
                if name in tools:
                    Draft202012Validator(tools[name]["outputSchema"]).validate(result)
                receipts.append({"case":case,"sha256":retain(case + ".json",answer)})
                return result

            grants = [*argv,"--allow","evidence.packs.list","--allow","evidence.packs.describe","--allow","evidence.open"]
            with mcp.Client(installed,grants,env=env) as client:
                discovered = mcp.successful(client.request("tools/list"))["tools"]
                assert len(discovered) == 4
                assert str(scratch) not in json.dumps(discovered)
                for tool in discovered:
                    tools[tool["name"]] = tool
                    for key in ("inputSchema","outputSchema"):
                        mcp.local_references(tool[key]); Draft202012Validator.check_schema(tool[key])
                    collecting = tool["name"] == "workbench_evidence_collect"
                    assert tool["annotations"]["readOnlyHint"] is (not collecting)
                    assert tool["annotations"]["destructiveHint"] is False
                    assert tool["annotations"]["openWorldHint"] is collecting
                retain("discovery.json",discovered)
                passed("Discovery exposes only granted tools with local schemas and truthful storage-write annotations")
                for modern in (True,False):
                    if not modern:
                        initialized = exchange(client,"initialize",{"protocolVersion":mcp.LEGACY,"capabilities":{},"clientInfo":{"name":"fixture","version":"1"}},False)
                        assert mcp.successful(initialized,False)["protocolVersion"] == mcp.LEGACY
                        client.send({"jsonrpc":"2.0","method":"notifications/initialized"})
                    suffix = "modern" if modern else "legacy"
                    call(client,"workbench_evidence_packs_list",{},"list-" + suffix,modern)
                    call(client,"workbench_evidence_packs_describe",{"id":"service-queries","version":1},"describe-" + suffix,modern)
                    arguments = {**inputs,"target_id":"fixture"}
                    Draft202012Validator(tools["workbench_evidence_collect"]["inputSchema"]).validate(arguments)
                    count = len(fixture.events)
                    collected = call(client,"workbench_evidence_collect",arguments,"collect-" + suffix,modern)
                    assert collected["record_mode"] == "always"
                    assert collected["data"]["capture"]["state"] == "complete"
                    assert len(fixture.events) == count + 6
                    validators["urn:sre-workbench:spec:evidence-collection-data:0.1"].validate(collected["data"])
                    ref = collected["data"]["bundle"]
                    manifest_bytes = (store / "runs" / ref["id"] / "manifest.json").read_bytes()
                    assert "sha256:" + digest(manifest_bytes) == ref["digest"]
                    manifest = json.loads(manifest_bytes)
                    assert len(manifest["artifacts"]) == 2
                    for artifact in manifest["artifacts"].values():
                        raw = (store / "runs" / ref["id"] / artifact["path"]).read_bytes()
                        assert "sha256:" + digest(raw) == artifact["digest"]
                        evidence.assert_no_canary(raw)
                        validators["urn:sre-workbench:spec:result:0.1"].validate(json.loads(raw))
                    call(client,"workbench_evidence_open",{"id":ref["id"],"expected_manifest_sha256":ref["digest"]},"open-" + suffix,modern)
                    assert len(fixture.events) == count + 6
                passed("Modern and legacy protocols collect two real HTTPS queries and independently open verified durable bundles")
                count, stored = len(fixture.events), evidence.private_files(store)
                for key,value in (("state_dir","/tmp/override"),("record","never"),("policy",{}),("expr","up"),
                    ("expected_pack_sha256",None),("target_id","ungranted"),("limits",{"concurrency":1}),
                    ("limits",{"timeout_ms":60001}),("pack",{"id":"service-queries","version":2})):
                    bad = {**inputs,"target_id":"fixture",key:value}
                    mcp.error(exchange(client,"tools/call",{"name":"workbench_evidence_collect","arguments":bad},False),-32602)
                assert len(fixture.events) == count and evidence.private_files(store) == stored
                passed("Wire overrides, invalid packs, null pins, targets and limits fail without HTTP or store mutation")
                for label,path in (("config",config),("source",source),("ca",certificates["ca.pem"])):
                    original = path.read_bytes(); path.write_bytes(original + b" ")
                    call(client,"workbench_evidence_collect",{**inputs,"target_id":"fixture"},"changed-" + label,False,"denied")
                    path.write_bytes(original)
                    assert len(fixture.events) == count and evidence.private_files(store) == stored
                old = scratch / "old-store"
                store.rename(old)
                shutil.copytree(old,store)
                call(client,"workbench_evidence_collect",{**inputs,"target_id":"fixture"},"replaced-store",False,"failed")
                assert len(fixture.events) == count and evidence.private_files(store) == stored
                passed("Consumed config, CA and source bytes plus store descriptor identity reject drift before HTTP or mutation")

            for grant,name,input_ in (("evidence.packs.list","workbench_evidence_packs_list",{}),
                ("evidence.packs.describe","workbench_evidence_packs_describe",{"id":"service-queries","version":1})):
                with mcp.Client(installed,["mcp","serve","--allow",grant],env={}) as client:
                    assert len(mcp.successful(client.request("tools/list"))["tools"]) == 1
                    call(client,name,input_,"isolated-" + grant)
                    mcp.error(client.request("tools/call",{"name":"workbench_evidence_collect","arguments":{**inputs,"target_id":"fixture"}}),-32602)
            with mcp.Client(installed,["--state-dir",str(store),"mcp","serve","--allow","evidence.open"],env={}) as client:
                mcp.error(client.request("tools/call",{"name":"workbench_evidence_collect","arguments":{**inputs,"target_id":"fixture"}}),-32602)
            with mcp.Client(installed,env={}) as client:
                assert mcp.successful(client.request("tools/list"))["tools"] == []
                mcp.error(client.request("tools/call",{"name":"workbench_evidence_collect","arguments":{**inputs,"target_id":"fixture"}}),-32602)
            assert len(fixture.events) == count
            passed("Catalog, offline-reader and empty grants never acquire collection authority")
            retained = evidence.private_files(store)
            for raw in retained.values(): evidence.assert_no_canary(raw)
            retain("retained-files.json",{path:digest(raw) for path,raw in retained.items()})
            assert digest(installed.read_bytes()) == binary_hash
        finally:
            fixture.close()
    assert source_snapshot() == before and digest(binary.read_bytes()) == binary_hash
    retain("source-manifest.json",before)
    retain("summary.json",{"status":"passed","binary_sha256":binary_hash,"checks":checks,"receipts":receipts})


if __name__ == "__main__":
    main()
