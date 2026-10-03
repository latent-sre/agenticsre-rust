#!/usr/bin/env python3
"""Public CLI integrity/selection tests; run with tools/test-sandbox.sh.

Uses ordinary system ELF bytes as synthetic packaging inputs, never executes them,
and does not establish acceptance for the save product or any native platform.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest

TOOL = Path(__file__).with_name("package-linux.py")


def encode(value: object) -> bytes:
    return (json.dumps(value, sort_keys=True, ensure_ascii=True, separators=(",", ":")) + "\n").encode()


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


class BundleTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="bundle-properties-")
        self.base = Path(self.temporary.name)
        self.root = self.base / "installation"
        self.notices = self.base / "notices"
        self.notices.mkdir()
        (self.notices / "LICENSE").write_text("synthetic test project notice\n", encoding="utf-8")
        (self.notices / "project").mkdir()
        for name in ("third-party__save-toolkit-LICENSE.txt",
                     "crates__workbench-core__resources__dashboard-hygiene__LICENSE",
                     "crates__workbench-core__resources__error-budget__LICENSE"):
            (self.notices / "project" / name).write_text("synthetic property fixture upstream notice\n", encoding="utf-8")
        self.source = self.base / "source.json"
        self.source.write_bytes(encode({"schema_version": 1, "git_commit": "1" * 40,
                                      "git_tree": "2" * 40,
                                      "files": {"fixture.txt": sha(b"synthetic source fixture\n")},
                                      "capture_note": "synthetic property fixture, not a product source build"}))
        self.inventory = self.base / "inventory.json"
        self.inventory.write_bytes(encode({"schema_version": 1, "scope": "synthetic fixture only",
                                         "packages": [], "distribution_gaps": ["not a distribution license review"]}))
        self.bundle, self.first = self.package("first", Path("/usr/bin/true"))

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def cli(self, *arguments, refusal: str | None = None) -> dict:
        result = subprocess.run([sys.executable, "-B", str(TOOL), *map(str, arguments)],
                                capture_output=True, timeout=15, check=False)
        self.assertEqual(result.returncode, 2 if refusal is not None else 0,
                         (result.stdout.decode(), result.stderr.decode()))
        self.assertFalse(result.stdout if refusal is not None else result.stderr)
        response = json.loads(result.stderr if refusal is not None else result.stdout)
        if refusal is not None:
            self.assertEqual(response["status"], "refused")
            self.assertIn(refusal, response["reason"])
        return response

    def package(self, name: str, binary: Path, refusal: str | None = None) -> tuple[Path, dict]:
        local_binary = self.base / f"{name}-elf"
        shutil.copyfile(binary, local_binary)
        data = local_binary.read_bytes()
        build = self.base / f"{name}-build.json"
        build.write_bytes(encode({"schema_version": 1, "source_manifest_sha256": sha(self.source.read_bytes()),
                                 "binary_sha256": sha(data), "binary_size": len(data),
                                 "build_command": ["synthetic-property-fixture"],
                                 "rustc": "not exercised", "cargo": "not exercised",
                                 "platform": {"os": "linux", "architecture": "x86_64", "libc": "test fixture",
                                              "environment": "system ELF never executed"}}))
        bundle = self.base / f"{name}.tar"
        response = self.cli("create", "--binary", local_binary, "--source-manifest", self.source,
                            "--build-provenance", build, "--inventory", self.inventory,
                            "--notices-dir", self.notices, "--product-version", "0.1.0", "--output", bundle,
                            refusal=refusal)
        return bundle, response

    def entries(self) -> list[tuple[tarfile.TarInfo, bytes]]:
        with tarfile.open(self.bundle, "r:") as archive:
            return [(item, archive.extractfile(item).read()) for item in archive]

    def rewrite(self, entries: list[tuple[tarfile.TarInfo, bytes]]) -> tuple[Path, str]:
        data = bytearray()
        for item, payload in entries:
            item.size = len(payload)
            data.extend(item.tobuf(format=tarfile.USTAR_FORMAT))
            data.extend(payload)
            data.extend(bytes((-len(payload)) % 512))
        data.extend(bytes(1024))
        path = self.base / "hostile.tar"
        path.write_bytes(data)
        return path, sha(data)

    def refuse_install(self, entries, reason: str) -> None:
        bundle, checksum = self.rewrite(entries)
        self.cli("install", bundle, "--expected-sha256", checksum, "--root", self.root, refusal=reason)
        self.assertFalse(self.root.exists(), "invalid archive wrote installation state")

    def install_first(self) -> dict:
        return self.cli("install", self.bundle, "--expected-sha256", self.first["archive_sha256"],
                        "--root", self.root)

    def inventory_with_references(self, references) -> dict:
        value = json.loads(self.inventory.read_bytes())
        value["packages"] = [{"name": "synthetic-dependency", "notice_files": references}]
        return value

    def entries_with_inventory(self, inventory: dict) -> list[tuple[tarfile.TarInfo, bytes]]:
        entries = self.entries()
        payload = encode(inventory)
        index = next(i for i, (item, _) in enumerate(entries)
                     if item.name == "provenance/dependency-inventory.json")
        entries[index] = (entries[index][0], payload)
        manifest = json.loads(entries[0][1])
        descriptor = next(item for item in manifest["files"]
                          if item["path"] == "provenance/dependency-inventory.json")
        descriptor.update(size=len(payload), sha256=sha(payload))
        manifest["dependency_inventory"].update(sha256=sha(payload), scope=inventory["scope"],
                                                 distribution_gaps=inventory["distribution_gaps"])
        entries[0] = (entries[0][0], encode(manifest))
        return entries

    def same_size_replacement(self) -> tuple[Path, dict]:
        source = json.loads(self.source.read_bytes())
        source["git_commit"] = "3" * 40
        self.source.write_bytes(encode(source))
        replacement, response = self.package("replacement", Path("/usr/bin/true"))
        self.assertEqual(self.bundle.stat().st_size, replacement.stat().st_size)
        self.assertNotEqual(self.first["archive_sha256"], response["archive_sha256"])
        self.cli("verify", replacement, "--expected-sha256", response["archive_sha256"])
        return replacement, response

    def race_after_archive_hash(self, operation: str, replacement: Path) -> dict:
        # Schedule real in-place input replacement immediately after the real SHA
        # calculation. Validation and installation themselves run unchanged via main().
        wrapper = '''
import importlib.util
from pathlib import Path
import sys
tool, original, replacement, expected, *arguments = sys.argv[1:]
specification = importlib.util.spec_from_file_location("race_packager", tool)
module = importlib.util.module_from_spec(specification)
specification.loader.exec_module(module)
original_hash = module.stream_hash
changed = False
def observed_hash(stream, size):
    global changed
    result = original_hash(stream, size)
    if not changed and result == expected:
        changed = True
        with Path(original).open("r+b") as destination:
            destination.write(Path(replacement).read_bytes())
        print("RACE_REPLACEMENT_COMPLETED", file=sys.stderr, flush=True)
    return result
module.stream_hash = observed_hash
sys.argv = [tool, *arguments]
status = module.main()
if not changed:
    raise RuntimeError("race did not reach the observed archive-hash boundary")
raise SystemExit(status)
'''
        arguments = [operation, str(self.bundle), "--expected-sha256", self.first["archive_sha256"]]
        if operation == "install":
            arguments.extend(("--root", str(self.root)))
        result = subprocess.run([sys.executable, "-B", "-c", wrapper, str(TOOL), str(self.bundle),
                                 str(replacement), self.first["archive_sha256"], *arguments],
                                capture_output=True, timeout=15, check=False)
        self.assertEqual(result.returncode, 0, (result.stdout.decode(), result.stderr.decode()))
        self.assertEqual(result.stderr, b"RACE_REPLACEMENT_COMPLETED\n")
        self.assertEqual(self.bundle.read_bytes(), replacement.read_bytes(), "real input replacement did not occur")
        return json.loads(result.stdout)

    def test_deterministic_repack_exact_bytes_and_archive_hash(self) -> None:
        repeated, response = self.package("repeated", Path("/usr/bin/true"))
        self.assertEqual(self.bundle.read_bytes(), repeated.read_bytes())
        self.assertEqual(self.first["archive_sha256"], response["archive_sha256"])
        self.assertEqual(response["archive_sha256"], sha(repeated.read_bytes()))

    def test_checksum_corruption_refused_before_root_creation(self) -> None:
        changed = bytearray(self.bundle.read_bytes())
        changed[1024] ^= 1
        self.bundle.write_bytes(changed)
        self.cli("install", self.bundle, "--expected-sha256", self.first["archive_sha256"],
                 "--root", self.root, refusal="archive SHA-256 mismatch")
        self.assertFalse(self.root.exists())

    def test_verify_binds_expected_digest_despite_same_size_replacement_after_hash(self) -> None:
        replacement, second = self.same_size_replacement()
        verified = self.race_after_archive_hash("verify", replacement)
        self.assertEqual(verified["archive_sha256"], self.first["archive_sha256"])
        self.assertEqual(verified["artifact_id"], self.first["artifact_id"],
                         "unauthorized replacement B was accepted under A expected SHA-256")
        self.assertNotEqual(verified["artifact_id"], second["artifact_id"])

    def test_install_binds_expected_digest_despite_same_size_replacement_after_hash(self) -> None:
        replacement, second = self.same_size_replacement()
        response = self.race_after_archive_hash("install", replacement)
        self.assertEqual(response["archive_sha256"], self.first["archive_sha256"])
        self.assertEqual(response["artifact_id"], self.first["artifact_id"],
                         "unauthorized replacement B was installed under A expected SHA-256")
        self.assertFalse((self.root / "releases" / second["artifact_id"]).exists())
        self.cli("inspect-installed", self.first["artifact_id"], "--root", self.root)

    def test_create_refuses_missing_declared_dependency_notice(self) -> None:
        self.inventory.write_bytes(encode(self.inventory_with_references(["cargo/synthetic-dependency-1.0.0/LICENSE"])))
        bundle, _ = self.package("missing-dependency-notice", Path("/usr/bin/true"),
                                 refusal="declared dependency notice missing from bundle")
        self.assertFalse(bundle.exists())

    def test_verify_refuses_missing_declared_dependency_notice(self) -> None:
        inventory = self.inventory_with_references(["cargo/synthetic-dependency-1.0.0/LICENSE"])
        bundle, checksum = self.rewrite(self.entries_with_inventory(inventory))
        self.cli("verify", bundle, "--expected-sha256", checksum,
                 refusal="declared dependency notice missing from bundle")

    def test_install_refuses_missing_declared_dependency_notice_before_root_creation(self) -> None:
        inventory = self.inventory_with_references(["cargo/synthetic-dependency-1.0.0/LICENSE"])
        self.refuse_install(self.entries_with_inventory(inventory), "declared dependency notice missing from bundle")

    def test_selection_refuses_self_consistent_missing_dependency_notice_metadata(self) -> None:
        self.install_first()
        release = self.root / "releases" / self.first["artifact_id"]
        inventory = self.inventory_with_references(["cargo/synthetic-dependency-1.0.0/LICENSE"])
        entries = self.entries_with_inventory(inventory)
        manifest_data = entries[0][1]
        (release / "manifest.json").write_bytes(manifest_data)
        (release / "provenance/dependency-inventory.json").write_bytes(encode(inventory))
        receipt = json.loads((release / "install.json").read_bytes())
        receipt["manifest_sha256"] = sha(manifest_data)
        (release / "install.json").write_bytes(encode(receipt))
        self.cli("activate", self.first["artifact_id"], "--root", self.root,
                 refusal="declared dependency notice missing from bundle")
        self.assertFalse((self.root / "current").is_symlink())

    def test_malformed_and_unsafe_dependency_notice_references_refused(self) -> None:
        cases = [(None, "missing or malformed dependency notice references"),
                 ("LICENSE", "missing or malformed dependency notice references"),
                 ([1], "invalid path length"), (["/LICENSE"], "unsafe archive path"),
                 (["cargo/../LICENSE"], "unsafe archive path"),
                 (["LICENSE", "LICENSE"], "duplicate dependency notice reference"),
                 (["x" * 201], "invalid path length")]
        for index, (references, reason) in enumerate(cases):
            with self.subTest(references=references):
                inventory = self.inventory_with_references(references)
                self.inventory.write_bytes(encode(inventory))
                bundle, _ = self.package(f"malformed-reference-{index}", Path("/usr/bin/true"), refusal=reason)
                self.assertFalse(bundle.exists())
        inventory = self.inventory_with_references([])
        inventory["packages"][0].pop("notice_files")
        self.inventory.write_bytes(encode(inventory))
        self.package("missing-reference-field", Path("/usr/bin/true"),
                     refusal="missing or malformed dependency notice references")

    def test_declared_dependency_notice_install_preserves_inventory_scope_and_gaps(self) -> None:
        reference = "cargo/synthetic-dependency-1.0.0/LICENSE"
        notice = self.notices / reference
        notice.parent.mkdir(parents=True)
        notice.write_bytes(b"synthetic dependency notice; not a distribution license review\n")
        inventory = self.inventory_with_references([reference])
        inventory["packages"][0]["license"] = "synthetic metadata retained unchanged"
        self.inventory.write_bytes(encode(inventory))
        bundle, response = self.package("declared-dependency-notice", Path("/usr/bin/true"))
        verified = self.cli("verify", bundle, "--expected-sha256", response["archive_sha256"])
        for key in ("scope", "distribution_gaps"):
            self.assertEqual(verified["manifest"]["dependency_inventory"][key], inventory[key])
        result = self.cli("install", bundle, "--expected-sha256", response["archive_sha256"], "--root", self.root)
        release = Path(result["release"])
        self.assertEqual((release / "provenance/dependency-inventory.json").read_bytes(), encode(inventory))
        self.assertEqual((release / "notices" / reference).read_bytes(), notice.read_bytes())
        self.cli("activate", response["artifact_id"], "--root", self.root)
        self.cli("inspect-installed", response["artifact_id"], "--root", self.root)

    def test_payload_digest_mismatch_even_with_updated_archive_digest(self) -> None:
        entries = self.entries()
        index = next(i for i, (item, _) in enumerate(entries) if item.name == "README.txt")
        item, payload = entries[index]
        entries[index] = (item, bytes([payload[0] ^ 1]) + payload[1:])
        self.refuse_install(entries, "file SHA-256 mismatch")

    def test_unsupported_manifest_revision_refused(self) -> None:
        entries = self.entries()
        value = json.loads(entries[0][1])
        value["schema_version"] = 2
        entries[0] = (entries[0][0], encode(value))
        self.refuse_install(entries, "unsupported bundle manifest revision")

    def test_unknown_required_metadata_refused(self) -> None:
        entries = self.entries()
        value = json.loads(entries[0][1])
        value["post_install"] = "never execute this"
        entries[0] = (entries[0][0], encode(value))
        self.refuse_install(entries, "unknown or missing metadata")

    def test_unsupported_storage_contract_refused(self) -> None:
        entries = self.entries()
        value = json.loads(entries[0][1])
        value["compatibility"]["record"] = "always"
        entries[0] = (entries[0][0], encode(value))
        self.refuse_install(entries, "unsupported storage/authority compatibility")

    def test_duplicate_json_key_and_excessive_nesting_refused(self) -> None:
        entries = self.entries()
        entries[0] = (entries[0][0], entries[0][1].replace(b'"schema_version":1', b'"schema_version":1,"schema_version":1'))
        self.refuse_install(entries, "duplicate JSON key")
        entries = self.entries()
        entries[0] = (entries[0][0], b'{"deep":' + b'[' * 33 + b'0' + b']' * 33 + b'}')
        self.refuse_install(entries, "JSON nesting exceeds limit")

    def test_nonfinite_metadata_refused_before_archive_creation(self) -> None:
        self.inventory.write_bytes(b'{"schema_version":1,"scope":"fixture","packages":[],"distribution_gaps":[1e400]}')
        path, _ = self.package("nonfinite", Path("/usr/bin/true"), refusal="nonfinite JSON")
        self.assertFalse(path.exists())

    def test_missing_upstream_notice_refused_before_archive_creation(self) -> None:
        (self.notices / "project/third-party__save-toolkit-LICENSE.txt").unlink()
        path, _ = self.package("missing-notice", Path("/usr/bin/true"),
                               refusal="notices directory must include project/upstream notices")
        self.assertFalse(path.exists())

    def test_input_script_and_notice_link_refused_without_execution(self) -> None:
        marker = self.base / "script-ran"
        script = self.base / "untrusted-script"
        script.write_text(f"#!/bin/sh\ntouch '{marker}'\n", encoding="utf-8")
        path, _ = self.package("script", script, refusal="binary is not Linux x86-64 ELF")
        self.assertFalse(path.exists())
        self.assertFalse(marker.exists())
        (self.notices / "linked-license").symlink_to(self.notices / "LICENSE")
        path, _ = self.package("notice-link", Path("/usr/bin/true"), refusal="notice contains a link or special file")
        self.assertFalse(path.exists())

    def test_traversal_absolute_and_duplicate_paths_refused(self) -> None:
        for name in ("../escape", "/escape", "notices/../../escape", "manifest.json"):
            with self.subTest(name=name):
                entries = self.entries()
                entries[1][0].name = name
                self.refuse_install(entries, "duplicate archive path" if name == "manifest.json" else "unsafe archive path")
                self.assertFalse((self.base / "escape").exists())

    def test_links_special_files_and_extension_headers_refused(self) -> None:
        for kind in (tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.FIFOTYPE, tarfile.CHRTYPE,
                     tarfile.BLKTYPE, tarfile.DIRTYPE, tarfile.XHDTYPE):
            with self.subTest(kind=kind):
                entries = self.entries()
                entries[1][0].type = kind
                entries[1][0].linkname = "../../escape"
                self.refuse_install(entries, "unsupported archive file type")

    def test_mode_and_size_mismatch_refused(self) -> None:
        entries = self.entries()
        entries[1][0].mode = 0o777
        self.refuse_install(entries, "archive file mode mismatch")
        entries = self.entries()
        item, payload = entries[1]
        entries[1] = (item, payload + b"x")
        self.refuse_install(entries, "archive file size/order mismatch")

    def test_archive_trailer_and_bound_refused(self) -> None:
        for extra in (bytes(512), b"x" * 512):
            with self.subTest(extra=extra[:1]):
                path = self.base / "trailer.tar"
                data = self.bundle.read_bytes() + extra
                path.write_bytes(data)
                self.cli("install", path, "--expected-sha256", sha(data), "--root", self.root,
                         refusal="noncanonical archive trailer")
                self.assertFalse(self.root.exists())
        path = self.base / "oversized.tar"
        with path.open("wb") as stream:
            stream.truncate(96 * 1024 * 1024 + 512)
        self.cli("verify", path, "--expected-sha256", "0" * 64, refusal="file exceeds size limit")

    def test_claimed_oversized_file_refused_before_payload_read(self) -> None:
        entries = self.entries()
        item, _ = next((item, payload) for item, payload in entries if item.name == "bin/save")
        item.size = 64 * 1024 * 1024 + 1
        original = self.bundle.read_bytes()
        with tarfile.open(self.bundle, "r:") as archive:
            offset = archive.getmember("bin/save").offset
        data = original[:offset] + item.tobuf(format=tarfile.USTAR_FORMAT) + original[offset + 512:]
        path = self.base / "oversized-header.tar"
        path.write_bytes(data)
        self.cli("install", path, "--expected-sha256", sha(data), "--root", self.root,
                 refusal="archive file size exceeds limit")
        self.assertFalse(self.root.exists())

    def test_file_directory_prefix_collision_refused_before_install(self) -> None:
        entries = self.entries()
        manifest = json.loads(entries[0][1])
        for name in ("notices/conflict", "notices/conflict/child"):
            manifest["files"].append({"path": name, "size": 1, "sha256": sha(b"x"), "mode": "0644"})
        manifest["files"].sort(key=lambda item: item["path"])
        entries[0] = (entries[0][0], encode(manifest))
        self.refuse_install(entries, "file/directory path collision")

    def test_install_is_repeatable_and_separate_from_selection(self) -> None:
        first = self.install_first()
        self.assertEqual(first["status"], "installed")
        self.assertFalse((self.root / "current").exists())
        binary = Path(first["binary"])
        self.assertEqual(binary.stat().st_mode & 0o7777, 0o755)
        self.assertEqual(sha(binary.read_bytes()), sha(Path("/usr/bin/true").read_bytes()))
        repeated = self.install_first()
        self.assertEqual(repeated["status"], "already_present")
        self.cli("inspect-installed", self.first["artifact_id"], "--root", self.root)
        self.assertFalse(list((self.root / "releases").glob(".staging-*")))

    def test_upgrade_rollback_preserve_previous_bytes_and_external_canaries(self) -> None:
        self.install_first()
        canary = self.base / "operator-config.json"
        canary.write_bytes(b"synthetic external config\n")
        evidence = self.base / "operator-evidence.json"
        evidence.write_bytes(b"synthetic external evidence\n")
        originals = {canary: canary.read_bytes(), evidence: evidence.read_bytes()}
        self.cli("activate", self.first["artifact_id"], "--root", self.root)
        previous_bytes = (self.root / "current/bin/save").read_bytes()
        bundle, second = self.package("second", Path("/usr/bin/printf"))
        self.assertNotEqual(self.first["artifact_id"], second["artifact_id"])
        self.cli("install", bundle, "--expected-sha256", second["archive_sha256"], "--root", self.root)
        self.assertEqual(os.readlink(self.root / "current"), "releases/" + self.first["artifact_id"])
        selected = self.cli("activate", second["artifact_id"], "--root", self.root)
        self.assertEqual(selected["previous_artifact_id"], self.first["artifact_id"])
        self.assertNotEqual(previous_bytes, (self.root / "current/bin/save").read_bytes())
        self.cli("rollback", self.first["artifact_id"], "--root", self.root)
        self.assertEqual(previous_bytes, (self.root / "current/bin/save").read_bytes())
        self.assertEqual(len(list((self.root / "releases").iterdir())), 2)
        for path, original in originals.items():
            self.assertEqual(path.read_bytes(), original)
        self.assertFalse(list(self.root.glob(".current-*")))

    def test_selection_revalidates_tampered_hash_modes_and_links(self) -> None:
        self.install_first()
        ident = self.first["artifact_id"]
        binary = self.root / "releases" / ident / "bin/save"
        original = binary.read_bytes()
        binary.write_bytes(original[:-1] + bytes([original[-1] ^ 1]))
        self.cli("activate", ident, "--root", self.root, refusal="installed file SHA-256 mismatch")
        self.assertFalse((self.root / "current").is_symlink())
        binary.write_bytes(original)
        binary.chmod(0o777)
        self.cli("activate", ident, "--root", self.root, refusal="installed file mode mismatch")
        binary.unlink()
        binary.symlink_to("/usr/bin/true")
        self.cli("activate", ident, "--root", self.root, refusal="Too many levels of symbolic links")
        self.assertFalse((self.root / "current").is_symlink())

    def test_unrelated_current_file_and_escaping_pointer_preserved(self) -> None:
        self.install_first()
        ident = self.first["artifact_id"]
        current = self.root / "current"
        current.write_bytes(b"external existing file\n")
        self.cli("activate", ident, "--root", self.root, refusal="current is an unrelated file")
        self.assertEqual(current.read_bytes(), b"external existing file\n")
        current.unlink()
        for pointer in ("../escape", "/usr/bin", "releases/../../escape"):
            with self.subTest(pointer=pointer):
                current.symlink_to(pointer)
                self.cli("activate", ident, "--root", self.root, refusal="current pointer escapes version layout")
                self.assertEqual(os.readlink(current), pointer)
                current.unlink()

    def test_existing_tampered_release_is_never_overwritten(self) -> None:
        response = self.install_first()
        binary = Path(response["binary"])
        binary.write_bytes(b"tampered")
        self.cli("install", self.bundle, "--expected-sha256", self.first["archive_sha256"],
                 "--root", self.root, refusal="installed file size mismatch")
        self.assertEqual(binary.read_bytes(), b"tampered")

    def test_root_symlink_refused_without_external_changes(self) -> None:
        external = self.base / "external"
        external.mkdir()
        self.root.symlink_to(external, target_is_directory=True)
        self.cli("install", self.bundle, "--expected-sha256", self.first["archive_sha256"],
                 "--root", self.root, refusal="directory path contains a symlink")
        self.assertEqual(list(external.iterdir()), [])

    def test_installed_extra_paths_and_hardlinks_refused_before_selection(self) -> None:
        self.install_first()
        ident = self.first["artifact_id"]
        release = self.root / "releases" / ident
        extra = release / "unexpected"
        extra.mkdir()
        self.cli("activate", ident, "--root", self.root, refusal="installed layout mismatch")
        extra.rmdir()
        os.link(release / "bin/save", self.base / "external-hardlink")
        self.cli("activate", ident, "--root", self.root, refusal="input must be an ordinary regular file")
        self.assertFalse((self.root / "current").is_symlink())


def main() -> int:
    global TOOL
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tool", type=Path, default=TOOL, help="explicit alternate tool for a negative gate probe")
    options, tests = parser.parse_known_args()
    TOOL = options.tool.resolve(strict=True)
    result = unittest.main(argv=[sys.argv[0], *tests], verbosity=2, exit=False).result
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    raise SystemExit(main())
