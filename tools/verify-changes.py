"""Installed controlled-change failure acceptance; run through tools/test-sandbox.sh.

Uses real mTLS, the ordinary installed CLI, the pinned upstream validator, SIGKILL and
an optional private 64KiB filesystem. No product test failpoints or live targets.
"""
from __future__ import annotations

import argparse
import base64
import ctypes
import errno
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import select
import selectors
import shutil
import signal
import struct
import subprocess
import tempfile
import time

from jsonschema import Draft202012Validator, FormatChecker
from referencing import Registry, Resource

ROOT = Path(__file__).resolve().parents[1]
PROMTOOL_SHA256 = "c736d55d3ccd959fe48329965fb5cb671d45585a9483954766448035100ff75c"
RUNNER = "changes.acceptance"
REQUESTER = "requester.fixture"
APPROVER = "approver.fixture"


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def encoded(value):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"), allow_nan=False).encode()


def identity(path):
    st = path.stat()
    return {"dev": st.st_dev, "ino": st.st_ino, "mode": st.st_mode, "uid": st.st_uid,
            "gid": st.st_gid, "nlink": st.st_nlink, "size": st.st_size,
            "mtime_ns": st.st_mtime_ns, "ctime_ns": st.st_ctime_ns, "sha256": sha(path.read_bytes())}


def files(directory):
    return {str(path.relative_to(directory)): {"sha256": sha(path.read_bytes()), "size": path.stat().st_size}
            for path in sorted(directory.rglob("*")) if path.is_file()}


