"""Prepare checksum-bound development-bundle notices without executing dependencies.

Rust inputs come from the existing checksum-verified offline vendor tree. Frontend
notices come from the checked-in inventory captured from the pinned installation.
This inventories locked packages; it does not certify redistribution compliance.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import shutil
import tomllib


MAX_NOTICE = 4 * 1024 * 1024
MAX_TOTAL = 32 * 1024 * 1024
NOTICE_NAME = re.compile(r"^(?:LICEN[CS]E|COPYING|COPYRIGHT|NOTICE)(?:[._-].*)?$", re.I)


def digest(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def regular(root: Path, relative: str) -> Path:
    parts = relative.split("/")
    if not relative or PurePosixPath(relative).is_absolute() or any(p in ("", ".", "..") for p in parts):
        raise ValueError("Unsafe notice path")
    result = root
    for part in parts:
        result /= part
        if result.is_symlink():
            raise ValueError("Symlink notice path")
    if not result.is_file() or result.stat().st_size > MAX_NOTICE:
        raise ValueError("Nonregular or oversized notice")
    return result


def prepare(repo: Path, vendor: Path, destination: Path) -> dict:
    if destination.exists():
        raise ValueError("Notice output must be a fresh directory")
    destination.mkdir(parents=True)
    notices = destination / "notices"
    notices.mkdir()
    total = 0

    def copy(source: Path, relative: str, expected: str | None = None) -> str:
        nonlocal total
        if source.is_symlink() or not source.is_file() or source.stat().st_size > MAX_NOTICE:
            raise ValueError("Invalid notice input")
        actual = digest(source)
        if expected is not None and actual != expected:
            raise ValueError("Notice checksum mismatch")
        total += source.stat().st_size
        if total > MAX_TOTAL:
            raise ValueError("Notice corpus too large")
        # Output paths are composed from validated package/checksum paths below.
        target = notices / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        if target.exists():
            raise ValueError("Duplicate notice output")
        shutil.copyfile(source, target)
        target.chmod(0o644)
        return relative

    for path in ("LICENSE", "third-party/save-toolkit-LICENSE.txt",
                 "crates/workbench-core/resources/dashboard-hygiene/LICENSE",
                 "crates/workbench-core/resources/error-budget/LICENSE"):
        relative = "LICENSE" if path == "LICENSE" else "project/" + path.replace("/", "__")
        copy(regular(repo, path), relative)

    lock_path = repo / "Cargo.lock"
    lock = tomllib.loads(lock_path.read_text())
    packages = []
    gaps = ["Development inventory has not received a release-level dependency/license clearance.",
            "Frontend notices were captured from the pinned installation; cached npm archives were unavailable for independent integrity revalidation.",
            "Rust standard-library/compiler and external system-library notices are outside this lockfile inventory and still require release-level coverage."]
    for package in lock["package"]:
        if "source" not in package:
            continue
        if package["source"] != "registry+https://github.com/rust-lang/crates.io-index":
            raise ValueError("Unsupported dependency source")
        identity = f'{package["name"]}-{package["version"]}'
        if not re.fullmatch(r"[A-Za-z0-9_.+-]+", identity):
            raise ValueError("Unsafe package identity")
        directory = vendor / identity
        checksums_path = regular(directory, ".cargo-checksum.json")
        checksums = json.loads(checksums_path.read_text())
        if checksums["package"] != package["checksum"]:
            raise ValueError("Vendor package checksum mismatch")
        metadata_path = regular(directory, "Cargo.toml")
        if digest(metadata_path) != checksums["files"]["Cargo.toml"]:
            raise ValueError("Vendor metadata checksum mismatch")
        metadata = tomllib.loads(metadata_path.read_text())["package"]
        if metadata["name"] != package["name"] or metadata["version"] != package["version"]:
            raise ValueError("Vendor package identity mismatch")
        selected = {path for path in checksums["files"] if NOTICE_NAME.fullmatch(PurePosixPath(path).name)}
        if metadata.get("license-file"):
            selected.add(metadata["license-file"])
        if len(selected) > 256:
            raise ValueError("Too many package notices")
        files = []
        for path in sorted(selected):
            source = regular(directory, path)
            files.append(copy(source, f"cargo/{identity}/{path}", checksums["files"][path]))
        if not metadata.get("license") and not metadata.get("license-file"):
            gaps.append("Missing declared license: " + identity)
        if not files:
            gaps.append("No available license text: " + identity)
        packages.append({"ecosystem": "cargo", "name": package["name"], "version": package["version"],
                         "archive_sha256": package["checksum"], "license": metadata.get("license"),
                         "license_file": metadata.get("license-file"), "notice_files": files})

    frontend = repo / "third-party/frontend-notices"
    frontend_index = json.loads(regular(frontend, "inventory.json").read_text())
    if frontend_index["lock_sha256"] != digest(repo / "web/package-lock.json"):
        raise ValueError("Frontend notice inventory does not match lockfile")
    for package in frontend_index["packages"]:
        files = []
        for entry in package["notices"]:
            path = entry["path"]
            files.append(copy(regular(frontend, path), "frontend/" + path, entry["sha256"]))
        packages.append({"ecosystem": "npm", "name": package["name"], "version": package["version"],
                         "lock_integrity": package["integrity"], "license": package["license"],
                         "notice_files": files, "notice_origin": frontend_index["origin"]})
        if not files:
            gaps.append("No available frontend license text: " + package["name"])
    inventory = {"schema_version": 1,
                 "scope": "All registry packages in Cargo.lock (including dev/platform packages), non-dev npm lockfile packages and selected Tailwind CSS/Vite asset contributors; not a linked-binary or complete frontend build-tool SBOM.",
                 "packages": packages, "distribution_gaps": gaps}
    (destination / "inventory.json").write_text(json.dumps(inventory, indent=2, sort_keys=True) + "\n")
    return {"packages": len(packages), "notice_bytes": total, "distribution_gaps": gaps}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", required=True, type=Path)
    parser.add_argument("--vendor", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    options = parser.parse_args()
    print(json.dumps(prepare(options.repo, options.vendor, options.output)))


if __name__ == "__main__":
    main()
