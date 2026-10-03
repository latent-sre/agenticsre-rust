#!/usr/bin/env python3
"""Create, verify, install and select a bounded Linux development bundle.

Python >=3.11, standard library only. Validation never runs bundled code.
Checksums bind supplied bytes; they do not authenticate the build or its operator.
"""
from __future__ import annotations

import argparse
from contextlib import contextmanager
import fcntl
import hashlib
import json
import math
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import sys
import tarfile
import tempfile
import uuid

REVISION = 1
MAX_ARCHIVE = 96 * 1024 * 1024
MAX_BINARY = 64 * 1024 * 1024
MAX_DATA_FILE = 4 * 1024 * 1024
MAX_JSON = 1024 * 1024
MAX_FILES = 2048
MAX_DIRECTORIES = 4096
MAX_SOURCE_FILES = 8192
MAX_PATH = 200
CHUNK = 1024 * 1024
SOURCE = "provenance/source-manifest.json"
BUILD = "provenance/build-provenance.json"
INVENTORY = "provenance/dependency-inventory.json"
FIXED = {"bin/save", SOURCE, BUILD, INVENTORY, "README.txt"}
REQUIRED_NOTICES = {"notices/LICENSE", "notices/project/third-party__save-toolkit-LICENSE.txt",
                    "notices/project/crates__workbench-core__resources__dashboard-hygiene__LICENSE",
                    "notices/project/crates__workbench-core__resources__error-budget__LICENSE"}
COMPATIBILITY = {"record": "never", "ui_history": "session-only",
                 "storage_migrations": "none", "external_config": "untouched",
                 "authority": "invoking-account"}
SHA = re.compile(r"[0-9a-f]{64}\Z")
GIT = re.compile(r"[0-9a-f]{40}\Z")
VERSION = re.compile(r"[0-9A-Za-z][0-9A-Za-z.+-]{0,63}\Z")
IDENTITY = re.compile(r"save-[0-9A-Za-z][0-9A-Za-z.+-]{0,63}-src-[0-9a-f]{16}-bin-[0-9a-f]{16}\Z")
COMPONENT = re.compile(r"[0-9A-Za-z_.+@-]+\Z")


class Refused(ValueError):
    """Invalid bytes or an unsafe operation; nothing is selected on refusal."""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise Refused(message)


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def canonical(value: object) -> bytes:
    return (json.dumps(value, ensure_ascii=True, allow_nan=False, sort_keys=True,
                       separators=(",", ":")) + "\n").encode("ascii")


def sha(value: object) -> str:
    require(isinstance(value, str) and SHA.fullmatch(value) is not None, "invalid SHA-256")
    return value


def path_name(value: object) -> str:
    require(isinstance(value, str) and 0 < len(value) <= MAX_PATH, "invalid path length")
    parts = value.split("/")
    require(all(part not in ("", ".", "..") and len(part) <= 100
                and COMPONENT.fullmatch(part) for part in parts), "unsafe archive path")
    require(not PurePosixPath(value).is_absolute(), "absolute archive path")
    return value


def object_keys(value: object, required: set[str], optional: set[str] | None = None) -> dict:
    require(isinstance(value, dict), "expected JSON object")
    require(required <= value.keys() and value.keys() <= required | (optional or set()),
            "unknown or missing metadata")
    return value


def bounded_json(data: bytes) -> dict:
    require(len(data) <= MAX_JSON, "JSON metadata exceeds limit")
    # Bound nesting before the decoder allocates nested containers.
    depth = 0
    quoted = False
    escaped = False
    for byte in data:
        if quoted:
            if escaped:
                escaped = False
            elif byte == 92:
                escaped = True
            elif byte == 34:
                quoted = False
        elif byte == 34:
            quoted = True
        elif byte in (91, 123):
            depth += 1
            require(depth <= 32, "JSON nesting exceeds limit")
        elif byte in (93, 125):
            depth -= 1

    def unique(pairs: list[tuple[str, object]]) -> dict:
        result = {}
        for key, value in pairs:
            require(key not in result, "duplicate JSON key")
            result[key] = value
        return result

    def finite(number: str) -> float:
        value = float(number)
        require(math.isfinite(value), "nonfinite JSON")
        return value

    def integer(number: str) -> int:
        require(len(number.lstrip("-")) <= 20, "JSON integer exceeds limit")
        return int(number)

    try:
        value = json.loads(data, object_pairs_hook=unique, parse_float=finite, parse_int=integer,
                           parse_constant=lambda _: (_ for _ in ()).throw(Refused("nonfinite JSON")))
    except (UnicodeDecodeError, json.JSONDecodeError, RecursionError) as error:
        raise Refused("invalid JSON metadata") from error
    require(isinstance(value, dict), "expected JSON metadata object")
    pending = [value]
    nodes = 0
    while pending:
        item = pending.pop()
        nodes += 1
        require(nodes <= 100_000, "JSON object count exceeds limit")
        if isinstance(item, dict):
            pending.extend(item.values())
        elif isinstance(item, list):
            pending.extend(item)
    return value


