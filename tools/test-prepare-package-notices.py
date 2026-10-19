"""Checksum/copy race regressions using synthetic dependency notices only."""
from __future__ import annotations

import builtins
import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location("notices", Path(__file__).with_name("prepare-package-notices.py"))
notices = importlib.util.module_from_spec(spec)
spec.loader.exec_module(notices)


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


class NoticeTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.repo = self.root / "repo"
        self.vendor = self.root / "vendor"
        self.output = self.root / "output"
        self.package = self.vendor / "example-1.0.0"
        self.project_paths = ["LICENSE", "third-party/save-toolkit-LICENSE.txt",
            "crates/workbench-core/resources/dashboard-hygiene/LICENSE",
            "crates/workbench-core/resources/error-budget/LICENSE"]
        for path in self.project_paths:
            self.write(self.repo / path, b"project notice\n")
        self.write(self.repo / "Cargo.lock", (
            'version = 4\n[[package]]\nname = "example"\nversion = "1.0.0"\n'
            'source = "registry+https://github.com/rust-lang/crates.io-index"\n'
            'checksum = "' + '0' * 64 + '"\n').encode())
        self.metadata = b'[package]\nname="example"\nversion="1.0.0"\nlicense="MIT"\n'
        self.notice = b"reviewed dependency notice\n"
        self.write(self.package / "Cargo.toml", self.metadata)
        self.write(self.package / "LICENSE", self.notice)
        self.write(self.package / ".cargo-checksum.json", json.dumps({
            "package": "0" * 64, "files": {"Cargo.toml": sha(self.metadata),
                                         "LICENSE": sha(self.notice)}}).encode())
        self.write(self.repo / "web/package-lock.json", b"{}\n")
        frontend = self.repo / "third-party/frontend-notices"
        self.frontend_notice = frontend / "example/LICENSE"
        self.write(self.frontend_notice, self.notice)
        self.write(frontend / "inventory.json", json.dumps({"lock_sha256": sha(b"{}\n"),
            "origin": "synthetic test", "packages": [{"name": "example", "version": "1.0.0",
            "integrity": "synthetic", "license": "MIT", "notices": [
                {"path": "example/LICENSE", "sha256": sha(self.notice)}]}]}).encode())

    def write(self, path, raw):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(raw)

    def prepare(self):
        return notices.prepare(self.repo, self.vendor, self.output)

    @contextlib.contextmanager
    def replace_after_read(self, path, replacement):
        """Race a real source rewrite just after its first opened read completes."""
        original_open = io.open
        replaced = False

        @contextlib.contextmanager
        def raced_stream(*args, **kwargs):
            nonlocal replaced
            with original_open(*args, **kwargs) as stream:
                yield stream
            if not replaced:
                replaced = True
                with original_open(path, "wb") as stream:
                    stream.write(replacement)

        def open_file(file, *args, **kwargs):
            if not isinstance(file, int) and Path(file) == path and not replaced:
                return raced_stream(file, *args, **kwargs)
            return original_open(file, *args, **kwargs)

        with patch.object(io, "open", open_file), patch.object(builtins, "open", open_file):
            yield
        self.assertTrue(replaced, "race hook did not run")

    def test_normal_corpus_is_exact_and_counted(self):
        result = self.prepare()
        self.assertEqual(result["packages"], 2)
        self.assertEqual(result["notice_bytes"], 4 * len(b"project notice\n") + 2 * len(self.notice))
        self.assertEqual((self.output / "notices/cargo/example-1.0.0/LICENSE").read_bytes(), self.notice)

    def test_cargo_notice_copy_uses_the_verified_bytes(self):
        with self.replace_after_read(self.package / "LICENSE", b"x" * len(self.notice)):
            self.prepare()
        self.assertEqual((self.output / "notices/cargo/example-1.0.0/LICENSE").read_bytes(), self.notice)

    def test_frontend_notice_copy_uses_the_verified_bytes(self):
        with self.replace_after_read(self.frontend_notice, b"x" * len(self.notice)):
            self.prepare()
        self.assertEqual((self.output / "notices/frontend/example/LICENSE").read_bytes(), self.notice)

    def test_metadata_parse_uses_the_verified_bytes(self):
        with self.replace_after_read(self.package / "Cargo.toml", self.metadata.replace(b'MIT', b'BSD')):
            self.prepare()
        inventory = json.loads((self.output / "inventory.json").read_bytes())
        self.assertEqual(inventory["packages"][0]["license"], "MIT")

    def test_corpus_limit_prevents_inventory_publication(self):
        with patch.object(notices, "MAX_TOTAL", len(b"project notice\n")):
            with self.assertRaisesRegex(ValueError, "corpus too large"):
                self.prepare()
        self.assertFalse((self.output / "inventory.json").exists())

    def test_checksum_failure_prevents_inventory_publication(self):
        (self.package / "LICENSE").write_bytes(b"modified notice")
        with self.assertRaisesRegex(ValueError, "checksum mismatch"):
            self.prepare()
        self.assertFalse((self.output / "inventory.json").exists())


if __name__ == "__main__":
    unittest.main()
