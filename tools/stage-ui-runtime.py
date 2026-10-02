"""Stage cached browser-test dependencies into one absent or empty caller-owned directory.

No downloads, installation, browser/product execution, or host-policy changes. Existing readable
files are hardlinked where possible; treat staged files as immutable. Files needing public read
permissions are reflinked/copied before chmod, so source inodes are never chmodded. A failed stage
is left for inspection; rerunning against a nonempty destination is refused.

Cally usage: .ui-runtime/python/bin/python3.14 -S -B TEST.py with PYTHONPATH set explicitly to
.ui-runtime/site-packages. WORKBENCH_BROWSER_EXECUTABLE selects .ui-runtime/browser. Only that
Chromium wrapper sets LD_LIBRARY_PATH; never export its library directory to save or Python.
"""
from __future__ import annotations

import argparse
from collections import Counter
from email.parser import Parser
import errno
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys

PYTHON = Path("/home/hawkfire/.local/share/uv/python/cpython-3.14.7-linux-x86_64-gnu")
PACKAGES = Path("/tmp/dualla-dashboard-preview-venv/lib/python3.14/site-packages")
CHROMIUM = Path("/tmp/dualla-playwright/chromium_headless_shell-1243/chrome-headless-shell-linux64")
LIBRARIES = Path("/tmp/dualla-browser-libs/root/usr/lib/x86_64-linux-gnu")
FONTS = Path("/usr/share/fonts/truetype/dejavu")
PACKAGE_NAMES = ("playwright", "greenlet", "pyee", "typing_extensions")
GLIBC = re.compile(r"^(?:ld-linux.*|ld-\d.*|lib(?:c|m|pthread|dl|rt|resolv|util|anl|BrokenLocale|nss_[^.]+)\.so(?:\..*)?)$")
MAX_FILES = 12000
MAX_BYTES = 768 * 1024 * 1024
FICLONE = 0x40049409

BROWSER = """#!/usr/bin/env bash
set -euo pipefail
IFS=$'\\n\\t'
runtime_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
# This environment belongs only to Chromium and its descendants.
export LD_LIBRARY_PATH="$runtime_dir/lib"
export FONTCONFIG_FILE="$runtime_dir/fonts.conf"
exec "$runtime_dir/chromium/chrome-headless-shell" "$@"
"""
FONTCONFIG = """<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
<fontconfig>
  <dir>/opt/workbench-source/.ui-runtime/fonts</dir>
  <cachedir>/tmp/workbench-font-cache</cachedir>
  <config><rescan><int>0</int></rescan></config>
</fontconfig>
"""