@contextmanager
def regular_file(path: Path, limit: int):
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, "rb") as stream:
        info = os.fstat(stream.fileno())
        require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1, "input must be an ordinary regular file")
        require(0 <= info.st_size <= limit, "file exceeds size limit")
        yield stream, info


@contextmanager
def archive_snapshot(path: Path):
    # The input may be writable by its owner. Hash, parse and extract only our
    # unnamed mode-0600 temporary copy, which another input owner cannot modify.
    with regular_file(path, MAX_ARCHIVE) as (source, info), tempfile.TemporaryFile(mode="w+b") as snapshot:
        remaining = info.st_size
        while remaining:
            block = source.read(min(CHUNK, remaining))
            require(bool(block), "archive changed while copying")
            snapshot.write(block)
            remaining -= len(block)
        require(not source.read(1), "archive changed while copying")
        snapshot.flush()
        snapshot.seek(0)
        yield snapshot, os.fstat(snapshot.fileno())


def read_data(path: Path, limit: int = MAX_JSON) -> bytes:
    with regular_file(path, limit) as (stream, info):
        data = stream.read(limit + 1)
        require(len(data) == info.st_size and len(data) <= limit, "file changed or exceeds limit")
        return data


def stream_hash(stream, size: int) -> str:
    result = hashlib.sha256()
    remaining = size
    while remaining:
        data = stream.read(min(CHUNK, remaining))
        require(bool(data), "truncated file")
        result.update(data)
        remaining -= len(data)
    return result.hexdigest()


def source_metadata(data: bytes) -> dict:
    value = object_keys(bounded_json(data), {"schema_version", "git_commit", "git_tree", "files"},
                        {"capture_note"})
    require(type(value["schema_version"]) is int and value["schema_version"] == REVISION,
            "unsupported source manifest revision")
    require(all(isinstance(value[key], str) and GIT.fullmatch(value[key])
                for key in ("git_commit", "git_tree")), "invalid git source identity")
    files = value["files"]
    require(isinstance(files, dict) and 0 < len(files) <= MAX_SOURCE_FILES, "invalid source file map")
    for name, checksum in files.items():
        path_name(name)
        sha(checksum)
    if "capture_note" in value:
        require(isinstance(value["capture_note"], str), "invalid source capture note")
    return value


def platform_metadata(value: object) -> dict:
    value = object_keys(value, {"os", "architecture", "libc", "environment"})
    require(value["os"] == "linux" and value["architecture"] == "x86_64", "unsupported platform")
    require(all(isinstance(item, str) and 0 < len(item) <= 4096 for item in value.values()),
            "invalid platform metadata")
    return value


def build_metadata(data: bytes, source_sha: str, binary_sha: str, binary_size: int) -> dict:
    value = object_keys(bounded_json(data), {"schema_version", "source_manifest_sha256", "binary_sha256",
                        "binary_size", "build_command", "rustc", "cargo", "platform"}, {"evidence"})
    require(type(value["schema_version"]) is int and value["schema_version"] == REVISION,
            "unsupported build provenance revision")
    require(value["source_manifest_sha256"] == source_sha, "source provenance digest mismatch")
    require(value["binary_sha256"] == binary_sha and type(value["binary_size"]) is int
            and value["binary_size"] == binary_size, "binary provenance digest/size mismatch")
    command = value["build_command"]
    require(isinstance(command, list) and 0 < len(command) <= 64
            and all(isinstance(item, str) and 0 < len(item) <= 4096 for item in command),
            "invalid build command metadata")
    require(all(isinstance(value[key], str) and 0 < len(value[key]) <= 4096 for key in ("rustc", "cargo")),
            "invalid toolchain metadata")
    platform_metadata(value["platform"])
    if "evidence" in value:
        require(isinstance(value["evidence"], str), "invalid build evidence metadata")
    return value


