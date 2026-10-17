"""Installed declared configuration-check acceptance; run through tools/test-sandbox.sh."""
from __future__ import annotations

import argparse
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


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--expected-binary-sha256", required=True)
    parser.add_argument("--source-manifest", type=Path, required=True)
    parser.add_argument("--expected-source-manifest-sha256", required=True)
    parser.add_argument("--evidence-dir", type=Path, required=True)
    options = parser.parse_args()
    assert os.environ.get("CARGO_HOME") == "/tmp/cargo" and Path("/toolchain").is_dir(), "Use tools/test-sandbox.sh"
    spec = importlib.util.spec_from_file_location("config_evidence_helpers", ROOT / "tools/verify-evidence.py")
    helpers = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helpers)
    manifest = helpers.source_guard(ROOT, options.source_manifest, options.expected_source_manifest_sha256)
    assert manifest["tools/verify-config-check.py"] == digest(Path(__file__).read_bytes())
    binary = options.binary.resolve(strict=True)
    assert digest(binary.read_bytes()) == options.expected_binary_sha256
    evidence = options.evidence_dir.resolve()
    evidence.mkdir(mode=0o700, parents=True, exist_ok=False)
    schemas = {p.stem.removesuffix(".schema"): helpers.strict_json(p.read_bytes())
               for p in (ROOT / "docs/sre-workbench/schemas").glob("*.schema.json")}
    registry = Registry().with_resources((s["$id"], Resource.from_contents(s)) for s in schemas.values())
    validators = {name: Draft202012Validator(schemas[name], registry=registry, format_checker=FormatChecker())
                  for name in ("result", "config-check-data")}
    commands = []
    observed_inputs = {}

    def retain(name, value):
        raw = value if isinstance(value, bytes) else (json.dumps(value, indent=2, sort_keys=True) + "\n").encode()
        path = evidence / name
        path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        path.write_bytes(raw)

    with tempfile.TemporaryDirectory(prefix="installed-config-") as directory:
        scratch = Path(directory)
        installed = scratch / "prefix/bin/save"
        installed.parent.mkdir(parents=True)
        shutil.copy2(binary, installed)
        installed.chmod(0o755)
        cwd = scratch / "unrelated"
        cwd.mkdir()
        # Same-named hostile source must not replace the embedded reviewed binding.
        (cwd / "dashboard_hygiene.py").write_text("raise RuntimeError('untrusted workspace code')\n")
        model = scratch / "model.json"
        typed = scratch / "input.json"
        clean = (ROOT / "tests/fixtures/dashboard-hygiene/clean.json").read_bytes()
        findings = (ROOT / "tests/fixtures/dashboard-hygiene/findings.json").read_bytes()
        base = {"kind": "grafana-dashboard", "validator": {"id": "dashboard-hygiene", "version": 1}, "file": str(model)}

        def run(name, args, expected=0, text=False):
            output = subprocess.run([str(installed), *([] if text else ["--json"]), *args], cwd=cwd,
                env={"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8", "TZ": "UTC", "PYTHONPATH": str(cwd)},
                capture_output=True, timeout=15, check=False)
            retain(f"commands/{name}.stdout" + (".txt" if text else ".json"), output.stdout)
            retain(f"commands/{name}.stderr", output.stderr)
            commands.append({"case": name, "exit": output.returncode})
            assert output.returncode == expected and not output.stderr, (name, output.returncode, output.stdout[:2000], output.stderr[:300])
            if text:
                return output.stdout.decode()
            assert output.stdout.endswith(b"\n") and output.stdout.count(b"\n") == 1
            assert len(output.stdout) <= 2 * 1024 * 1024
            result = helpers.strict_json(output.stdout)
            validators["result"].validate(result)
            assert result["assessment"] == "not_assessed"
            if "validation_state" in result["data"]:
                validators["config-check-data"].validate(result["data"])
            return result

        def check(name, content, expected=0, inputs=None, extra=()):
            model.write_bytes(content)
            typed.write_text(json.dumps(inputs or base))
            before = digest(model.read_bytes())
            result = run(name, ["config", "check", "--input", str(typed), *extra], expected)
            assert before == digest(model.read_bytes()), "Original configuration mutated"
            observed_inputs[name] = before
            return result

        valid = check("clean", clean)
        data = valid["data"]
        assert data["validation_state"] == "complete" and data["checked_panels"] == 1 and data["findings_total"] == 0
        assert data["input"]["digest"] == "sha256:" + digest(clean) and data["input"]["bytes"] == len(clean)
        assert len(data["implemented_rules"]) == 10 and "live_datasource_queries" in data["unsupported_sections"]
        assert data["validator"]["checker_digest"] == "sha256:" + digest((ROOT / "crates/workbench-core/resources/dashboard-hygiene/dashboard_hygiene.py").read_bytes())
        envelope = {"kind": "Dashboard", "apiVersion": "dashboard.grafana.app/v1beta1", "spec": json.loads(clean)}
        assert check("supported-envelope", json.dumps(envelope).encode())["data"]["validation_state"] == "complete"
        envelope["apiVersion"] = "dashboard.grafana.app/v99"
        unknown_schema = check("unsupported-schema", json.dumps(envelope).encode(), 1)
        assert unknown_schema["data"]["validation_state"] == "unsupported"
        assert "grafana_dashboard_schema" in unknown_schema["data"]["unsupported_sections"]
        pinned = {**base, "validator": {**base["validator"], "expected_binding_sha256": data["validator"]["binding_digest"]}, "expected_input_sha256": "sha256:" + digest(clean)}
        assert check("pinned", clean, inputs=pinned)["data"]["validator"] == data["validator"]
        changed = check("input-pin-mismatch", findings, 1, pinned)
        assert any(e["code"] == "input_digest_mismatch" for e in changed["errors"])
        assert changed["data"]["input"]["digest"] == "sha256:" + digest(findings) and changed["data"]["checked_panels"] == 0
        bad_validator = {**base, "file": str(scratch / "missing"), "validator": {**base["validator"], "expected_binding_sha256": "sha256:" + "0" * 64}}
        refused = check("validator-pin-mismatch", clean, 2, bad_validator)
        assert any(e["code"] == "validator_digest_mismatch" for e in refused["errors"])
        assert refused["effect_outcome"] == "not_attempted" and not refused["data"].get("supervisor")
        ordinary = check("findings", findings)
        fail_policy = check("findings-exit", findings, 1, extra=("--fail-on-findings",))
        assert ordinary["data"]["findings_total"] > 0 and ordinary["data"]["findings"] == fail_policy["data"]["findings"]
        assert fail_policy["execution"]["status"] == "succeeded"
        rendered = run("findings-text", ["config", "check", "--input", str(typed)], text=True)
        assert "dashboard-hygiene" in rendered and "sha256:" in rendered
        assert ordinary["data"]["findings"][0]["rule"] in rendered
        for name, content, code in (
            ("invalid-json", b'{"title":"input-canary-never-echo",', "invalid_model_json"),
            ("duplicate-key", b'{"panels":[],"panels":[]}', "duplicate_model_key"),
            ("empty", b'{"panels":[]}', "uncheckable_model"),
            ("unsupported-v2", b'{"elements":{},"layout":{}}', "unsupported_model_v2"),
            ("invalid-utf8", b'\xff', "invalid_model_encoding"),
        ):
            result = check(name, content, 1)
            assert any(e["code"] == code for e in result["errors"]), result
            assert result["data"]["validation_state"] != "complete"
            assert result["data"]["input"]["digest"] == "sha256:" + digest(content)
            assert "input-canary-never-echo" not in json.dumps(result)
        large = check("oversized", b" " * (2 * 1024 * 1024 + 1), 1)
        assert large["data"]["input"]["digest"] is None and large["data"]["input"]["bytes"] is None
        partial_model = json.loads(clean)
        partial_model["panels"][0]["title"] = "long" * 200
        partial = check("partial-fields", json.dumps(partial_model).encode(), 1)
        assert partial["data"]["validation_state"] == "partial" and partial["data"]["fields_truncated"]
        many_findings = json.loads(findings)
        many_findings["panels"] *= 20
        limited = check("capture-limited", json.dumps(many_findings).encode(), 1, extra=("--max-output-bytes", "1024"))
        assert limited["data"]["validation_state"] != "complete"
        fifo = scratch / "fifo"
        os.mkfifo(fifo)
        typed.write_text(json.dumps({**base, "file": str(fifo)}))
        nonregular = run("fifo-refused", ["config", "check", "--input", str(typed)], 1)
        assert any(e["code"] == "non_regular_model" for e in nonregular["errors"])
        model.write_bytes(clean)
        request = {"spec_version": "0.1", "request_id": "config-installed", "operation": "config.check",
            "operation_version": 1, "target": {"kind": "local", "id": "workstation"}, "inputs": pinned,
            "limits": {"timeout_ms": 30000, "max_output_bytes": 1048576}, "record": "never"}
        request_path = scratch / "request.json"
        request_path.write_text(json.dumps(request))
        structured = run("structured", ["call", "--request", str(request_path)])["data"]
        assert structured["validator"] == data["validator"] and structured["input"] == data["input"]
        assert structured["findings"] == data["findings"] and model.read_bytes() == clean
        discovery = run("discover", ["capabilities", "describe", "config.check"])
        assert "config.check" in json.dumps(discovery)
        assert digest(installed.read_bytes()) == options.expected_binary_sha256
    helpers.source_guard(ROOT, options.source_manifest, options.expected_source_manifest_sha256)
    assert digest(binary.read_bytes()) == options.expected_binary_sha256
    retain("input-sha256.json", observed_inputs)
    retain("verification.json", {"passed": True, "commands": commands, "source_files": len(manifest),
        "source_manifest_sha256": options.expected_source_manifest_sha256, "binary_sha256": options.expected_binary_sha256,
        "installed_copy_outside_checkout": True, "input_files_unchanged": True, "no_credentials_or_network": True,
        "hostile_module_not_imported": True})
    print(f"PASS: {len(commands)} installed commands; input/validator pins, findings, unsupported models, bounds and schemas")


if __name__ == "__main__":
    main()
