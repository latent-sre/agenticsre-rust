"""Public read-profile contracts; run inside the declared isolated test boundary."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[1]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--git", type=Path, default=Path("/usr/bin/git"))
    parser.add_argument("--rg", type=Path, default=Path("/usr/bin/rg"))
    parser.add_argument("--bwrap", type=Path, default=Path("/usr/bin/bwrap"))
    parser.add_argument("--admission-only", action="store_true",
                        help="Check inspection/refusal only; does not establish containment")
    parser.add_argument("--no-schema", action="store_true",
                        help="Skip JSON Schema when the isolated image lacks jsonschema")
    parser.add_argument("--json-results", action="store_true",
                        help="Emit synthetic receipts for schema validation outside the image")
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    bindings = {}
    for name in ("git", "rg", "bwrap"):
        path = getattr(args, name).resolve(strict=True)
        with path.open("rb") as handle:
            sha256 = hashlib.file_digest(handle, "sha256").hexdigest()
        bindings[name] = {"path": str(path), "sha256": "sha256:" + sha256}
    validators = {}
    if not args.no_schema:
        from jsonschema import Draft202012Validator, FormatChecker
        from referencing import Registry, Resource

        schemas = [json.loads(path.read_text(encoding="utf-8")) for path in
                   (ROOT / "docs/sre-workbench/schemas").glob("*.schema.json")]
        registry = Registry().with_resources((s["$id"], Resource.from_contents(s)) for s in schemas)
        validators = {s["$id"]: Draft202012Validator(s, registry=registry,
                       format_checker=FormatChecker()) for s in schemas}
    checked = 0
    environment = {"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8",
                   "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_SYSTEM": "/dev/null",
                   "GIT_CONFIG_NOSYSTEM": "1", "GIT_TERMINAL_PROMPT": "0"}

    with tempfile.TemporaryDirectory(prefix="workbench-read-public-") as temporary:
        scratch = Path(temporary)
        checkout = scratch / "checkout"
        checkout.mkdir()
        policy_file = scratch / "policy.json"
        policy = {"version": 1, "profile": "linux-read-v1",
                  "roots": [{"id": "fixture", "path": str(checkout)}],
                  "executables": bindings}
        policy_file.write_text(json.dumps(policy), encoding="utf-8")
        policy_file.chmod(0o600)

        def git(*command: str) -> None:
            subprocess.run([bindings["git"]["path"], "-c", "core.hooksPath=/nonexistent",
                            "-c", "commit.gpgsign=false", "-c", "user.name=Fixture",
                            "-c", "user.email=fixture@example.invalid", *command],
                           cwd=checkout, env=environment, check=True, timeout=10,
                           stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)

        git("init", "--template=", "--object-format=sha1", "--initial-branch=fixture")
        (checkout / "tracked.txt").write_text("original\n", encoding="utf-8")
        (checkout / "literal λ.txt").write_text("café\n", encoding="utf-8")
        git("add", "--", "tracked.txt", "literal λ.txt")
        git("commit", "-m", "fixture initial")
        (checkout / "tracked.txt").write_text("needle\n", encoding="utf-8")
        (checkout / "untracked.txt").write_text("untracked\n", encoding="utf-8")
        index_before = hashlib.sha256((checkout / ".git/index").read_bytes()).hexdigest()

        def invoke(*command: str, expected: int = 0, policy_path: Path = policy_file) -> dict:
            nonlocal checked
            run = subprocess.run([str(binary), "--json", "--read-policy", str(policy_path),
                                  *command], cwd=scratch, env=environment, capture_output=True,
                                 check=False, timeout=12)
            if run.returncode != expected:
                raise RuntimeError(f"{command[:3]}: exit {run.returncode}, expected {expected}; "
                                   f"stderr={run.stderr[:300]!r}; stdout={run.stdout[:600]!r}")
            assert len(run.stdout) <= 2 * 1024 * 1024
            result = json.loads(run.stdout)
            if validators:
                validators["urn:sre-workbench:spec:result:0.1"].validate(result)
                if result["data"].get("profile") == "linux-read-v1":
                    validators["urn:sre-workbench:spec:read-profile-data:0.1"].validate(result["data"])
            assert result["assessment"] == "not_assessed" and result["record_mode"] == "never"
            checked += 1
            if args.json_results:
                print("READ_PROFILE_RESULT " + json.dumps(result, separators=(",", ":")), flush=True)
            return result

        def inspect(program: str, *command: str, expected: int = 0,
                    policy_path: Path = policy_file) -> dict:
            return invoke("command", "inspect", "--cwd", str(checkout), "--", program,
                          *command, expected=expected, policy_path=policy_path)

        def denied(result: dict) -> None:
            assert result["effect_outcome"] == "not_attempted"
            assert not result["data"].get("execution_performed", False)
            assert result["execution"]["status"] != "succeeded"

        forms = [("git", ["status"]), ("git", ["diff", "--cached", "--stat"]),
                 ("git", ["log", "-n", "100"]), ("rg", ["--files"]),
                 ("rg", ["-F", "-e", "$(literal); café", "--", "literal λ.txt"])]
        for program, command in forms:
            result = inspect(program, *command)
            assert result["data"]["profile"] == "linux-read-v1"
            assert result["data"]["execution_performed"] is False
            assert result["data"]["inspection_advisory"] is True
            assert result["data"]["isolation"]["state"] == "required"
            assert result["data"]["tool"]["execution_confirmed"] is False
            assert result["effect_outcome"] == "not_applicable"

        forbidden = [("git", ["-c", "alias.x=!touch marker", "x"]),
                     ("git", ["status", "--porcelain=v2"]),
                     ("git", ["diff", "--output=marker"]),
                     ("git", ["diff", "--ext-diff"]),
                     ("git", ["diff", "HEAD"]),
                     ("git", ["log", "--format=%G?"]),
                     ("git", ["log", "-n", "101"]),
                     ("git", ["log", "--max-count=0"]),
                     ("rg", ["--pre", "touch marker", "-e", "x"]),
                     ("rg", ["--follow", "-e", "x"]),
                     ("rg", ["--files", "-e", "x"]),
                     ("rg", ["--pcre2", "-e", "x"]),
                     ("rg", ["-e", "x", "--", "../outside"]),
                     ("rg", ["-e", "x", "--", "/etc/passwd"]),
                     ("rg", ["-e", "x", "--", "-"]),
                     ("/usr/bin/git", ["status"]), ("sh", ["-c", "touch marker"])]
        for program, command in forbidden:
            denied(invoke("exec", "--cwd", str(checkout), "--", program, *command, expected=2))
        denied(invoke("exec", "--", "git", "status", expected=2))
        denied(invoke("exec", "--cwd", str(scratch), "--", "git", "status", expected=2))
        (checkout / "outside-link").symlink_to(scratch / "outside")
        (scratch / "outside").write_text("outside canary\n", encoding="utf-8")
        denied(inspect("rg", "-e", "canary", "--", "outside-link", expected=2))

        malformed = []
        for field, value in (("version", 2), ("profile", "operator-local"),
                             ("roots", []), ("extra", True)):
            changed = copy.deepcopy(policy)
            changed[field] = value
            malformed.append(changed)
        for key, value in (("sha256", "sha256:" + "0" * 64),
                           ("path", str(checkout / "git"))):
            changed = copy.deepcopy(policy)
            changed["executables"]["git"][key] = value
            malformed.append(changed)
        for field in ("roots", "executables"):
            changed = copy.deepcopy(policy)
            changed[field] = [list(policy["roots"][0].values())] if field == "roots" else []
            malformed.append(changed)
        bad_policy = scratch / "bad-policy.json"
        for value in malformed:
            bad_policy.write_text(json.dumps(value), encoding="utf-8")
            denied(inspect("git", "status", expected=2, policy_path=bad_policy))
        raw_policy = json.dumps(policy)
        for text in ('{"version":1,' + raw_policy[1:], " " * 65537,
                     "[" * 10 + "0" + "]" * 10):
            bad_policy.write_text(text, encoding="utf-8")
            denied(inspect("git", "status", expected=2, policy_path=bad_policy))
        bad_policy.write_text(raw_policy, encoding="utf-8")
        bad_policy.chmod(0o666)
        denied(inspect("git", "status", expected=2, policy_path=bad_policy))
        bad_policy.chmod(0o600)
        linked_policy = scratch / "linked-policy.json"
        linked_policy.symlink_to(policy_file)
        denied(inspect("git", "status", expected=2, policy_path=linked_policy))

        request = {"spec_version": "0.1", "request_id": "read-profile-public-001",
                   "operation": "command.inspect", "operation_version": 1,
                   "target": {"kind": "local", "id": "workstation"},
                   "inputs": {"program": "git", "args": ["status"], "cwd": str(checkout)},
                   "limits": {"timeout_ms": 3000, "max_output_bytes": 1048576}, "record": "never"}
        request_file = scratch / "request.json"
        request_file.write_text(json.dumps(request), encoding="utf-8")
        inspected_call = invoke("call", "--request", str(request_file))
        assert inspected_call["data"] == inspect("git", "status")["data"]
        for field, value in (("read_policy", str(policy_file)), ("role", "admin"),
                             ("grants", ["*"])):
            changed = copy.deepcopy(request)
            changed[field] = value
            request_file.write_text(json.dumps(changed), encoding="utf-8")
            denied(invoke("call", "--request", str(request_file), expected=2))

        if not args.admission_only:
            def execute(program: str, *command: str, expected: int = 0) -> dict:
                result = invoke("exec", "--cwd", str(checkout), "--", program,
                                *command, expected=expected)
                assert result["data"]["execution_performed"] is True
                assert result["data"]["tool"]["execution_confirmed"] is True
                assert result["data"]["isolation"]["state"] == "confirmed"
                assert result["effect_outcome"] == "not_applicable"
                return result

            mission = execute("git", "status")
            assert mission["output"]["stdout"] == " M tracked.txt\n"
            assert execute("git", "diff", "--name-only")["output"]["stdout"] == "tracked.txt\n"
            assert "fixture initial" in execute("git", "log", "-n", "1")["output"]["stdout"]
            files = execute("rg", "--files")["output"]["stdout"].splitlines()
            assert any(name.endswith("tracked.txt") for name in files)
            assert not any(".git/" in name or "outside-link" in name for name in files)
            assert "needle" in execute("rg", "-F", "-e", "needle", "--", "tracked.txt")["output"]["stdout"]
            no_match = execute("rg", "-e", "no-such-fixture-pattern", expected=1)
            assert no_match["data"]["tool"]["exit_status"] == 1
            assert not any(error["code"].startswith("sandbox_") for error in no_match["errors"])
            request["operation"] = "process.exec"
            request_file.write_text(json.dumps(request), encoding="utf-8")
            called = invoke("call", "--request", str(request_file))
            for key in ("operation", "output", "execution", "effect_outcome"):
                assert called[key] == mission[key], key
            text = subprocess.run([str(binary), "--read-policy", str(policy_file), "exec",
                                   "--cwd", str(checkout), "--", "git", "status"],
                                  cwd=scratch, env=environment, capture_output=True, timeout=12)
            assert text.returncode == 0 and b"tracked.txt" in text.stdout
            (checkout / "noise.txt").write_text(("needle " + "x" * 100 + "\n") * 100, encoding="utf-8")
            truncated = invoke("exec", "--cwd", str(checkout), "--max-output-bytes", "1024",
                               "--", "rg", "-e", "needle", "--", "noise.txt", expected=1)
            assert truncated["execution"]["status"] == "partial"
            assert truncated["output"]["stdout_truncated"] is True

        assert hashlib.sha256((checkout / ".git/index").read_bytes()).hexdigest() == index_before
        assert not (checkout / "marker").exists() and not (scratch / "marker").exists()
    scope = "admission/inspection only; containment not exercised" if args.admission_only else "native execution, CLI/call/text parity and admission"
    schema = "JSON Schema checked" if validators else "JSON Schema unavailable in this image"
    print(f"PASS: {checked} public read-profile results; {scope}; {schema}.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