def inventory_metadata(data: bytes) -> dict:
    value = object_keys(bounded_json(data), {"schema_version", "scope", "packages", "distribution_gaps"})
    require(type(value["schema_version"]) is int and value["schema_version"] == REVISION,
            "unsupported inventory revision")
    require(isinstance(value["scope"], str) and 0 < len(value["scope"]) <= 4096, "missing inventory scope")
    require(isinstance(value["packages"], list) and len(value["packages"]) <= 4096
            and all(isinstance(item, dict) for item in value["packages"]), "invalid dependency inventory")
    require(isinstance(value["distribution_gaps"], list)
            and len(value["distribution_gaps"]) <= 4096, "invalid distribution gaps")
    for package in value["packages"]:
        references = package.get("notice_files")
        require(isinstance(references, list) and len(references) <= 256,
                "missing or malformed dependency notice references")
        paths = [path_name("notices/" + path_name(reference)) for reference in references]
        require(len(paths) == len(set(paths)), "duplicate dependency notice reference")
    return value


def verify_inventory_notices(inventory: dict, bundled_paths) -> None:
    for package in inventory["packages"]:
        for reference in package["notice_files"]:
            require("notices/" + reference in bundled_paths, "declared dependency notice missing from bundle")


def elf_header(data: bytes) -> None:
    require(len(data) >= 64 and data[:4] == b"\x7fELF" and data[4:7] == b"\x02\x01\x01"
            and int.from_bytes(data[16:18], "little") in (2, 3)
            and int.from_bytes(data[18:20], "little") == 62, "binary is not Linux x86-64 ELF")


def artifact_id(version: str, source_sha: str, binary_sha: str) -> str:
    require(VERSION.fullmatch(version) is not None, "invalid product version")
    return f"save-{version}-src-{source_sha[:16]}-bin-{binary_sha[:16]}"


def header(name: str, size: int, mode: int) -> bytes:
    info = tarfile.TarInfo(name)
    info.type = tarfile.REGTYPE
    info.size = size
    info.mode = mode
    info.mtime = info.uid = info.gid = 0
    try:
        return info.tobuf(format=tarfile.USTAR_FORMAT)
    except ValueError as error:
        raise Refused("path cannot be represented by bundle format") from error


def descriptor(name: str, data: bytes | Path) -> dict:
    path_name(name)
    mode = "0755" if name == "bin/save" else "0644"
    if isinstance(data, bytes):
        size, checksum = len(data), digest(data)
    else:
        limit = MAX_BINARY if name == "bin/save" else MAX_DATA_FILE
        with regular_file(data, limit) as (stream, info):
            size, checksum = info.st_size, stream_hash(stream, info.st_size)
    return {"path": name, "size": size, "sha256": checksum, "mode": mode}


def manifest_metadata(data: bytes) -> dict:
    value = object_keys(bounded_json(data), {"schema_version", "archive_format", "artifact_id", "product",
                        "source", "binary", "platform", "build_provenance", "dependency_inventory",
                        "compatibility", "files"})
    require(type(value["schema_version"]) is int and value["schema_version"] == REVISION,
            "unsupported bundle manifest revision")
    require(value["archive_format"] == "canonical-ustar-v1", "unsupported archive format")
    product = object_keys(value["product"], {"name", "version"})
    require(product["name"] == "save" and isinstance(product["version"], str), "invalid product identity")
    source = object_keys(value["source"], {"path", "sha256", "git_commit", "git_tree"})
    require(source["path"] == SOURCE and all(isinstance(source[key], str) and GIT.fullmatch(source[key])
            for key in ("git_commit", "git_tree")), "invalid source identity")
    binary = object_keys(value["binary"], {"path", "size", "sha256", "mode"})
    require(binary["path"] == "bin/save" and binary["mode"] == "0755", "invalid executable metadata")
    expected_id = artifact_id(product["version"], sha(source["sha256"]), sha(binary["sha256"]))
    require(value["artifact_id"] == expected_id, "artifact identity mismatch")
    platform_metadata(value["platform"])
    build = object_keys(value["build_provenance"], {"path", "sha256", "claim"})
    require(build["path"] == BUILD and build["claim"] == "supplied observations; not independently authenticated",
            "invalid build provenance claim")
    sha(build["sha256"])
    inventory = object_keys(value["dependency_inventory"], {"path", "sha256", "scope", "distribution_gaps"})
    require(inventory["path"] == INVENTORY, "invalid inventory path")
    sha(inventory["sha256"])
    require(value["compatibility"] == COMPATIBILITY, "unsupported storage/authority compatibility")
    files = value["files"]
    require(isinstance(files, list) and 0 < len(files) <= MAX_FILES, "file count exceeds limit")
    names = []
    directories = set()
    total = 0
    for item in files:
        item = object_keys(item, {"path", "size", "sha256", "mode"})
        name = path_name(item["path"])
        require(name in FIXED or name.startswith("notices/"), "unknown bundle file")
        require(item["mode"] == ("0755" if name == "bin/save" else "0644"), "file mode mismatch")
        require(type(item["size"]) is int and 0 <= item["size"] <=
                (MAX_BINARY if name == "bin/save" else MAX_DATA_FILE), "file size exceeds limit")
        sha(item["sha256"])
        total += item["size"]
        names.append(name)
        directories.update(str(parent) for parent in PurePosixPath(name).parents if str(parent) != ".")
        require(len(directories) <= MAX_DIRECTORIES, "directory count exceeds limit")
    require(names == sorted(set(names)), "duplicate or unordered manifest files")
    require(not set(names) & directories, "file/directory path collision")
    require(FIXED | REQUIRED_NOTICES <= set(names), "required bundle file missing")
    require(total <= MAX_ARCHIVE, "expanded bundle exceeds limit")
    entries = {item["path"]: item for item in files}
    for meta in (source, binary, build, inventory):
        actual = entries[meta["path"]]
        require(meta["sha256"] == actual["sha256"], "metadata file digest mismatch")
    require(binary == entries["bin/save"], "binary descriptor mismatch")
    return value


