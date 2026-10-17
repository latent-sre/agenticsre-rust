"""Verify installed MCP diagnostic grants in the repository's private test sandbox."""
from __future__ import annotations

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


def module(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec and spec.loader
    loaded = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(loaded)
    return loaded


def main() -> None:
    assert os.environ.get("CARGO_HOME") == "/tmp/cargo" and Path("/toolchain").is_dir(), "Use tools/test-sandbox.sh"
    source = (ROOT / "target/debug/save").resolve(strict=True)
    diagnostics = module("diagnostic_fixture", ROOT / "tools/verify-diagnostics.py")
    mcp = module("mcp_client", ROOT / "tools/verify-mcp.py")
    schemas = [json.loads(path.read_text()) for path in (ROOT / "docs/sre-workbench/schemas").glob("*.schema.json")]
    registry = Registry().with_resources((schema["$id"], Resource.from_contents(schema)) for schema in schemas)
    result_schema = next(schema for schema in schemas if schema["$id"] == "urn:sre-workbench:spec:result:0.1")
    data_schema = next(schema for schema in schemas if schema["$id"] == "urn:sre-workbench:spec:diagnostics-data:0.1")
    result_validator = Draft202012Validator(result_schema, registry=registry, format_checker=FormatChecker())
    data_validator = Draft202012Validator(data_schema, registry=registry, format_checker=FormatChecker())

    with tempfile.TemporaryDirectory(prefix="mcp-diagnostics-") as temporary:
        root = Path(temporary)
        installed = root / "bin/save"
        installed.parent.mkdir()
        shutil.copyfile(source, installed)
        installed.chmod(0o700)
        assert hashlib.sha256(installed.read_bytes()).digest() == hashlib.sha256(source.read_bytes()).digest()
        certs = diagnostics.certificates(root)
        dns = diagnostics.Dns()
        https = diagnostics.Https(certs["server.pem"], certs["server.key"])
        try:
            config = {"schema_version": 1, "targets": [{
                "id": "fixture-web", "host": "diagnostics.workbench.test", "port": https.port,
                "path": "/probe", "allowed_ips": ["127.0.0.1"],
                "resolver": {"kind": "explicit", "address": "127.0.0.1", "port": dns.port, "transport": "udp"},
                "tls_ca_file": str(certs["ca.pem"]), "process": diagnostics.process_identity(os.getpid()),
            }]}
            config_file = root / "config.json"
            config_file.write_text(json.dumps(config))
            env = {"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8", "TZ": "UTC"}
            operations = ("diagnostics.list", "diagnostics.describe", "diagnostics.run")
            argv = ["--diagnostics-config", str(config_file), "mcp", "serve",
                    *[part for operation in operations for part in ("--allow", operation)],
                    "--diagnostic-target", "fixture-web"]

            def call(client, ident, name, arguments, modern=True):
                client.send(mcp.request(ident, "tools/call", {"name": name, "arguments": arguments}, modern=modern))
                response = client.read()
                assert response["id"] == ident
                return mcp.successful(response, modern=modern)

            def receipt(answer):
                result = answer["structuredContent"]
                result_validator.validate(result)
                assert result["record_mode"] == "never" and result["assessment"] == "not_assessed"
                if result["data"].get("steps"):
                    data_validator.validate(result["data"])
                return result

            with mcp.Client(installed, argv, env=env) as client:
                discovered = mcp.successful(client.request("tools/list", ident="discover"))["tools"]
                assert {tool["name"] for tool in discovered} == {
                    "workbench_diagnostics_list", "workbench_diagnostics_describe", "workbench_diagnostics_run"}
                for tool in discovered:
                    mcp.local_references(tool["inputSchema"])
                    mcp.local_references(tool["outputSchema"])
                    Draft202012Validator.check_schema(tool["inputSchema"])
                    Draft202012Validator.check_schema(tool["outputSchema"])
                run_tool = next(tool for tool in discovered if tool["name"] == "workbench_diagnostics_run")
                assert run_tool["inputSchema"]["properties"]["target_id"]["enum"] == ["fixture-web"]
                assert run_tool["inputSchema"]["properties"]["limits"]["properties"]["concurrency"]["maximum"] == 2
                listed = receipt(call(client, "list", "workbench_diagnostics_list", {}))
                described = receipt(call(client, "describe", "workbench_diagnostics_describe", {"id": "connectivity"}))
                assert listed["data"]["packs"][0]["pack"] == described["data"]["pack"]
                assert not dns.events and not https.events

                arguments = {"target_id": "fixture-web", "id": "connectivity", "version": 1,
                             "input": {"capture_body": False, "on_failure": "continue"}}
                for ident, name, bad in (
                    ("hidden", "workbench_grafana_query", {}),
                    ("target", "workbench_diagnostics_run", arguments | {"target_id": "other"}),
                    ("override", "workbench_diagnostics_run", arguments | {"host": "other.invalid"}),
                    ("null", "workbench_diagnostics_run", arguments | {"input": {"capture_body": None}}),
                    ("limit", "workbench_diagnostics_run", arguments | {"limits": {"concurrency": 3}}),
                ):
                    mcp.error(client.request("tools/call", {"name": name, "arguments": bad}, ident=ident), -32602)
                assert not dns.events and not https.events

                answer = call(client, "modern", "workbench_diagnostics_run", arguments)
                assert answer["isError"] is False
                result = receipt(answer)
                assert result["operation"] == "diagnostics.run"
                assert result["execution"]["status"] == "succeeded" and result["coverage"]["state"] == "complete"
                assert [step["id"] for step in result["data"]["steps"]] == ["dns", "tcp", "tls", "http", "process"]
                assert all(step["attempted"] and step["state"] == "succeeded" for step in result["data"]["steps"])
                assert result["data"]["steps"][4]["observation"]["pid"] == os.getpid()
                assert len(dns.events) == 2 and https.connections == 3 and len(https.events) == 1
                assert result["data"]["binding"]["identity"]["configuration_sha256"] == "sha256:" + hashlib.sha256(config_file.read_bytes()).hexdigest()

                wire = {"spec_version": "0.1", "request_id": "mcp-diagnostic-parity", "operation": "diagnostics.run",
                        "operation_version": 1, "target": {"kind": "connection", "id": "fixture-web"},
                        "inputs": {key: value for key, value in arguments.items() if key != "target_id"},
                        "limits": {"timeout_ms": 30000, "max_output_bytes": 16384, "concurrency": 2}, "record": "never"}
                request_file = root / "request.json"
                request_file.write_text(json.dumps(wire))
                direct = subprocess.run([str(installed), "--json", "--diagnostics-config", str(config_file),
                                         "call", "--request", str(request_file)], cwd=root, env=env,
                                        capture_output=True, timeout=12, check=False)
                assert direct.returncode == 0 and not direct.stderr, direct.stderr
                direct_receipt = json.loads(direct.stdout)
                result_validator.validate(direct_receipt)
                assert diagnostics.comparable(result) == diagnostics.comparable(direct_receipt)

                client.send(mcp.request("init", "initialize", {"protocolVersion": mcp.LEGACY,
                    "capabilities": {}, "clientInfo": {"name": "diagnostic-fixture", "version": "1"}}, modern=False))
                initialized = client.read()
                assert mcp.successful(initialized, modern=False)["protocolVersion"] == mcp.LEGACY
                client.send({"jsonrpc": "2.0", "method": "notifications/initialized"})
                legacy = receipt(call(client, "legacy", "workbench_diagnostics_run", arguments, modern=False))
                assert legacy["execution"]["status"] == "succeeded"
                before = (len(dns.events), len(https.events))
                config["targets"][0]["path"] = "/changed"
                config_file.write_text(json.dumps(config))
                denied = receipt(call(client, "drift", "workbench_diagnostics_run", arguments))
                assert denied["execution"]["status"] == "denied"
                assert any(error["code"] == "diagnostics_identity_changed" for error in denied["errors"])
                assert (len(dns.events), len(https.events)) == before

            for missing in (["mcp", "serve", "--allow", "diagnostics.run"],
                            ["--diagnostics-config", str(config_file), "mcp", "serve", "--allow", "diagnostics.list"],
                            ["--diagnostics-config", str(config_file), "mcp", "serve", "--allow", "diagnostics.run", "--target", "fixture-web"]):
                rejected = subprocess.run([str(installed), *missing], cwd=root, env=env,
                                          input=b"", capture_output=True, timeout=7, check=False)
                assert rejected.returncode == 2 and not rejected.stdout
            print("PASS: installed MCP diagnostics; modern/legacy five-check receipts, CLI parity, grants and pinned drift refusal")
        finally:
            dns.close()
            https.close()


if __name__ == "__main__":
    main()