def sha256(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def add_tree(plan: dict[Path, Path], source: Path, destination: Path,
             *, exclude: frozenset[str] = frozenset()) -> None:
    if not source.is_dir():
        raise ValueError(f"cached directory missing: {source}")
    for directory, names, files in os.walk(source, followlinks=False):
        names[:] = sorted(name for name in names if name not in exclude and name != "__pycache__")
        parent = Path(directory)
        for name in names:
            if (parent / name).is_symlink():
                raise ValueError(f"unexpected directory symlink in cached runtime: {parent / name}")
        for name in sorted(files):
            if name.endswith((".pyc", ".pyo")):
                continue
            item = parent / name
            resolved = item.resolve(strict=True)
            if not resolved.is_relative_to(source.resolve()) or not resolved.is_file():
                raise ValueError(f"cached file leaves selected tree: {item}")
            plan[destination / item.relative_to(source)] = resolved


def dependencies(binary: Path) -> tuple[dict[str, Path], list[str]]:
    run = subprocess.run(["/usr/bin/ldd", str(binary)], check=True, capture_output=True,
                         text=True, timeout=10,
                         env={"PATH": "/usr/bin:/bin", "LANG": "C",
                              "LD_LIBRARY_PATH": str(LIBRARIES)})
    libraries: dict[str, Path] = {}
    excluded = []
    for line in run.stdout.splitlines():
        if "=> not found" in line:
            raise ValueError(f"cached browser dependency is missing: {line.strip()}")
        match = re.fullmatch(r"\s*(\S+) => (/\S+) \(0x[0-9a-f]+\)\s*", line)
        if match is None:
            if "linux-vdso" in line or re.fullmatch(r"\s*/\S*ld-linux\S* \(0x[0-9a-f]+\)\s*", line):
                continue
            raise ValueError(f"unrecognized ldd dependency record: {line.strip()}")
        name, supplied_path = match.groups()
        if not re.fullmatch(r"[A-Za-z0-9_.+-]+", name):
            raise ValueError("invalid dependency library name")
        if GLIBC.fullmatch(name):
            excluded.append(name)
            continue
        path = Path(supplied_path).resolve(strict=True)
        if not path.is_file() or path.stat().st_size > MAX_BYTES:
            raise ValueError(f"invalid dependency file: {path}")
        libraries[name] = path
    return libraries, sorted(excluded)


def copy_file(source: Path, destination: Path) -> str:
    """Never chmod a hardlink: permission changes apply only to a new private inode."""
    destination.parent.mkdir(parents=True, exist_ok=True, mode=0o755)
    mode = stat.S_IMODE(source.stat().st_mode)
    readable = mode & 0o444 == 0o444
    executable = bool(mode & 0o111)
    if readable and (not executable or mode & 0o111 == 0o111):
        try:
            os.link(source, destination)
            return "hardlink"
        except OSError as error:
            if error.errno not in (errno.EXDEV, errno.EPERM, errno.EACCES, errno.EMLINK):
                raise
    method = "reflink"
    with source.open("rb") as original, destination.open("xb") as staged:
        try:
            fcntl.ioctl(staged.fileno(), FICLONE, original.fileno())
        except OSError as error:
            if error.errno not in (errno.EXDEV, errno.EINVAL, errno.EOPNOTSUPP,
                                  errno.ENOTTY, errno.EPERM):
                raise
            staged.truncate(0)
            shutil.copyfileobj(original, staged, length=1024 * 1024)
            method = "copy"
    destination.chmod(0o755 if executable else 0o644)
    return method


def stage(destination: Path) -> dict:
    if sys.platform != "linux":
        raise ValueError("this cached runtime stage supports Linux only")
    if destination.is_symlink() or (destination.exists() and
                                   (not destination.is_dir() or any(destination.iterdir()))):
        raise ValueError("destination must be absent or an empty directory; nothing was changed")
    destination = destination.absolute()
    plan: dict[Path, Path] = {}
    interpreter = PYTHON / "bin/python3.14"
    if not interpreter.is_file() or not os.access(interpreter, os.X_OK):
        raise ValueError("the pinned standalone Python3.14 cache is unavailable")
    plan[Path("python/bin/python3.14")] = interpreter
    plan[Path("python/BUILD")] = PYTHON / "BUILD"
    add_tree(plan, PYTHON / "lib/python3.14", Path("python/lib/python3.14"),
             exclude=frozenset({"site-packages"}))
    versions = {}
    for name in PACKAGE_NAMES:
        source = PACKAGES / name
        if source.is_dir():
            add_tree(plan, source, Path("site-packages") / name)
        elif (PACKAGES / f"{name}.py").is_file():
            plan[Path("site-packages") / f"{name}.py"] = PACKAGES / f"{name}.py"
        else:
            raise ValueError(f"required cached Python package missing: {name}")
        metadata = list(PACKAGES.glob(f"{name}-*.dist-info"))
        if len(metadata) != 1:
            raise ValueError(f"cached package metadata is missing or ambiguous: {name}")
        fields = Parser().parsestr((metadata[0] / "METADATA").read_text(encoding="utf-8"))
        if fields.get("Name", "").replace("-", "_") != name:
            raise ValueError(f"cached package metadata identity mismatch: {name}")
        versions[name] = fields["Version"]
        add_tree(plan, metadata[0], Path("site-packages") / metadata[0].name)
    node = PACKAGES / "playwright/driver/node"
    if not node.is_file() or not os.access(node, os.X_OK):
        raise ValueError("Playwright's cached Node driver is missing")
    add_tree(plan, CHROMIUM, Path("chromium"))
    libraries, excluded = dependencies(CHROMIUM / "chrome-headless-shell")
    plan.update({Path("lib") / name: path for name, path in libraries.items()})
    for font in sorted(FONTS.glob("DejaVu*.ttf")):
        plan[Path("fonts") / font.name] = font.resolve(strict=True)
    if not any(path.parts[0] == "fonts" for path in plan):
        raise ValueError("cached DejaVu fonts are unavailable")
    plan[Path("fonts/LICENSE")] = Path("/usr/share/doc/fonts-dejavu-core/copyright")
    total = sum(path.stat().st_size for path in plan.values())
    if len(plan) > MAX_FILES or total > MAX_BYTES:
        raise ValueError("cached runtime exceeds 12000 files or 768MiB; nothing was staged")
    # No source overlaps are possible with a destination nested inside a selected cache.
    for source in (PYTHON, PACKAGES, CHROMIUM, LIBRARIES, FONTS):
        if destination.resolve().is_relative_to(source.resolve()):
            raise ValueError("destination must be outside cached source directories")
    destination.mkdir(parents=True, exist_ok=True, mode=0o755)
    if any(destination.iterdir()):
        raise ValueError("destination became nonempty before staging")
    destination.chmod(0o755)  # Caller-owned empty directory; directory hardlinks are forbidden.
    records = []
    for relative, source in sorted(plan.items()):
        original_mode = stat.S_IMODE(source.stat().st_mode)
        digest = sha256(source)
        staged = destination / relative
        method = copy_file(source, staged)
        if sha256(staged) != digest or stat.S_IMODE(source.stat().st_mode) != original_mode:
            raise ValueError(f"staged bytes or source permissions changed: {relative}")
        records.append({"path": str(relative), "source": str(source), "sha256": digest,
                        "bytes": staged.stat().st_size, "method": method,
                        "source_mode": oct(original_mode),
                        "staged_mode": oct(stat.S_IMODE(staged.stat().st_mode))})
    for name, content, mode in (("browser", BROWSER, 0o755), ("fonts.conf", FONTCONFIG, 0o644)):
        staged = destination / name
        with staged.open("x", encoding="utf-8") as stream:
            stream.write(content)
        staged.chmod(mode)  # Newly generated files; never shared source inodes.
        records.append({"path": name, "source": "generated:tools/stage-ui-runtime.py",
                        "sha256": sha256(staged), "bytes": staged.stat().st_size,
                        "method": "generated", "staged_mode": oct(mode)})
    manifest = {"version": 1, "purpose": "cached offline GUI verification runtime",
                "python_source": str(PYTHON), "python_version": "3.14.7",
                "chromium_source": str(CHROMIUM), "packages": versions,
                "glibc_libraries_excluded": excluded,
                "target_glibc": "Cally image supplies glibc2.41; no glibc or loader is bundled",
                "font_target": "/opt/workbench-source/.ui-runtime/fonts",
                "limits": {"files": MAX_FILES, "bytes": MAX_BYTES}, "files": records,
                "notes": ["No source cache file is chmodded; hardlinks must remain immutable.",
                          "LD_LIBRARY_PATH is set only by the Chromium wrapper.",
                          "Node/greenlet use target-image C++ runtime dependencies.",
                          "Hashes identify cached bytes; staging does not establish upstream authenticity."]}
    with (destination / "provenance.json").open("x", encoding="utf-8") as stream:
        json.dump(manifest, stream, indent=2)
        stream.write("\n")
    return {"destination": str(destination), "files": len(records),
            "bytes": sum(record["bytes"] for record in records),
            "methods": dict(Counter(record["method"] for record in records)),
            "provenance_sha256": sha256(destination / "provenance.json")}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("destination", type=Path, help="Absent or empty .ui-runtime directory")
    args = parser.parse_args()
    os.umask(0o022)  # New staged directories/provenance must remain readable after root-owned COPY.
    try:
        summary = stage(args.destination)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"runtime staging failed: {error}", file=sys.stderr)
        return 1
    print(json.dumps(summary, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