def verify_payload_metadata(manifest: dict, payloads: dict[str, bytes]) -> None:
    source = source_metadata(payloads[SOURCE])
    require(all(manifest["source"][key] == source[key] for key in ("git_commit", "git_tree")),
            "source manifest identity mismatch")
    build = build_metadata(payloads[BUILD], manifest["source"]["sha256"], manifest["binary"]["sha256"],
                           manifest["binary"]["size"])
    require(manifest["platform"] == build["platform"], "platform provenance mismatch")
    inventory = inventory_metadata(payloads[INVENTORY])
    verify_inventory_notices(inventory, {item["path"] for item in manifest["files"]})
    require(all(manifest["dependency_inventory"][key] == inventory[key]
                for key in ("scope", "distribution_gaps")), "inventory metadata mismatch")


def verify_archive(stream, expected_sha: str) -> tuple[dict, list[tuple[dict, int]], str]:
    sha(expected_sha)
    info = os.fstat(stream.fileno())
    require(1024 <= info.st_size <= MAX_ARCHIVE and info.st_size % 512 == 0, "archive size exceeds bounds")
    stream.seek(0)
    require(stream_hash(stream, info.st_size) == expected_sha, "archive SHA-256 mismatch")
    stream.seek(0)
    indexes = []
    manifest = None
    manifest_sha = ""
    metadata = {}
    names = []
    while True:
        block = stream.read(512)
        require(len(block) == 512, "truncated archive header")
        if block == bytes(512):
            require(stream.read(512) == bytes(512) and stream.tell() == info.st_size,
                    "noncanonical archive trailer")
            break
        require(len(names) <= MAX_FILES, "archive file count exceeds limit")
        require(block[257:265] == b"ustar\x0000" and block[156:157] == b"0", "unsupported archive file type")
        try:
            member = tarfile.TarInfo.frombuf(block, "ascii", "strict")
        except (tarfile.HeaderError, UnicodeError, ValueError) as error:
            raise Refused("invalid archive header") from error
        name = path_name(member.name)
        require(name not in names, "duplicate archive path")
        names.append(name)
        require(member.size >= 0 and member.size <= (MAX_BINARY if name == "bin/save" else MAX_DATA_FILE),
                "archive file size exceeds limit")
        mode = 0o755 if name == "bin/save" else 0o644
        require(member.mode == mode, "archive file mode mismatch")
        require(block == header(name, member.size, mode), "noncanonical archive header")
        offset = stream.tell()
        if manifest is None:
            require(name == "manifest.json" and member.size <= MAX_JSON, "manifest must be first")
            data = stream.read(member.size)
            require(len(data) == member.size, "truncated manifest")
            manifest = manifest_metadata(data)
            require(data == canonical(manifest), "noncanonical manifest encoding")
            manifest_sha = digest(data)
        else:
            position = len(indexes)
            require(position < len(manifest["files"]), "unlisted archive file")
            item = manifest["files"][position]
            require(name == item["path"] and member.size == item["size"], "archive file size/order mismatch")
            if name in (SOURCE, BUILD, INVENTORY):
                require(member.size <= MAX_JSON, "JSON metadata exceeds limit")
                data = stream.read(member.size)
                require(len(data) == member.size and digest(data) == item["sha256"], "file SHA-256 mismatch")
                metadata[name] = data
            else:
                if name == "bin/save":
                    elf_header(stream.read(min(member.size, 64)))
                    stream.seek(offset)
                require(stream_hash(stream, member.size) == item["sha256"], "file SHA-256 mismatch")
            indexes.append((item, offset))
        padding = (-member.size) % 512
        require(stream.read(padding) == bytes(padding), "noncanonical archive padding")
    require(manifest is not None and len(indexes) == len(manifest["files"]), "bundle file missing")
    require(os.fstat(stream.fileno()).st_size == info.st_size, "archive changed during validation")
    verify_payload_metadata(manifest, metadata)
    return manifest, indexes, manifest_sha


