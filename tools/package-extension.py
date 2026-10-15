"""Build an inert development package from a reviewed manifest and compiled ELF; never execute it."""
import argparse
import base64
import hashlib
import json
from pathlib import Path


def unique(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("Duplicate manifest field")
        result[key] = value
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--executable", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    options = parser.parse_args()
    with options.manifest.open("rb") as stream:
        raw = stream.read(65537)
    if len(raw) > 65536:
        parser.error("Manifest exceeds 64 KiB")
    manifest = json.loads(raw, object_pairs_hook=unique)
    with options.executable.open("rb") as stream:
        executable = stream.read(8 * 1024 * 1024 + 1)
    if not 64 <= len(executable) <= 8 * 1024 * 1024 or executable[:4] != b"\x7fELF":
        parser.error("Expected an ELF executable of 64 bytes through 8 MiB")
    manifest["executable"]["sha256"] = "sha256:" + hashlib.sha256(executable).hexdigest()
    manifest["executable"]["bytes"] = len(executable)
    package = {"package_format": 1, "manifest": manifest,
               "executable_base64": base64.b64encode(executable).decode("ascii")}
    raw = (json.dumps(package, ensure_ascii=False, separators=(",", ":"), allow_nan=False) + "\n").encode()
    if len(raw) > 12 * 1024 * 1024:
        parser.error("Package exceeds 12 MiB")
    with options.output.open("xb") as stream:
        stream.write(raw)
    print("sha256:" + hashlib.sha256(raw).hexdigest())


if __name__ == "__main__":
    main()
