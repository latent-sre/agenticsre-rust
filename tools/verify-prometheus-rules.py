"""Installed pinned Prometheus rule validation; run through tools/test-sandbox.sh."""
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
PROMTOOL_SHA256 = "c736d55d3ccd959fe48329965fb5cb671d45585a9483954766448035100ff75c"


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--promtool", type=Path, required=True)
    parser.add_argument("--expected-binary-sha256", required=True)
    parser.add_argument("--source-manifest", type=Path, required=True)
    parser.add_argument("--expected-source-manifest-sha256", required=True)
    parser.add_argument("--evidence-dir", type=Path, required=True)
    options = parser.parse_args()
    assert os.environ.get("CARGO_HOME") == "/tmp/cargo" and Path("/toolchain").is_dir(), "Use tools/test-sandbox.sh"
    spec = importlib.util.spec_from_file_location("rule_evidence_helpers", ROOT / "tools/verify-evidence.py")
    helpers = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helpers)
    manifest = helpers.source_guard(ROOT, options.source_manifest, options.expected_source_manifest_sha256)
    assert manifest["tools/verify-prometheus-rules.py"] == digest(Path(__file__).read_bytes())
    binary, dependency = options.binary.resolve(strict=True), options.promtool.resolve(strict=True)
    assert digest(binary.read_bytes()) == options.expected_binary_sha256
    assert digest(dependency.read_bytes()) == PROMTOOL_SHA256
    evidence = options.evidence_dir.resolve()
    evidence.mkdir(mode=0o700, parents=True, exist_ok=False)
    schemas = {p.stem.removesuffix(".schema"): helpers.strict_json(p.read_bytes())
               for p in (ROOT / "docs/sre-workbench/schemas").glob("*.schema.json")}
    registry = Registry().with_resources((s["$id"], Resource.from_contents(s)) for s in schemas.values())
    validators = {name: Draft202012Validator(schemas[name], registry=registry, format_checker=FormatChecker())
                  for name in ("result", "config-check-data")}
    commands, inputs_seen = [], {}
    snapshot_names_before = set(Path("/tmp").glob("promtool-snapshot-*"))

    def retain(name, value):
        raw = value if isinstance(value, bytes) else (json.dumps(value, indent=2, sort_keys=True) + "\n").encode()
        path = evidence / name
        path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        path.write_bytes(raw)

    with tempfile.TemporaryDirectory(prefix="installed-rules-") as directory:
        scratch = Path(directory)
        installed, promtool = scratch / "prefix/bin/save", scratch / "prefix/bin/promtool"
        installed.parent.mkdir(parents=True)
        shutil.copy2(binary, installed)
        shutil.copy2(dependency, promtool)
        installed.chmod(0o755)
        promtool.chmod(0o755)
        cwd = scratch / "unrelated"
        cwd.mkdir()
        model, typed = scratch / "rules.yml", scratch / "input.json"
        fixtures = {p.name: p.read_bytes() for p in (ROOT / "tests/fixtures/prometheus-rules").glob("*.yml")}
        base = {"kind": "prometheus-rules", "validator": {"id": "promtool", "version": 1}, "file": str(model)}

        def run(name, args, expected=0, tool=promtool, text=False):
            argv = [str(installed), *([] if text else ["--json"])]
            if tool is not None:
                argv += ["--promtool", str(tool)]
            output = subprocess.run([*argv, *args], cwd=cwd,
                env={"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8", "TZ": "UTC"},
                capture_output=True, timeout=30, check=False)
            retain(f"commands/{name}.stdout" + (".txt" if text else ".json"), output.stdout)
            retain(f"commands/{name}.stderr", output.stderr)
            commands.append({"case": name, "exit": output.returncode})
            assert output.returncode == expected and not output.stderr, (name, output.returncode, output.stdout[:3000], output.stderr[:300])
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

        def check(name, content, expected=0, inputs=None, extra=(), tool=promtool):
            model.write_bytes(content)
            typed.write_text(json.dumps(inputs or base))
            before = digest(model.read_bytes())
            result = run(name, ["config", "check", "--input", str(typed), *extra], expected, tool=tool)
            assert before == digest(model.read_bytes()), "Original rule file mutated"
            inputs_seen[name] = before
            return result

        clean = fixtures["clean.yml"]
        valid = check("valid", clean)
        data = valid["data"]
        assert data["validation_outcome"] == "valid" and data["validation_state"] == "complete"
        assert data["checked_rules"] == 2 and data["findings_total"] == 0
        assert data["input"]["digest"] == "sha256:" + digest(clean)
        assert data["runtime"]["executable_sha256"] == "sha256:" + PROMTOOL_SHA256
        assert data["runtime"]["version"] == "3.15.0"
        assert len(data["implemented_rules"]) == 5 and len(data["unsupported_sections"]) == 5
        pinned = {**base, "validator": {**base["validator"], "expected_binding_sha256": data["validator"]["binding_digest"]}, "expected_input_sha256": "sha256:" + digest(clean)}
        assert check("pinned", clean, inputs=pinned)["data"]["input"] == data["input"]
        mismatch = check("input-pin-mismatch", fixtures["invalid-promql.yml"], 1, pinned)
        assert mismatch["effect_outcome"] == "not_attempted" and not mismatch["data"].get("supervisor")
        assert any(e["code"] == "input_digest_mismatch" for e in mismatch["errors"])
        for name in ("invalid-promql", "invalid-template", "unknown-field", "invalid-recording-fields"):
            content = fixtures[name + ".yml"]
            result = check(name, content, 1)
            assert result["data"]["validation_outcome"] == "invalid" and result["execution"]["child_exit_code"] == 1
            assert result["data"]["input"]["digest"] == "sha256:" + digest(content)
            assert result["data"]["diagnostics"]["untrusted"] and result["data"]["diagnostics"]["stderr"]
        duplicate = check("duplicate", fixtures["duplicate.yml"])
        assert duplicate["data"]["validation_outcome"] == "lint_findings" and duplicate["execution"]["child_exit_code"] == 3
        assert duplicate["data"]["findings_total"] > 0
        policy = check("duplicate-exit", fixtures["duplicate.yml"], 1, extra=("--fail-on-findings",))
        assert policy["execution"]["status"] == "succeeded" and policy["data"]["findings"] == duplicate["data"]["findings"]
        rendered = run("duplicate-text", ["config", "check", "--input", str(typed)], text=True)
        assert "duplicate-rules" in rendered and "promtool" in rendered
        empty = check("empty", fixtures["empty.yml"], 1)
        assert empty["data"]["validation_outcome"] == "uncheckable" and empty["execution"]["child_exit_code"] == 0
        for name, content, code in (("multiple-documents", fixtures["multiple-documents.yml"], 0),
            ("multiple-documents-lint", fixtures["duplicate.yml"] + b"---\ngroups: []\n", 3)):
            partial = check(name, content, 1)
            assert partial["execution"]["status"] == "partial" and partial["execution"]["child_exit_code"] == code
            assert partial["data"]["validation_outcome"] == "unknown"
        many = "groups:\n  - name: many\n    rules:\n" + "".join(f"      - alert: Broken{i}\n        expr: sum(\n" for i in range(100))
        limited = check("output-limited", many.encode(), 1, extra=("--max-output-bytes", "1024"))
        assert limited["data"]["validation_state"] == "partial" and limited["data"]["diagnostics"]["truncated"]
        large = check("oversized", b" " * (2 * 1024 * 1024 + 1), 1)
        assert large["data"]["input"]["digest"] is None and not large["data"].get("supervisor")
        missing = check("missing-dependency", clean, 2, tool=None)
        assert missing["effect_outcome"] == "not_attempted"
        wrong = check("wrong-binary", clean, 2, tool=Path("/usr/bin/true").resolve(strict=True))
        assert not wrong["data"].get("supervisor") and wrong["data"]["input"]["digest"] is None
        badpin = {**base, "validator": {**base["validator"], "expected_binding_sha256": "sha256:" + "0" * 64}}
        refused = check("binding-pin", clean, 2, badpin, tool=scratch / "absent-promtool")
        assert any(e["code"] == "validator_digest_mismatch" for e in refused["errors"])
        timeout = check("preparation-deadline", clean, 1, extra=("--timeout-ms", "1"))
        assert timeout["execution"]["status"] == "timed_out" and not timeout["data"].get("supervisor")
        fifo = scratch / "fifo"
        os.mkfifo(fifo)
        typed.write_text(json.dumps({**base, "file": str(fifo)}))
        refused = run("fifo", ["config", "check", "--input", str(typed)], 1)
        assert not refused["data"].get("supervisor")
        model.write_bytes(clean)
        request = {"spec_version": "0.1", "request_id": "rules-installed", "operation": "config.check",
            "operation_version": 1, "target": {"kind": "local", "id": "workstation"}, "inputs": pinned,
            "limits": {"timeout_ms": 30000, "max_output_bytes": 1048576}, "record": "never"}
        request_path = scratch / "request.json"
        request_path.write_text(json.dumps(request))
        structured = run("structured", ["call", "--request", str(request_path)])["data"]
        assert structured["validator"] == data["validator"] and structured["input"] == data["input"]
        assert structured["checked_rules"] == 2 and model.read_bytes() == clean
        assert digest(installed.read_bytes()) == options.expected_binary_sha256
        assert digest(promtool.read_bytes()) == PROMTOOL_SHA256
    helpers.source_guard(ROOT, options.source_manifest, options.expected_source_manifest_sha256)
    assert set(Path("/tmp").glob("promtool-snapshot-*")) == snapshot_names_before
    assert digest(binary.read_bytes()) == options.expected_binary_sha256
    assert digest(dependency.read_bytes()) == PROMTOOL_SHA256
    retain("input-sha256.json", inputs_seen)
    retain("verification.json", {"passed": True, "commands": commands, "source_files": len(manifest),
        "source_manifest_sha256": options.expected_source_manifest_sha256, "binary_sha256": options.expected_binary_sha256,
        "promtool_sha256": PROMTOOL_SHA256, "installed_copy_outside_checkout": True,
        "input_files_unchanged": True, "no_credentials_or_network": True, "no_executable_snapshot_names_retained": True})
    print(f"PASS: {len(commands)} installed commands; native rules, pins, lint, incomplete coverage, bounds and schemas")


if __name__ == "__main__":
    main()
