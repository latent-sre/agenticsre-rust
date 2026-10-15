"""Verify inert package management with tools/test-sandbox.sh --small-evidence-disk."""
from __future__ import annotations

import argparse
import base64
import copy
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


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--expected-binary-sha256", required=True)
    parser.add_argument("--source-manifest", type=Path, required=True)
    parser.add_argument("--expected-source-manifest-sha256", required=True)
    parser.add_argument("--evidence-dir", type=Path, required=True)
    options = parser.parse_args()
    assert os.environ.get("CARGO_HOME") == "/tmp/cargo" and Path("/toolchain").is_dir(), "Use tools/test-sandbox.sh"
    small_disk = Path("/tmp/evidence-small-disk")
    assert small_disk.is_dir(), "Use --small-evidence-disk for the private disk-full fixture"
    spec = importlib.util.spec_from_file_location("extension_helpers", ROOT / "tools/verify-evidence.py")
    helpers = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helpers)
    manifest = helpers.source_guard(ROOT, options.source_manifest, options.expected_source_manifest_sha256)
    assert manifest["tools/verify-extensions.py"] == sha(Path(__file__).read_bytes())
    binary, fixture = options.binary.resolve(strict=True), options.fixture.resolve(strict=True)
    assert sha(binary.read_bytes()) == options.expected_binary_sha256
    fixture_bytes = fixture.read_bytes()
    marker = Path("/tmp/save-extension-must-not-execute")
    assert not marker.exists()
    evidence = options.evidence_dir.resolve()
    evidence.mkdir(mode=0o700, parents=True, exist_ok=False)
    schemas = {p.stem.removesuffix(".schema"): helpers.strict_json(p.read_bytes())
               for p in (ROOT / "docs/sre-workbench/schemas").glob("*.schema.json")}
    registry = Registry().with_resources((s["$id"], Resource.from_contents(s)) for s in schemas.values())
    validators = {n: Draft202012Validator(schemas[n], registry=registry, format_checker=FormatChecker())
                  for n in ("result", "extension-data", "extension-package")}
    commands, package_hashes = [], {}

    def retain(name, value):
        raw = value if isinstance(value, bytes) else (json.dumps(value, sort_keys=True, indent=2) + "\n").encode()
        p = evidence / name
        p.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        p.write_bytes(raw)

    with tempfile.TemporaryDirectory(prefix="installed-extensions-") as directory:
        scratch = Path(directory)
        installed = scratch / "prefix/bin/save"
        installed.parent.mkdir(parents=True)
        shutil.copy2(binary, installed)
        installed.chmod(0o755)
        cwd = scratch / "unrelated"
        cwd.mkdir()
        package_file, store = scratch / "package.json", scratch / "store"
        template = json.loads((ROOT / "tests/fixtures/extensions/manifest.json").read_text())
        template["executable"].update(sha256="sha256:" + sha(fixture_bytes), bytes=len(fixture_bytes))
        package = {"package_format": 1, "manifest": template,
                   "executable_base64": base64.b64encode(fixture_bytes).decode()}
        validators["extension-package"].validate(package)
        clean = json.dumps(package, separators=(",", ":")).encode()

        def set_package(value):
            raw = value if isinstance(value, bytes) else json.dumps(value, separators=(",", ":")).encode()
            package_file.write_bytes(raw)
            return "sha256:" + sha(raw)

        def run(name, args, expected=0, store_path=store, text=False):
            argv = [str(installed), *([] if text else ["--json"])]
            if store_path is not None:
                argv += ["--extension-state-dir", str(store_path)]
            output = subprocess.run([*argv, *args], cwd=cwd,
                env={"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8", "TZ": "UTC"},
                capture_output=True, timeout=30, check=False)
            retain(f"commands/{name}.stdout" + (".txt" if text else ".json"), output.stdout)
            retain(f"commands/{name}.stderr", output.stderr)
            commands.append({"case": name, "exit": output.returncode})
            assert (output.returncode == 0 if expected == 0 else output.returncode != 0), (name, output.returncode, output.stdout[:3000])
            assert not output.stderr and not marker.exists(), (name, output.stderr)
            if text:
                return output.stdout.decode()
            assert output.stdout.endswith(b"\n") and output.stdout.count(b"\n") == 1
            assert len(output.stdout) <= 2 * 1024 * 1024
            result = helpers.strict_json(output.stdout)
            validators["result"].validate(result)
            assert result["assessment"] == "not_assessed" and result["execution"]["child_exit_code"] is None
            if "packages" in result["data"]:
                validators["extension-data"].validate(result["data"])
                assert not result["data"]["enabled"] and not result["data"]["execution_available"]
                assert "executable_base64" not in result["data"]
            return result

        def inspect(name, expected=0, path=package_file, extra=()):
            return run(name, ["extension", "inspect", str(path), *extra], expected)

        def install(name, expected=0, pin=None, store_path=store, extra=()):
            before = package_file.read_bytes()
            result = run(name, ["extension", "install", str(package_file), "--expected-package-sha256",
                pin or "sha256:" + sha(before), *extra], expected, store_path=store_path)
            assert package_file.read_bytes() == before
            package_hashes[name] = sha(before)
            return result

        pin = set_package(clean)
        observed = inspect("inspect")
        assert not store.exists() and observed["data"]["packages"][0]["package_sha256"] == pin
        assert observed["data"]["packages"][0]["manifest"] == template
        install("wrong-pin", 1, "sha256:" + "0" * 64)
        assert not store.exists()
        install("missing-store", 1, store_path=None)
        assert not store.exists()
        installed_result = install("install")
        assert installed_result["data"]["action"] == "installed"
        assert store.stat().st_mode & 0o777 == 0o700
        before_store = {p.name: sha(p.read_bytes()) for p in store.iterdir()}
        assert all(p.stat().st_mode & 0o777 == 0o600 for p in store.iterdir())
        repeat = install("repeat")
        assert repeat["data"]["action"] == "already_installed"
        assert {p.name: sha(p.read_bytes()) for p in store.iterdir()} == before_store
        listed = run("list", ["extension", "list"])
        assert listed["data"]["installed_count"] == 1
        assert listed["data"]["packages"] == observed["data"]["packages"]
        text = run("list-text", ["extension", "list"], text=True)
        assert "fixture.capacity" in text and "disabled" in text.lower()
        partial = run("bounded-list", ["extension", "list", "--max-output-bytes", "1024"], 1)
        assert partial["execution"]["status"] == "partial" and partial["data"]["omitted_packages"] == 1
        for name, edit in (
            ("replacement", lambda p: p["manifest"]["provenance"].update(revision="replaced")),
            ("protocol", lambda p: p["manifest"]["protocol"].update(version=2)),
            ("hook", lambda p: p["manifest"].update(post_install=["/bin/sh"])),
            ("grants", lambda p: p["manifest"]["capabilities"][0].update(requested_access=["network"])),
            ("schema-reference", lambda p: p["manifest"]["capabilities"][0]["input_schema"].update({"$ref":"https://invalid.example/schema"})),
            ("payload-digest", lambda p: p["manifest"]["executable"].update(sha256="sha256:" + "0" * 64)),
            ("required-mismatch", lambda p: p["manifest"]["capabilities"][0]["input_schema"].update(required=[])),
        ):
            candidate = copy.deepcopy(package)
            edit(candidate)
            set_package(candidate)
            install(name, 1)
            assert {p.name: sha(p.read_bytes()) for p in store.iterdir()} == before_store
        set_package(b'{"package_format":1,' + clean[1:])
        inspect("duplicate-keys", 1)
        set_package(clean)
        link = scratch / "linked.json"
        link.symlink_to(package_file)
        inspect("symlink", 1, link)
        parent_link = scratch / "linked-parent"
        parent_link.symlink_to(scratch, target_is_directory=True)
        inspect("parent-symlink", 1, parent_link / "package.json")
        hardlink = scratch / "hardlink.json"
        os.link(package_file, hardlink)
        inspect("hardlink", 1, hardlink)
        hardlink.unlink()
        fifo = scratch / "fifo"
        os.mkfifo(fifo)
        inspect("fifo", 1, fifo)
        request = {"spec_version":"0.1","request_id":"installed-extension","operation":"extension.inspect",
            "operation_version":1,"target":{"kind":"local","id":"workstation"},
            "inputs":{"package":str(package_file),"expected_package_sha256":pin},
            "limits":{"timeout_ms":30000,"max_output_bytes":1048576},"record":"never"}
        request_path = scratch / "request.json"
        request_path.write_text(json.dumps(request))
        structured = run("structured", ["call","--request",str(request_path)])
        assert structured["data"] == observed["data"]
        request["operation"] = "ext.fixture.capacity.project"
        request["inputs"] = {"capacity_bytes":1000,"used_bytes":500,"growth_bytes_per_day":10}
        request_path.write_text(json.dumps(request))
        run("disabled-dispatch", ["call","--request",str(request_path)], 1)
        stage = store / "stage-interrupted.json"
        stage.write_bytes(b"unfinished installation")
        stage.chmod(0o600)
        run("interrupted-store", ["extension","list"], 1)
        assert stage.read_bytes() == b"unfinished installation"
        stage.unlink()  # Test-owned injection cleanup only, not product recovery.
        stored_package = next(store.glob("package-*.json"))
        original = stored_package.read_bytes()
        stored_package.write_bytes(original + b" ")
        run("replaced-installed-bytes", ["extension","list"], 1)
        assert stored_package.read_bytes() == original + b" "
        stored_package.write_bytes(original)
        assert {p.name:sha(p.read_bytes()) for p in store.iterdir()} == before_store
        set_package(b" " * (12 * 1024 * 1024 + 1))
        inspect("oversized", 1)
        big = copy.deepcopy(package)
        payload = fixture_bytes + bytes(8*1024*1024-len(fixture_bytes))
        big["manifest"]["executable"].update(sha256="sha256:"+sha(payload),bytes=len(payload))
        big["executable_base64"] = base64.b64encode(payload).decode()
        set_package(big)
        timeout = inspect("deadline", 1, extra=("--timeout-ms","1"))
        assert timeout["execution"]["status"] == "timed_out"
        assert {p.name:sha(p.read_bytes()) for p in store.iterdir()} == before_store
        set_package(clean)
        full_store = small_disk / "extensions"
        full = install("disk-full", 1, store_path=full_store)
        assert full["effect_outcome"] == "unknown" and full["execution"]["status"] == "failed"
        assert not list(full_store.glob("package-*.json"))
        residual = {p.name: {"bytes":p.stat().st_size,"sha256":sha(p.read_bytes())}
                    for p in full_store.iterdir()}
        assert any(name.startswith("pending-") for name in residual)
        run("disk-full-recovery-refusal", ["extension","list"], 1, store_path=full_store)
        assert {p.name: {"bytes":p.stat().st_size,"sha256":sha(p.read_bytes())}
                for p in full_store.iterdir()} == residual
        retain("disk-full-residual.json", residual)
        assert not marker.exists()
        assert sha(installed.read_bytes()) == options.expected_binary_sha256
    helpers.source_guard(ROOT, options.source_manifest, options.expected_source_manifest_sha256)
    assert sha(binary.read_bytes()) == options.expected_binary_sha256 and fixture.read_bytes() == fixture_bytes
    retain("package-sha256.json", package_hashes)
    retain("verification.json", {"passed":True,"commands":commands,"source_files":len(manifest),
        "source_manifest_sha256":options.expected_source_manifest_sha256,"binary_sha256":options.expected_binary_sha256,
        "fixture_sha256":sha(fixture_bytes),"installed_copy_outside_checkout":True,"no_package_execution":True,
        "no_implicit_enablement":True,"input_bytes_unchanged":True,"no_credentials_or_network":True})
    print(f"PASS: {len(commands)} installed commands; inert packages, pins, store integrity, refusal, bounds and schemas")


if __name__ == "__main__":
    main()