class Publications:
    """Observe real ledger renames; this does not add a product scheduling hook."""
    def __init__(self, directory):
        self.libc = ctypes.CDLL(None, use_errno=True)
        self.fd = self.libc.inotify_init1(os.O_CLOEXEC | os.O_NONBLOCK)
        assert self.fd >= 0, ctypes.get_errno()
        watch = self.libc.inotify_add_watch(self.fd, os.fsencode(directory), 0x80)  # IN_MOVED_TO
        assert watch >= 0, ctypes.get_errno()

    def ledger(self, timeout):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            ready, _, _ = select.select([self.fd], [], [], max(0, deadline - time.monotonic()))
            if not ready:
                break
            raw = os.read(self.fd, 65536)
            offset = 0
            while offset < len(raw):
                _, mask, _, length = struct.unpack_from("iIII", raw, offset)
                name = raw[offset + 16:offset + 16 + length].split(b"\0", 1)[0]
                offset += 16 + length
                if mask & 0x80 and name == b"ledger.json":
                    return
        raise AssertionError("No durable ledger publication observed during apply")

    def close(self):
        os.close(self.fd)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--promtool", type=Path, required=True)
    parser.add_argument("--expected-binary-sha256", required=True)
    parser.add_argument("--source-manifest", type=Path, required=True)
    parser.add_argument("--expected-source-manifest-sha256", required=True)
    parser.add_argument("--evidence-dir", type=Path, required=True)
    parser.add_argument("--small-store", type=Path, help="Fresh change ledger directly under /tmp/evidence-small-disk")
    options = parser.parse_args()
    assert os.environ.get("CARGO_HOME") == "/tmp/cargo" and Path("/toolchain").is_dir(), "Use tools/test-sandbox.sh"
    spec = importlib.util.spec_from_file_location("change_helpers", ROOT / "tools/verify-evidence.py")
    helpers = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helpers)
    source = helpers.source_guard(ROOT, options.source_manifest, options.expected_source_manifest_sha256)
    assert source["tools/verify-changes.py"] == sha(Path(__file__).read_bytes())
    original = options.binary.resolve(strict=True)
    upstream = options.promtool.resolve(strict=True)
    assert sha(original.read_bytes()) == options.expected_binary_sha256
    assert sha(upstream.read_bytes()) == PROMTOOL_SHA256
    evidence = options.evidence_dir.resolve()
    evidence.mkdir(mode=0o700, parents=True, exist_ok=False)
    schemas = {path.stem.removesuffix(".schema"): helpers.strict_json(path.read_bytes())
               for path in (ROOT / "docs/sre-workbench/schemas").glob("*.schema.json")}
    registry = Registry().with_resources((schema["$id"], Resource.from_contents(schema)) for schema in schemas.values())
    validators = {name: Draft202012Validator(schemas[name], registry=registry, format_checker=FormatChecker())
                  for name in ("result", "change-data", "config-check-data", "runner-client-config", "runner-server-config")}
    commands, lifecycles, gaps, active = [], [], [], []
    environment = {"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8", "TZ": "UTC"}

    def retain(name, value):
        raw = value if isinstance(value, bytes) else (json.dumps(value, sort_keys=True, indent=2) + "\n").encode()
        path = evidence / name
        path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        path.write_bytes(raw)

    def validate(raw):
        value = helpers.strict_json(raw)
        validators["result"].validate(value)
        assert value["assessment"] == "not_assessed"
        if value["operation"].startswith("change."):
            validators["change-data"].validate(value["data"])
            if value["data"].get("plan") is not None:
                validation = value["data"]["plan"]["validation"]
                validators["result"].validate(validation)
                validators["config-check-data"].validate(validation["data"])
        elif value["operation"] == "config.check":
            validators["config-check-data"].validate(value["data"])
        return value

    def finish(name, completed, expected=0, args=()):
        retain(f"commands/{name}.stdout.json", completed.stdout)
        retain(f"commands/{name}.stderr", completed.stderr)
        value = validate(completed.stdout)
        commands.append({"name": name, "exit_code": completed.returncode, "args": list(map(str, args))})
        assert not completed.stderr, (name, completed.stderr[:300])
        assert expected is None or completed.returncode == expected, (name, completed.returncode, value)
        return value

    with tempfile.TemporaryDirectory(prefix="installed-changes-") as temporary:
        root = Path(temporary)
        root.chmod(0o700)
        prefix = root / "prefix/bin"
        prefix.mkdir(parents=True)
        binary, promtool = prefix / "save", prefix / "promtool"
        for src, destination in ((original, binary), (upstream, promtool)):
            shutil.copyfile(src, destination)
            destination.chmod(0o500)
        cwd = root / "unrelated"
        cwd.mkdir()
        pki = root / "pki"
        pki.mkdir(mode=0o700)

        def private(path, raw):
            path.write_bytes(raw)
            path.chmod(0o600)

        def local(name, args, expected=0):
            completed = subprocess.run([str(binary), "--json", *map(str, args)], cwd=cwd, env=environment,
                                       capture_output=True, timeout=35)
            return finish(name, completed, expected, args)

        def openssl(*args):
            completed = subprocess.run(["/usr/bin/openssl", *map(str, args)], cwd=pki,
                                       env={**environment, "OPENSSL_CONF": "/dev/null"}, capture_output=True, timeout=10)
            assert completed.returncode == 0, (args[0], completed.stderr[:300])
            return completed.stdout

        openssl("req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256", "-nodes",
                "-keyout", "ca.key", "-out", "ca.pem", "-days", "2", "-subj", "/CN=Changes acceptance CA",
                "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign,cRLSign")
        openssl("x509", "-in", "ca.pem", "-outform", "DER", "-out", "ca.der")
        pins = {}
        for serial, name in enumerate(("server", "requester", "approver"), 1):
            hostname = f"{name}.test"
            openssl("req", "-new", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256", "-nodes",
                    "-keyout", f"{name}.key.pem", "-out", f"{name}.csr", "-subj", f"/CN={hostname}")
            usage = "serverAuth" if name == "server" else "clientAuth"
            private(pki / "extensions.cnf", f"basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage={usage}\nsubjectAltName=DNS:{hostname}\n".encode())
            openssl("x509", "-req", "-in", f"{name}.csr", "-CA", "ca.pem", "-CAkey", "ca.key", "-set_serial", serial,
                    "-days", "1", "-extfile", "extensions.cnf", "-out", f"{name}.pem")
            openssl("x509", "-in", f"{name}.pem", "-outform", "DER", "-out", f"{name}.der")
            openssl("pkcs8", "-topk8", "-nocrypt", "-in", f"{name}.key.pem", "-outform", "DER", "-out", f"{name}.key.der")
            pins[name] = "sha256:" + sha((pki / f"{name}.der").read_bytes())
        for path in pki.iterdir():
            path.chmod(0o600)
        retain("tls-identities.json", {"leaf_sha256": pins, "private_material_retained": False,
                                       "openssl_version": openssl("version").decode().strip()})
        clean = (ROOT / "tests/fixtures/prometheus-rules/clean.yml").read_bytes()
        target = root / "alerts.yml"
        private(target, clean)
        check_input = root / "check.json"
        private(check_input, encoded({"kind": "prometheus-rules", "validator": {"id": "promtool", "version": 1}, "file": str(target)}))
        checked = local("pinned-validator", ["--promtool", promtool, "config", "check", "--input", check_input])
        assert checked["data"]["validation_outcome"] == "valid" and checked["data"]["checked_rules"] > 0
        assert checked["data"]["runtime"]["executable_sha256"] == "sha256:" + PROMTOOL_SHA256
        binding = checked["data"]["validator"]["binding_digest"]
        configuration = {"config_version": 1, "runner_id": RUNNER, "listen": "127.0.0.1:0", "state_dir": str(root / "jobs"),
                         "ca_certificate": str(pki / "ca.der"), "certificate": str(pki / "server.der"), "private_key": str(pki / "server.key.der"),
                         "principals": [{"id": REQUESTER, "certificate_sha256": pins["requester"]}, {"id": APPROVER, "certificate_sha256": pins["approver"]}],
                         "targets": [{"id": "fixture.target", "snapshot": {"name": "Disposable rules", "environment": "isolated-sandbox"}}],
                         "grants": [], "promtool_path": str(promtool),
                         "changes": {"state_dir": str(root / "changes"), "targets": [{"id": "rules.fixture", "path": str(target),
                            "validator_binding_sha256": binding, "requesters": [REQUESTER], "approvers": [APPROVER]}]}}
        server = None
        generation = 0
        clients = {}

        def start():
            nonlocal server, generation
            generation += 1
            validators["runner-server-config"].validate(configuration)
            config = root / "server.json"
            private(config, encoded(configuration))
            retain(f"servers/{generation}-config.json", configuration)
            server = subprocess.Popen([str(binary), "--json", "--record", "always", "runner", "serve", "--server-config", str(config)],
                                      cwd=cwd, env=environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            active.append(server)
            with selectors.DefaultSelector() as selector:
                selector.register(server.stdout, selectors.EVENT_READ)
                assert selector.select(10), "runner readiness timed out"
                raw = server.stdout.readline(65536)
            ready = helpers.strict_json(raw)
            assert ready["runner_id"] == RUNNER and ready["server_certificate_sha256"] == pins["server"], ready
            retain(f"servers/{generation}-ready.json", ready)
            for principal in ("requester", "approver"):
                client = {"config_version": 1, "endpoint": ready["address"], "server_name": "server.test", "runner_id": RUNNER,
                          "server_certificate_sha256": pins["server"], "ca_certificate": str(pki / "ca.der"),
                          "certificate": str(pki / f"{principal}.der"), "private_key": str(pki / f"{principal}.key.der")}
                validators["runner-client-config"].validate(client)
                clients[principal] = root / f"{principal}-client.json"
                private(clients[principal], encoded(client))

        def stop(kill=False):
            nonlocal server
            assert server is not None
            if kill:
                server.kill()
            else:
                server.send_signal(signal.SIGCONT)
                server.terminate()
            stdout, stderr = server.communicate(timeout=5)
            retain(f"servers/{generation}-exit.stdout", stdout)
            retain(f"servers/{generation}-exit.stderr", stderr)
            assert server.returncode == (-signal.SIGKILL if kill else 143), (server.returncode, stdout, stderr)
            assert not stdout and not stderr
            server = None

        def arguments(name, method, body, principal="requester"):
            path = root / f"{name}-input.json"
            private(path, encoded(body))
            return ["--runner-config", str(clients[principal]), "--record", "never" if method == "status" else "always",
                    "change", method, "--runner", RUNNER, "--input", str(path), "--timeout-ms", "30000", "--max-output-bytes", "1048576"]

        def change(name, method, body, principal="requester", expected=0):
            return local(name, arguments(name, method, body, principal), expected)

        def approve_case(name, proposed):
            plan = change(name + "-plan", "plan", {"request_id": name, "target": "rules.fixture",
                         "proposed_base64": base64.b64encode(proposed).decode(), "expires_in_seconds": 300})
            assert plan["data"]["state"] == "planned"
            approval = change(name + "-approve", "approve", {"requester": REQUESTER, "request_id": name,
                              "plan_sha256": plan["data"]["plan_sha256"]}, principal="approver")
            return {"request_id": name, "plan_sha256": plan["data"]["plan_sha256"], "approval_id": approval["data"]["approval_id"]}, plan

        try:
            start()
            proposed = clean.replace(b"for: 5m", b"for: 10m")
            assert proposed != clean
            apply, planned = approve_case("concurrent", proposed)
            before = identity(target)
            children = []
            for number in range(2):
                name = f"concurrent-apply-{number}"
                args = arguments(name, "apply", apply)
                child = subprocess.Popen([str(binary), "--json", *args], cwd=cwd, env=environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                active.append(child)
                children.append((name, child, args))
            results = []
            for name, child, args in children:
                stdout, stderr = child.communicate(timeout=35)
                results.append(finish(name, subprocess.CompletedProcess(args, child.returncode, stdout, stderr), expected=None, args=args))
            after = identity(target)
            assert any(result["data"]["state"] == "verified" for result in results)
            assert all(result["data"]["state"] == "verified" or
                       (result["execution"]["status"] == "denied" and result["data"]["remote_error"] == "change_store_busy"
                        and result["effect_outcome"] == "not_attempted") for result in results)
            assert after["ino"] != before["ino"] and target.read_bytes() == proposed
            disk = helpers.strict_json((root / "changes/ledger.json").read_bytes())
            record = next(iter(disk["records"].values()))
            assert record["consumed"] and record["prepared"]["snapshot"]["file"]["ino"] == after["ino"]
            replay = change("concurrent-replay", "apply", apply)
            assert replay["data"]["plan"] == planned["data"]["plan"] and identity(target) == after
            lifecycles.append({"case": "concurrent-identical-apply", "before": before, "after": after,
                               "one_prepared_inode": True, "repeated_delivery_preserves_attempt": True})

            lookup = {"requester": REQUESTER, "request_id": "concurrent"}
            limited_args = arguments("bounded-status", "status", lookup)
            limited_args[limited_args.index("--max-output-bytes") + 1] = "1024"
            limited = local("bounded-status", limited_args, expected=1)
            assert limited["execution"]["status"] == "partial"
            assert limited["data"]["plan"] is None
            assert limited["data"]["plan_sha256"] == planned["data"]["plan_sha256"]
            assert limited["data"]["state"] == "verified"
            assert identity(target) == after

            text_args = arguments("human-status", "status", lookup)
            text = subprocess.run([str(binary), *text_args], cwd=cwd, env=environment,
                                  capture_output=True, timeout=35)
            retain("commands/human-status.stdout.txt", text.stdout)
            retain("commands/human-status.stderr", text.stderr)
            commands.append({"name": "human-status", "exit_code": text.returncode, "args": text_args,
                             "output_format": "text"})
            assert text.returncode == 0 and not text.stderr
            assert planned["data"]["plan_sha256"].encode() in text.stdout
            assert b"Plan SHA-256:" in text.stdout and b"State:" in text.stdout and b"verified" in text.stdout
            assert identity(target) == after
            lifecycles.append({"case": "bounded-and-human-status", "bounded_execution": "partial",
                               "plan_omitted_with_digest_preserved": True, "human_digest_and_state_visible": True})

            # Wake on the actual atomic publication of dispatch intent, then stop/kill the
            # real server. Scheduling can allow rename/readback to finish before SIGSTOP.
            interrupted_bytes = proposed.replace(b"for: 10m", b"for: 15m")
            apply, planned = approve_case("interrupted", interrupted_bytes)
            before = identity(target)
            publications = Publications(root / "changes")
            args = arguments("interrupted-apply", "apply", apply)
            child = subprocess.Popen([str(binary), "--json", *args], cwd=cwd, env=environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            active.append(child)
            try:
                publications.ledger(15)
                server.send_signal(signal.SIGSTOP)
                # Wait until /proc confirms stop before inspecting the caught state.
                deadline = time.monotonic() + 3
                while "\nState:\tT" not in Path(f"/proc/{server.pid}/status").read_text():
                    assert time.monotonic() < deadline, "server did not stop"
                    time.sleep(0.001)
                disk = helpers.strict_json((root / "changes/ledger.json").read_bytes())
                caught = next(record for record in disk["records"].values() if record["plan"]["request_id"] == "interrupted")
                caught_target = identity(target)
                retain("interrupted/caught-ledger.json", disk)
                retain("interrupted/caught-target.json", caught_target)
                stop(kill=True)
            finally:
                publications.close()
            stdout, stderr = child.communicate(timeout=35)
            lost = finish("interrupted-apply", subprocess.CompletedProcess(args, child.returncode, stdout, stderr), expected=None, args=args)
            assert lost["data"]["request_id"] == "interrupted"
            assert caught["consumed"] and caught["prepared"] is not None
            start()
            assert identity(target) == caught_target, "restart repeated a target mutation"
            lookup = {"requester": REQUESTER, "request_id": "interrupted"}
            status = change("interrupted-status", "status", lookup, expected=None)
            assert status["data"]["plan"] == planned["data"]["plan"]
            reconciled = change("interrupted-reconcile", "reconcile", lookup, expected=None)
            assert reconciled["data"]["plan"] == planned["data"]["plan"]
            assert identity(target) == caught_target, "reconcile repeated replacement"
            if caught_target["ino"] == before["ino"]:
                assert target.read_bytes() == proposed and reconciled["data"]["state"] == "not_applied"
            else:
                assert caught_target["ino"] == caught["prepared"]["snapshot"]["file"]["ino"]
                assert target.read_bytes() == interrupted_bytes and reconciled["data"]["state"] == "verified"
            delivered_again = change("interrupted-repeated-delivery", "apply", apply, expected=None)
            assert delivered_again["data"]["state"] == reconciled["data"]["state"]
            assert identity(target) == caught_target, "repeated delivery replayed interrupted replacement"
            if caught["state"] != "dispatching":
                gaps.append("SIGKILL caught a completed durable attempt; interruption specifically between dispatch intent and final publication was not demonstrated.")
            if caught_target["ino"] != before["ino"]:
                gaps.append("SIGKILL did not catch the pre-rename side of durable dispatch intent; original-inode interrupted reconciliation remains unproved by this external run.")
            lifecycles.append({"case": "real-sigkill-apply-restart", "caught_state": caught["state"], "before": before,
                               "caught_target": caught_target, "reconciled_state": reconciled["data"]["state"],
                               "client_execution": lost["execution"]["status"], "restart_and_reconcile_did_not_replace": True})
            stop()

            if options.small_store is not None:
                small = options.small_store.resolve()
                assert small.parent == Path("/tmp/evidence-small-disk") and not small.exists()
                assert os.statvfs(small.parent).f_blocks * os.statvfs(small.parent).f_frsize == 65536
                configuration["changes"]["state_dir"] = str(small)
                configuration["state_dir"] = str(root / "small-jobs")
                start()
                current = target.read_bytes()
                disk_proposed = current + b"\n# actual private filesystem exhaustion\n"
                apply, _ = approve_case("enospc", disk_proposed)
                assert (small / "ledger.json").stat().st_size > 4096
                filler = small.parent / "change-filler"
                with filler.open("xb", buffering=0) as handle:
                    while True:
                        try:
                            handle.write(b"x" * 4096)
                        except OSError as error:
                            assert error.errno == errno.ENOSPC, error
                            actual_enospc = error.errno
                            break
                    # One page permits validator staging, then leaves a partial ledger
                    # write at durable dispatch-intent publication (the target is elsewhere).
                    assert handle.tell() >= 4096
                    handle.truncate(handle.tell() - 4096)
                    os.fsync(handle.fileno())
                before = identity(target)
                refused = change("enospc-apply", "apply", apply, expected=None)
                assert refused["execution"]["status"] != "succeeded"
                assert refused["effect_outcome"] == "unknown", refused
                assert identity(target) == before
                snapshot = files(small)
                assert set(snapshot) - {"lock", "ledger.json"}, "Expected retained partial ledger artifact"
                for name in snapshot:
                    retain(f"enospc/files/{name}", (small / name).read_bytes())
                subsequent = change("enospc-poisoned-status", "status", {"requester": REQUESTER, "request_id": "enospc"}, expected=None)
                assert subsequent["execution"]["status"] != "succeeded" and files(small) == snapshot
                stop()
                # Leave partial bytes and filler intact: startup must refuse them as found.
                restart = subprocess.run([str(binary), "--json", "--record", "always", "runner", "serve", "--server-config", str(root / "server.json")],
                                         cwd=cwd, env=environment, capture_output=True, timeout=10)
                failed_start = finish("enospc-restart-refused", restart, expected=None)
                assert restart.returncode != 0 and failed_start["execution"]["status"] != "succeeded"
                assert files(small) == snapshot and identity(target) == before
                retain("enospc/files.json", snapshot)
                lifecycles.append({"case": "private-64kib-enospc", "observed_errno": actual_enospc,
                                   "target_unchanged": True, "partial_artifacts_preserved": True, "restart_refused": True})
            else:
                gaps.append("Private 64KiB ENOSPC acceptance not run; pass --small-store under tools/test-sandbox.sh --small-evidence-disk.")
        finally:
            for process in active:
                if process.poll() is None:
                    process.send_signal(signal.SIGCONT)
                    process.terminate()
                    try:
                        process.wait(timeout=4)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait(timeout=3)
        assert sha(binary.read_bytes()) == options.expected_binary_sha256
        assert sha(promtool.read_bytes()) == PROMTOOL_SHA256
    helpers.source_guard(ROOT, options.source_manifest, options.expected_source_manifest_sha256)
    assert sha(original.read_bytes()) == options.expected_binary_sha256 and sha(upstream.read_bytes()) == PROMTOOL_SHA256
    retain("lifecycles.json", lifecycles)
    retain("verification.json", {"passed": True, "commands": commands, "lifecycles": lifecycles, "remaining_gaps": gaps,
                                 "binary_sha256": options.expected_binary_sha256, "promtool_sha256": PROMTOOL_SHA256,
                                 "source_manifest_sha256": options.expected_source_manifest_sha256})
    print(f"PASS: {len(commands)} installed change commands; concurrent apply, real SIGKILL/restart and preserved failure evidence; {len(gaps)} explicit gaps")


if __name__ == "__main__":
    main()