def safe_directory(path: Path, create: bool = False) -> Path:
    require(path.is_absolute() and ".." not in path.parts, "directory must be an explicit absolute path")
    current = Path(path.anchor)
    for part in path.parts[1:]:
        current /= part
        try:
            info = current.lstat()
        except FileNotFoundError:
            require(create, "directory does not exist")
            current.mkdir(mode=0o755)
            info = current.lstat()
        require(stat.S_ISDIR(info.st_mode), "directory path contains a symlink or non-directory")
    return path


def create(options) -> dict:
    binary = options.binary
    source_data = read_data(options.source_manifest)
    source = source_metadata(source_data)
    build_data = read_data(options.build_provenance)
    inventory_data = read_data(options.inventory)
    inventory = inventory_metadata(inventory_data)
    files: dict[str, bytes | Path] = {"bin/save": binary, SOURCE: source_data, BUILD: build_data,
                                    INVENTORY: inventory_data}
    safe_directory(options.notices_dir)
    pending = [options.notices_dir]
    directory_count = 0
    while pending:
        with os.scandir(pending.pop()) as entries:
            for entry in entries:
                path = Path(entry.path)
                name = "notices/" + path.relative_to(options.notices_dir).as_posix()
                path_name(name)
                info = entry.stat(follow_symlinks=False)
                require(stat.S_ISDIR(info.st_mode) or stat.S_ISREG(info.st_mode), "notice contains a link or special file")
                if stat.S_ISDIR(info.st_mode):
                    directory_count += 1
                    require(directory_count <= MAX_DIRECTORIES, "notice directory count exceeds limit")
                    pending.append(path)
                else:
                    require(len(files) < MAX_FILES - 1, "notice file count exceeds limit")
                    files[name] = path
    require(REQUIRED_NOTICES <= files.keys(), "notices directory must include project/upstream notices")
    verify_inventory_notices(inventory, files.keys())
    binary_item = descriptor("bin/save", binary)
    with regular_file(binary, MAX_BINARY) as (stream, _):
        elf_header(stream.read(64))
    source_sha = digest(source_data)
    build = build_metadata(build_data, source_sha, binary_item["sha256"], binary_item["size"])
    ident = artifact_id(options.product_version, source_sha, binary_item["sha256"])
    files["README.txt"] = (
        f"save Linux development bundle {ident}\n\n"
        "Development artifact; not a fully cleared production release.\n"
        "Support is limited to the Linux/x86-64/libc environment in manifest.json.\n"
        "The binary embeds its browser assets and both reviewed offline task bindings.\n"
        "Optional Python >=3.11 is needed for dashboard-hygiene and error-budget.\n"
        "Node is unnecessary at runtime. Restricted execution additionally requires the\n"
        "Git/ripgrep/Bubblewrap, fixed libraries and kernel prerequisites in the profile contract.\n"
        "Use an operator-trusted copy of tools/package-linux.py with Python >=3.11:\n"
        "  verify BUNDLE --expected-sha256 TRUSTED_SHA256\n"
        "  install BUNDLE --expected-sha256 TRUSTED_SHA256 --root /explicit/root\n"
        f"  activate {ident} --root /explicit/root\n"
        "  rollback RETAINED_ARTIFACT_ID --root /explicit/root\n"
        "Execute /explicit/root/current/bin/save; no global PATH or service is installed.\n"
        "Validation does not run archive code, product commands or post-install hooks.\n"
        "Versions are retained and never overwritten by the installer. Selection is atomic\n"
        "within the filesystem; power-loss durability is not claimed.\n"
        "record=never; UI history is session-only; there are no storage/config migrations.\n"
        "External config and evidence are never rewritten. Same-account modification remains possible.\n"
        "Dependency inventory scope and unresolved distribution gaps are in the manifest\n"
        "and provenance/dependency-inventory.json. Inventory does not prove binary linkage.\n"
        f"Distribution gaps (prepared inventory): {json.dumps(inventory['distribution_gaps'], ensure_ascii=True, sort_keys=True)}\n"
        "Build provenance records supplied observations, not independent build authentication.\n"
        "The expected SHA256 must come from a separate trusted record; no signature is supplied.\n"
        "Format: uncompressed canonical USTAR, <=96MiB; <=2048 payload files; binary <=64MiB;\n"
        "other files <=4MiB; <=4096 directories; JSON <=1MiB/depth32/100000 values; ASCII paths <=200 bytes.\n"
    ).encode("ascii")
    entries = [descriptor(name, files[name]) for name in sorted(files)]
    manifest = {"schema_version": REVISION, "archive_format": "canonical-ustar-v1", "artifact_id": ident,
                "product": {"name": "save", "version": options.product_version},
                "source": {"path": SOURCE, "sha256": source_sha,
                           "git_commit": source["git_commit"], "git_tree": source["git_tree"]},
                "binary": binary_item, "platform": build["platform"],
                "build_provenance": {"path": BUILD, "sha256": digest(build_data),
                                     "claim": "supplied observations; not independently authenticated"},
                "dependency_inventory": {"path": INVENTORY, "sha256": digest(inventory_data),
                                         "scope": inventory["scope"],
                                         "distribution_gaps": inventory["distribution_gaps"]},
                "compatibility": COMPATIBILITY, "files": entries}
    manifest_data = canonical(manifest)
    manifest_metadata(manifest_data)
    size = 1024 + sum(512 + ((item["size"] + 511) // 512) * 512 for item in entries)
    size += 512 + ((len(manifest_data) + 511) // 512) * 512
    require(size <= MAX_ARCHIVE, "archive size exceeds bounds")
    output = options.output
    safe_directory(output.parent)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(prefix=".bundle-", dir=output.parent, delete=False) as stream:
            temporary = Path(stream.name)
            all_entries = [{"path": "manifest.json", "size": len(manifest_data),
                            "mode": "0644", "sha256": digest(manifest_data)}, *entries]
            for item in all_entries:
                name = item["path"]
                data = manifest_data if name == "manifest.json" else files[name]
                stream.write(header(name, item["size"], int(item["mode"], 8)))
                checksum = hashlib.sha256()
                if isinstance(data, bytes):
                    stream.write(data)
                    checksum.update(data)
                else:
                    with regular_file(data, item["size"]) as (source_stream, info):
                        require(info.st_size == item["size"], "input file changed")
                        remaining = item["size"]
                        while remaining:
                            block = source_stream.read(min(CHUNK, remaining))
                            require(bool(block), "input file changed")
                            stream.write(block)
                            checksum.update(block)
                            remaining -= len(block)
                require(checksum.hexdigest() == item["sha256"], "input file digest changed")
                stream.write(bytes((-item["size"]) % 512))
            stream.write(bytes(1024))
            stream.flush()
            os.fsync(stream.fileno())
            os.fchmod(stream.fileno(), 0o644)
        with regular_file(temporary, MAX_ARCHIVE) as (stream, info):
            archive_sha = stream_hash(stream, info.st_size)
            verify_archive(stream, archive_sha)
        try:
            os.link(temporary, output, follow_symlinks=False)
            state = "created"
        except FileExistsError:
            with regular_file(output, MAX_ARCHIVE) as (stream, info):
                require(stream_hash(stream, info.st_size) == archive_sha, "output exists with different bytes")
            state = "already_present"
        return {"operation": "create", "status": state, "artifact_id": ident,
                "archive": str(output), "archive_sha256": archive_sha, "archive_size": size}
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


@contextmanager
def install_root(root: Path, create_root: bool = False):
    safe_directory(root, create=create_root)
    safe_directory(root / "releases", create=create_root)
    descriptor = os.open(root / ".install.lock", os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_NONBLOCK, 0o600)
    try:
        info = os.fstat(descriptor)
        require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1, "unsafe installer lock")
        fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        yield root
    finally:
        os.close(descriptor)


def verify_installed(release: Path, ident: str) -> tuple[dict, dict]:
    require(IDENTITY.fullmatch(ident) is not None, "invalid artifact ID")
    safe_directory(release)
    manifest_data = read_data(release / "manifest.json")
    require(stat.S_IMODE((release / "manifest.json").lstat().st_mode) == 0o644, "installed manifest mode mismatch")
    manifest = manifest_metadata(manifest_data)
    require(canonical(manifest) == manifest_data and manifest["artifact_id"] == ident,
            "installed manifest identity mismatch")
    receipt = object_keys(bounded_json(read_data(release / "install.json")),
                          {"schema_version", "artifact_id", "archive_sha256", "manifest_sha256", "compatibility"})
    require(type(receipt["schema_version"]) is int and receipt["schema_version"] == REVISION
            and receipt["artifact_id"] == ident and receipt["manifest_sha256"] == digest(manifest_data)
            and receipt["compatibility"] == COMPATIBILITY, "installed receipt mismatch")
    sha(receipt["archive_sha256"])
    require(stat.S_IMODE((release / "install.json").lstat().st_mode) == 0o644, "installed receipt mode mismatch")
    expected = {item["path"] for item in manifest["files"]} | {"manifest.json", "install.json"}
    expected_directories = {str(parent) for name in expected for parent in PurePosixPath(name).parents
                            if str(parent) != "."}
    found, directories_found = set(), set()
    pending = [release]
    while pending:
        parent = pending.pop()
        with os.scandir(parent) as entries:
            for entry in entries:
                path = Path(entry.path)
                name = path.relative_to(release).as_posix()
                info = entry.stat(follow_symlinks=False)
                if stat.S_ISDIR(info.st_mode):
                    require(name in expected_directories, "installed layout mismatch")
                    require(stat.S_IMODE(info.st_mode) == 0o755, "installed directory mode mismatch")
                    directories_found.add(name)
                    pending.append(path)
                else:
                    require(name in expected, "installed layout mismatch")
                    found.add(name)
    require(found == expected and directories_found == expected_directories, "installed layout mismatch")
    metadata = {}
    for item in manifest["files"]:
        path = release / item["path"]
        with regular_file(path, item["size"]) as (stream, info):
            require(info.st_size == item["size"], "installed file size mismatch")
            require(stat.S_IMODE(info.st_mode) == int(item["mode"], 8), "installed file mode mismatch")
            if item["path"] == "bin/save":
                elf_header(stream.read(64))
                stream.seek(0)
            require(stream_hash(stream, item["size"]) == item["sha256"], "installed file SHA-256 mismatch")
            if item["path"] in (SOURCE, BUILD, INVENTORY):
                stream.seek(0)
                metadata[item["path"]] = stream.read(MAX_JSON + 1)
    verify_payload_metadata(manifest, metadata)
    return manifest, receipt


def installed(root: Path, ident: str) -> tuple[dict, dict]:
    return verify_installed(root / "releases" / ident, ident)


def install(options) -> dict:
    # Open and completely verify before even creating the install root.
    with archive_snapshot(options.bundle) as (stream, _):
        manifest, indexes, manifest_sha = verify_archive(stream, options.expected_sha256)
        ident = manifest["artifact_id"]
        with install_root(options.root, create_root=True):
            release = options.root / "releases" / ident
            if release.exists() or release.is_symlink():
                existing, receipt = installed(options.root, ident)
                require(existing == manifest and receipt["archive_sha256"] == options.expected_sha256,
                        "installed version exists with different bytes")
                state = "already_present"
            else:
                staging = Path(tempfile.mkdtemp(prefix=".staging-", dir=options.root / "releases"))
                try:
                    (staging / "manifest.json").write_bytes(canonical(manifest))
                    (staging / "manifest.json").chmod(0o644)
                    for item, offset in indexes:
                        output = staging / item["path"]
                        output.parent.mkdir(parents=True, exist_ok=True)
                        stream.seek(offset)
                        checksum = hashlib.sha256()
                        with output.open("xb") as target:
                            remaining = item["size"]
                            while remaining:
                                block = stream.read(min(CHUNK, remaining))
                                require(bool(block), "archive changed during install")
                                target.write(block)
                                checksum.update(block)
                                remaining -= len(block)
                            target.flush()
                            os.fchmod(target.fileno(), int(item["mode"], 8))
                        require(checksum.hexdigest() == item["sha256"], "archive changed during install")
                    receipt = {"schema_version": REVISION, "artifact_id": ident,
                               "archive_sha256": options.expected_sha256, "manifest_sha256": manifest_sha,
                               "compatibility": COMPATIBILITY}
                    (staging / "install.json").write_bytes(canonical(receipt))
                    (staging / "install.json").chmod(0o644)
                    # Pin directory modes despite the caller's umask and verify the staged
                    # installed representation before making it visible as a release.
                    staging.chmod(0o755)
                    for parent, directories, _ in os.walk(staging):
                        for directory in directories:
                            (Path(parent) / directory).chmod(0o755)
                    verify_installed(staging, ident)
                    require(not release.exists() and not release.is_symlink(), "version appeared during install")
                    staging.rename(release)
                    state = "installed"
                finally:
                    if staging.exists():
                        shutil.rmtree(staging)
            installed(options.root, ident)
    return {"operation": "install", "status": state, "artifact_id": ident,
            "archive_sha256": options.expected_sha256, "release": str(release),
            "binary": str(release / "bin/save"), "selected": False, "compatibility": COMPATIBILITY}


def selection(options) -> dict:
    with install_root(options.root):
        manifest, receipt = installed(options.root, options.artifact_id)
        current = options.root / "current"
        previous = None
        if current.exists() or current.is_symlink():
            require(current.is_symlink(), "current is an unrelated file or directory")
            pointer = os.readlink(current)
            parts = pointer.split("/")
            require(len(parts) == 2 and parts[0] == "releases" and IDENTITY.fullmatch(parts[1]) is not None,
                    "current pointer escapes version layout")
            safe_directory(options.root / pointer)
            previous = parts[1]
        temporary = options.root / f".current-{uuid.uuid4().hex}"
        try:
            temporary.symlink_to(f"releases/{options.artifact_id}")
            os.replace(temporary, current)
        finally:
            temporary.unlink(missing_ok=True)
    return {"operation": options.operation, "status": "selected", "artifact_id": manifest["artifact_id"],
            "previous_artifact_id": previous, "archive_sha256": receipt["archive_sha256"],
            "pointer": f"releases/{options.artifact_id}", "binary": str(current / "bin/save"),
            "compatibility": COMPATIBILITY}


def main() -> int:
    require(sys.version_info >= (3, 11), "Python >=3.11 is required")
    parser = argparse.ArgumentParser(description=__doc__, epilog=(
        "All output is JSON. Refusal exits 2. Only explicit root/releases/ID and root/current are changed; "
        "no global installation or migration. Activation/rollback are separate from install. "
        "Archive <=96MiB, binary <=64MiB, data <=4MiB, JSON <=1MiB, <=2048 files. "
        "Uncompressed canonical USTAR only; expected SHA256 must come from a separate trusted record."))
    commands = parser.add_subparsers(dest="operation", required=True)
    command = commands.add_parser("create", help="deterministically package explicit observed build bytes")
    for option in ("binary", "source-manifest", "build-provenance", "inventory", "notices-dir", "output"):
        command.add_argument(f"--{option}", required=True, type=Path)
    command.add_argument("--product-version", required=True)
    for name in ("verify", "install"):
        command = commands.add_parser(name)
        command.add_argument("bundle", type=Path)
        command.add_argument("--expected-sha256", required=True)
        if name == "install":
            command.add_argument("--root", required=True, type=Path)
    for name in ("activate", "rollback", "inspect-installed"):
        command = commands.add_parser(name)
        command.add_argument("artifact_id")
        command.add_argument("--root", required=True, type=Path)
    options = parser.parse_args()
    try:
        if options.operation == "create":
            result = create(options)
        elif options.operation == "install":
            result = install(options)
        elif options.operation == "verify":
            with archive_snapshot(options.bundle) as (stream, info):
                manifest, _, manifest_sha = verify_archive(stream, options.expected_sha256)
            result = {"operation": "verify", "status": "verified", "artifact_id": manifest["artifact_id"],
                      "archive_sha256": options.expected_sha256, "archive_size": info.st_size,
                      "manifest_sha256": manifest_sha, "manifest": manifest}
        elif options.operation == "inspect-installed":
            with install_root(options.root):
                manifest, receipt = installed(options.root, options.artifact_id)
            result = {"operation": "inspect-installed", "status": "verified", "manifest": manifest,
                      "installation": receipt, "binary": str(options.root / "releases" / options.artifact_id / "bin/save")}
        else:
            result = selection(options)
        print(json.dumps(result, sort_keys=True, allow_nan=False))
        return 0
    except (Refused, OSError, OverflowError) as error:
        print(json.dumps({"operation": options.operation, "status": "refused", "reason": str(error)}), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
