#!/usr/bin/env python3
"""Transfer bounded GUI evidence through a disposable container's ordinary log."""
from __future__ import annotations

import argparse
import base64
import hashlib
import json
from pathlib import Path
import re
import stat

PREFIX = "WORKBENCH_UI_ARTIFACT "
MAX_FILE = 2 * 1024 * 1024
MAX_TOTAL = 16 * 1024 * 1024
MAX_FILES = 16
NAME = re.compile(r"[a-z][a-z0-9-]{0,63}\.(?:png|json)")


def export(directory: Path) -> None:
    total = 0
    files = sorted(directory.iterdir()) if directory.exists() else []
    if len(files) > MAX_FILES:
        raise ValueError("Too many GUI evidence files")
    for path in files:
        metadata = path.lstat()
        if not NAME.fullmatch(path.name) or not stat.S_ISREG(metadata.st_mode):
            raise ValueError("Unexpected GUI evidence file")
        if metadata.st_size > MAX_FILE:
            raise ValueError("GUI evidence file exceeds size bound")
        content = path.read_bytes()
        if len(content) > MAX_FILE:
            raise ValueError("GUI evidence changed beyond size bound")
        total += len(content)
        if total > MAX_TOTAL:
            raise ValueError("GUI evidence exceeds total bound")
        print(PREFIX + json.dumps({"name": path.name,
                                  "sha256": hashlib.sha256(content).hexdigest(),
                                  "data": base64.b64encode(content).decode("ascii")}))


def collect(log: Path, directory: Path) -> None:
    directory.mkdir(exist_ok=False)
    total = 0
    count = 0
    # Bounded readline also prevents unrelated output from forcing a huge allocation.
    with log.open(encoding="utf-8") as source:
        while line := source.readline(MAX_FILE * 2):
            if not line.endswith("\n"):
                raise ValueError("Oversized or incomplete evidence log line")
            if not line.startswith(PREFIX):
                continue
            item = json.loads(line[len(PREFIX):])
            if not isinstance(item, dict) or set(item) != {"name", "sha256", "data"}:
                raise ValueError("Invalid GUI evidence record")
            if not isinstance(item["name"], str) or not NAME.fullmatch(item["name"]):
                raise ValueError("Invalid GUI evidence name")
            content = base64.b64decode(item["data"], validate=True)
            total += len(content)
            count += 1
            if len(content) > MAX_FILE or total > MAX_TOTAL or count > MAX_FILES:
                raise ValueError("GUI evidence exceeds bounds")
            if hashlib.sha256(content).hexdigest() != item["sha256"]:
                raise ValueError("GUI evidence digest mismatch")
            with (directory / item["name"]).open("xb") as target:
                target.write(content)
    print(f"Collected {count} GUI evidence files ({total} bytes)")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("export", "collect"))
    parser.add_argument("path", type=Path)
    parser.add_argument("destination", type=Path, nargs="?")
    args = parser.parse_args()
    if args.mode == "export":
        export(args.path)
    elif args.destination is None:
        parser.error("collect requires a destination")
    else:
        collect(args.path, args.destination)
