"""Installed authenticated runner mission; execute only through tools/test-sandbox.sh."""
from __future__ import annotations

import argparse
import base64
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import selectors
import shutil
import signal
import socket
import ssl
import struct
import subprocess
import tempfile
import time

from jsonschema import Draft202012Validator, FormatChecker
from referencing import Registry, Resource

ROOT = Path(__file__).resolve().parents[1]
RUNNER = "fixture.runner"
CAPACITY = "ext.example.capacity.project"


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def encoded(value):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"), allow_nan=False).encode()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--peer", type=Path, required=True)
    parser.add_argument("--adversary", type=Path, required=True)
    parser.add_argument("--expected-binary-sha256", required=True)
    parser.add_argument("--expected-peer-sha256", required=True)
    parser.add_argument("--expected-adversary-sha256", required=True)
    parser.add_argument("--source-manifest", type=Path, required=True)
    parser.add_argument("--expected-source-manifest-sha256", required=True)
    parser.add_argument("--evidence-dir", type=Path, required=True)
    parser.add_argument("--small-store", type=Path, help="Fresh runner store under the sandbox's private 64KiB /tmp/evidence-small-disk")
    options = parser.parse_args()
    assert os.environ.get("CARGO_HOME") == "/tmp/cargo" and Path("/toolchain").is_dir(), "Use tools/test-sandbox.sh"
    spec = importlib.util.spec_from_file_location("runner_helpers", ROOT / "tools/verify-evidence.py")
    helpers = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helpers)
    source = helpers.source_guard(ROOT, options.source_manifest, options.expected_source_manifest_sha256)
    assert source["tools/verify-runner.py"] == sha(Path(__file__).read_bytes())
    originals = {options.binary.resolve(strict=True): options.expected_binary_sha256,
                 options.peer.resolve(strict=True): options.expected_peer_sha256,
                 options.adversary.resolve(strict=True): options.expected_adversary_sha256}
    for path, expected in originals.items():
        assert sha(path.read_bytes()) == expected
    evidence = options.evidence_dir.resolve()
    evidence.mkdir(mode=0o700, parents=True, exist_ok=False)
    schemas = {p.stem.removesuffix(".schema"): helpers.strict_json(p.read_bytes())
               for p in (ROOT / "docs/sre-workbench/schemas").glob("*.schema.json")}
    registry = Registry().with_resources((s["$id"], Resource.from_contents(s)) for s in schemas.values())
    validators = {name: Draft202012Validator(schemas[name], registry=registry, format_checker=FormatChecker())
                  for name in ("result", "runner-data", "runner-client-config", "runner-server-config", "runner-wire-request", "runner-wire-response", "extension-package", "extension-policy", "capability")}
    commands, lifecycles, processes, orphan_handles = [], [], [], []
    environment = {"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8", "TZ": "UTC"}

    def retain(name, value):
        raw = value if isinstance(value, bytes) else (json.dumps(value, sort_keys=True, indent=2) + "\n").encode()
        path = evidence / name
        path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        path.write_bytes(raw)

    def validate(raw):
        result = helpers.strict_json(raw)
        validators["result"].validate(result)
        assert result["assessment"] == "not_assessed"
        if result["operation"].startswith("runner.") and "admission" in result["data"]:
            validators["runner-data"].validate(result["data"])
            if result["data"].get("result") is not None:
                validators["result"].validate(result["data"]["result"])
        return result

    with tempfile.TemporaryDirectory(prefix="installed-runner-") as temporary:
        root = Path(temporary)
        prefix = root / "prefix/bin"
        prefix.mkdir(parents=True)
        binary = prefix / "save"
        peer, adversary = prefix / "capacity-projector", prefix / "extension-peer-fixture"
        for original, installed in zip(originals, (binary, peer, adversary), strict=True):
            shutil.copyfile(original, installed)
            installed.chmod(0o500)
        cwd = root / "unrelated"
        cwd.mkdir()
        pki = root / "pki"
        pki.mkdir(mode=0o700)

        def private(path, content):
            path.write_bytes(content)
            path.chmod(0o600)

        def openssl(*args):
            observed = subprocess.run(["/usr/bin/openssl", *map(str, args)], cwd=pki,
                                      env={**environment, "OPENSSL_CONF": "/dev/null"}, capture_output=True, timeout=10)
            assert observed.returncode == 0, (args[0], observed.stderr[:300])
            return observed.stdout

        openssl("req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256", "-nodes",
                "-keyout", "ca.key", "-out", "ca.pem", "-days", "2", "-subj", "/CN=Runner acceptance CA",
                "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign,cRLSign")
        openssl("x509", "-in", "ca.pem", "-outform", "DER", "-out", "ca.der")
        pins = {}
        for index, identity in enumerate(("server", "a", "b", "unknown"), 1):
            hostname = "runner.test" if identity == "server" else f"{identity}.test"
            usage = "serverAuth" if identity == "server" else "clientAuth"
            openssl("req", "-new", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256", "-nodes",
                    "-keyout", f"{identity}.key.pem", "-out", f"{identity}.csr", "-subj", f"/CN={hostname}")
            private(pki / "extensions.cnf", f"basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage={usage}\nsubjectAltName=DNS:{hostname}\n".encode())
            openssl("x509", "-req", "-in", f"{identity}.csr", "-CA", "ca.pem", "-CAkey", "ca.key",
                    "-set_serial", str(index), "-days", "1", "-extfile", "extensions.cnf", "-out", f"{identity}.pem")
            openssl("x509", "-in", f"{identity}.pem", "-outform", "DER", "-out", f"{identity}.der")
            openssl("pkcs8", "-topk8", "-nocrypt", "-in", f"{identity}.key.pem", "-outform", "DER", "-out", f"{identity}.key.der")
            pins[identity] = "sha256:" + sha((pki / f"{identity}.der").read_bytes())
        for path in pki.iterdir():
            path.chmod(0o600)
        retain("tls-identities.json", {"leaf_sha256": pins, "openssl_version": openssl("version").decode().strip(),
                                       "tls_version": "TLSv1.3", "alpn": "save-runner/1", "private_material_retained": False})

        def finish(name, observed, expected=0, args=()):
            retain(f"commands/{name}.stdout.json", observed.stdout)
            retain(f"commands/{name}.stderr", observed.stderr)
            result = validate(observed.stdout)
            commands.append({"name": name, "exit_code": observed.returncode, "args": list(args)})
            assert (expected is None or observed.returncode == expected) and not observed.stderr, (name, observed.returncode, result, observed.stderr[:300])
            return result

        def local(name, args, expected=0):
            observed = subprocess.run([str(binary), "--json", *args], cwd=cwd, env=environment, capture_output=True, timeout=15)
            return finish(name, observed, expected, args)

        description = local("runner-submit-description", ["capabilities", "describe", "runner.submit"])
        capability = description["data"]["capability"]
        validators["capability"].validate(capability)
        assert capability["effects"] == "unclassified"
        submit_validator = Draft202012Validator(capability["output_schema"], registry=registry, format_checker=FormatChecker())
        binding = local("task-description", ["task", "describe", "error-budget"])["data"]["source"]["binding_digest"]
        state = root / "runner-state"
        server_config = root / "server.json"
        configuration = {"config_version": 1, "runner_id": RUNNER, "listen": "127.0.0.1:0", "state_dir": str(state),
                         "ca_certificate": str(pki / "ca.der"), "certificate": str(pki / "server.der"), "private_key": str(pki / "server.key.der"),
                         "principals": [{"id": f"principal.{identity}", "certificate_sha256": pins[identity]} for identity in ("a", "b")],
                         "targets": [{"id": "fixture.target", "snapshot": {"name": "Acceptance target", "environment": "isolated-sandbox"}}],
                         "grants": [{"principal": f"principal.{identity}", "target": "fixture.target", "operation": "task.run", "operation_version": 1,
                                     "task": {"id": "error-budget", "version": 1, "binding_sha256": binding}, "max_runtime_ms": 30000} for identity in ("a", "b")]}
        task_grants = copy.deepcopy(configuration["grants"])
        current = None
        server_generation = 0

        def start():
            nonlocal current, server_generation
            server_generation += 1
            validators["runner-server-config"].validate(configuration)
            private(server_config, encoded(configuration))
            retain(f"servers/{server_generation}-config.json", configuration)
            process = subprocess.Popen([str(binary), "--json", "--record", "always", "runner", "serve", "--server-config", str(server_config)],
                                       cwd=cwd, env=environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            processes.append(process)
            # The explicit server writes only its small readiness frame until it
            # exits. Read it with a finite poll, leaving the pipe open afterward.
            with selectors.DefaultSelector() as selector:
                selector.register(process.stdout, selectors.EVENT_READ)
                deadline = time.monotonic() + 10
                raw = bytearray()
                while b"\n" not in raw:
                    assert process.poll() is None, ("server exited before readiness", process.stderr.read(4096))
                    remaining = deadline - time.monotonic()
                    assert remaining > 0 and selector.select(remaining), "runner readiness timed out"
                    chunk = os.read(process.stdout.fileno(), 65536 - len(raw))
                    assert chunk, "readiness EOF"
                    raw.extend(chunk)
                    assert len(raw) < 65536
            ready = helpers.strict_json(bytes(raw).strip())
            assert ready["runner_id"] == RUNNER and ready["protocol"] == 1 and ready["server_certificate_sha256"] == pins["server"]
            retain(f"servers/{server_generation}-ready.json", ready)
            current = {"process": process, "generation": server_generation, "ready": ready}
            return current

        def stop(kill=False):
            nonlocal current
            server = current
            assert server is not None
            process = server["process"]
            started = time.monotonic()
            process.kill() if kill else process.terminate()
            stdout, stderr = process.communicate(timeout=5)
            retain(f"servers/{server['generation']}-exit.stdout", stdout)
            retain(f"servers/{server['generation']}-exit.stderr", stderr)
            assert process.returncode == (-signal.SIGKILL if kill else 143), (process.returncode, stdout, stderr)
            assert not stdout and not stderr
            lifecycles.append({"case": "server-killed" if kill else "server-stopped", "generation": server["generation"],
                               "exit_code": process.returncode, "elapsed_ms": round((time.monotonic() - started) * 1000)})
            current = None

        def client_config(identity="a"):
            return {"config_version": 1, "endpoint": current["ready"]["address"], "server_name": "runner.test", "runner_id": RUNNER,
                    "server_certificate_sha256": pins["server"], "ca_certificate": str(pki / "ca.der"),
                    "certificate": str(pki / f"{identity}.der"), "private_key": str(pki / f"{identity}.key.der")}

        def remote(name, method, request_id=None, identity="a", body=None, expected=0, override=None, after=0, record=None, runner_id=RUNNER):
            config = override if override is not None else client_config(identity)
            validators["runner-client-config"].validate(config)
            config_path = root / "client.json"
            private(config_path, encoded(config))
            args = ["--runner-config", str(config_path)]
            if record is True or (record is None and method in ("submit", "cancel")):
                args += ["--record", "always"]
            args += ["runner", method, runner_id]
            if method == "submit":
                body_path = root / "body.json"
                private(body_path, encoded(body))
                args += ["--input", str(body_path)]
            elif request_id is not None:
                args += [request_id]
            if method == "watch":
                args += ["--after-sequence", str(after)]
            return local(name, args, expected)

        def submission(request_id, operation="task.run", runtime=30000):
            inputs = {"id": "error-budget", "version": 1, "input": {"slo": 99.9, "bad_minutes": 40.32}} if operation == "task.run" else {"capacity_bytes": 1500, "used_bytes": 1000, "growth_bytes_per_day": 100}
            return {"request_id": request_id, "target": "fixture.target", "operation": operation, "operation_version": 1,
                    "inputs": inputs, "max_runtime_ms": runtime, "retain_events": 2}

        def await_state(name, request_id, desired=("terminal",), identity="a"):
            deadline = time.monotonic() + 12
            count = 0
            while True:
                result = remote(f"{name}-{count}", "status", request_id, identity)
                if result["data"].get("job") and result["data"]["job"]["state"] in desired:
                    return result
                assert time.monotonic() < deadline, (name, result)
                time.sleep(0.05)
                count += 1

        def raw_tls(identity="a"):
            context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
            context.minimum_version = context.maximum_version = ssl.TLSVersion.TLSv1_3
            context.load_verify_locations(cafile=str(pki / "ca.pem"))
            context.load_cert_chain(str(pki / f"{identity}.pem"), str(pki / f"{identity}.key.pem"))
            context.set_alpn_protocols(["save-runner/1"])
            host, port = current["ready"]["address"].rsplit(":", 1)
            stream = context.wrap_socket(socket.create_connection((host, int(port)), timeout=3), server_hostname="runner.test")
            assert stream.version() == "TLSv1.3" and stream.selected_alpn_protocol() == "save-runner/1"
            assert "sha256:" + sha(stream.getpeercert(binary_form=True)) == pins["server"]
            return stream

        def receive(stream):
            def exact(size):
                data = bytearray()
                while len(data) < size:
                    chunk = stream.recv(size - len(data))
                    assert chunk, "unexpected RPC EOF"
                    data.extend(chunk)
                return bytes(data)
            length = struct.unpack("!I", exact(4))[0]
            assert 0 < length <= 2 * 1024 * 1024
            result = helpers.strict_json(exact(length))
            validators["runner-wire-response"].validate(result)
            return result

        def package(executable, version):
            manifest = helpers.strict_json((ROOT / "examples/extensions/capacity/manifest.json").read_bytes())
            manifest["version"] = version
            raw = executable.read_bytes()
            manifest["executable"].update(sha256="sha256:" + sha(raw), bytes=len(raw))
            value = {"package_format": 1, "manifest": manifest, "executable_base64": base64.b64encode(raw).decode()}
            validators["extension-package"].validate(value)
            content = encoded(value)
            path = root / f"package-{version}.json"
            private(path, content)
            return path, "sha256:" + sha(content)

        def extension_grant(package_pin):
            policy_value = {"policy_version": 1, "execution_profile": "operator-reviewed-v1", "packages": [{"package_sha256": package_pin, "capabilities": [CAPACITY]}]}
            validators["extension-policy"].validate(policy_value)
            policy = root / "extension-policy.json"
            content = encoded(policy_value)
            private(policy, content)
            configuration["extension_state_dir"] = str(root / "extensions")
            configuration["extension_policy_path"] = str(policy)
            configuration["grants"] = [{"principal": f"principal.{identity}", "target": "fixture.target", "operation": CAPACITY, "operation_version": 1,
                                         "package_sha256": package_pin, "policy_sha256": "sha256:" + sha(content), "max_runtime_ms": 3600000} for identity in ("a", "b")]

        def child_ready(mode):
            marker = Path(f"/tmp/extension-peer-fixture-{mode}.pid")
            deadline = time.monotonic() + 8
            while not marker.exists() or not marker.read_bytes():
                assert time.monotonic() < deadline and current["process"].poll() is None, "remote fixture readiness timed out"
                time.sleep(0.02)
            pid = int(marker.read_text())
            parent = next(line.split()[1] for line in Path(f"/proc/{pid}/status").read_text().splitlines() if line.startswith("PPid:"))
            assert int(parent) == current["process"].pid, "fixture PID must belong to this live server"
            fd = os.pidfd_open(pid)
            orphan_handles.append(fd)
            return marker, pid, fd

        try:
            start()
            handshake = remote("handshake-a", "handshake")
            assert handshake["data"]["principal"]["id"] == "principal.a"
            assert handshake["data"]["capabilities"][0]["implementation_sha256"] == binding
            assert handshake["data"]["clock"]["offset_established"] is False
            for method in ("submit", "cancel"):
                refused = remote(f"{method}-recording-required", method, request_id="no-consent", body=submission("no-consent"), record=False, expected=2)
                assert any(error["code"] == "recording_required" for error in refused["errors"])
                assert refused["effect_outcome"] == "not_attempted"
            mismatched = remote("runner-id-mismatch", "submit", body=submission("wrong-runner"), runner_id="wrong.runner", expected=2)
            assert any(error["code"] == "runner_identity_mismatch" for error in mismatched["errors"])
            assert mismatched["effect_outcome"] == "not_attempted"
            body = submission("task-once")
            first = remote("submit-task", "submit", body=body)
            assert first["data"]["admission"] == "accepted"
            submit_validator.validate(first)
            duplicate = remote("duplicate-task", "submit", body=body)
            assert duplicate["data"]["job"]["job_id"] == first["data"]["job"]["job_id"]
            altered = copy.deepcopy(body)
            altered["inputs"]["input"]["bad_minutes"] = 1
            conflict = remote("conflicting-task", "submit", body=altered, expected=2)
            assert conflict["data"]["remote_error"] == "job_request_conflict"
            absent = remote("principal-b-cannot-see-a", "status", "task-once", identity="b")
            assert absent["data"]["admission"] == "not_found" and absent["data"]["result"] is None
            b_job = remote("principal-b-same-id", "submit", identity="b", body=body)
            assert b_job["data"]["job"]["job_id"] != first["data"]["job"]["job_id"]
            task = await_state("task-terminal", "task-once")
            assert task["data"]["result"]["execution"]["status"] == "succeeded"
            assert task["data"]["result"]["data"]["calculation"]["status"]["remaining"] == 0
            task_digest = task["data"]["result_sha256"]
            controller = root / "text-controller.json"
            private(controller, encoded(client_config()))
            text_args = ["--runner-config", str(controller), "runner", "status", RUNNER, "task-once"]
            human = subprocess.run([str(binary), *text_args], cwd=cwd, env=environment, capture_output=True, timeout=15)
            retain("commands/terminal-status-text.stdout.txt", human.stdout)
            retain("commands/terminal-status-text.stderr", human.stderr)
            commands.append({"name": "terminal-status-text", "exit_code": human.returncode, "args": text_args})
            assert human.returncode == 0 and not human.stderr
            for text in (RUNNER, "principal.a", "task-once", '"state":"terminal"', "Stored child receipt (untrusted output)"):
                assert text.encode() in human.stdout, text
            assert task["data"]["job"]["job_id"].encode() in human.stdout
            for method in ("status", "watch"):
                inputs = {"request_id": "task-once"}
                if method == "watch":
                    inputs["after_sequence"] = 0
                limited_request = {"spec_version": "0.1", "request_id": f"limited-{method}", "operation": f"runner.{method}", "operation_version": 1,
                                   "target": {"kind": "runner", "id": RUNNER}, "inputs": inputs,
                                   "limits": {"timeout_ms": 30000, "max_output_bytes": 1024}, "record": "never"}
                limited_path = root / f"limited-{method}.json"
                private(limited_path, encoded(limited_request))
                limited = local(f"limited-{method}", ["--runner-config", str(controller), "call", "--request", str(limited_path)], expected=1)
                assert limited["execution"]["status"] == "partial"
                assert limited["data"]["result"] is None and limited["data"]["result_omitted"] is True
                assert limited["data"]["result_sha256"] == task_digest
                if method == "watch":
                    events, cursor = limited["data"]["events"], limited["data"]["cursor"]
                    expected_next = events[-1]["sequence"] if events else cursor["after_sequence"]
                    assert cursor["next_sequence"] == expected_next
                    remaining = remote("limited-watch-reconnect", "watch", "task-once", after=expected_next)
                    assert all(event["sequence"] > expected_next for event in remaining["data"]["events"])
                    assert remaining["data"]["result_sha256"] == task_digest
            watch = remote("watch-gap", "watch", "task-once")
            assert watch["data"]["cursor"]["gap"] is not None
            reconnect = remote("watch-reconnect", "watch", "task-once", after=watch["data"]["cursor"]["next_sequence"])
            assert not reconnect["data"]["events"] and reconnect["data"]["result_sha256"] == task_digest
            remote("runtime-grant-refused", "submit", body=submission("too-long", runtime=30001), expected=2)
            wrong_target = submission("wrong-target")
            wrong_target["target"] = "ungranted.target"
            remote("target-grant-refused", "submit", body=wrong_target, expected=2)
            wrong_operation = submission("wrong-operation")
            wrong_operation["operation_version"] = 2
            remote("operation-version-refused", "submit", body=wrong_operation, expected=2)

            for field, value, code in (("server_name", "wrong.test", "runner_tls_failed"),
                                       ("server_certificate_sha256", "sha256:" + "0" * 64, "runner_server_identity_mismatch"),
                                       ("ca_certificate", str(root / "missing-ca.der"), "runner_config_unavailable")):
                config = client_config()
                config[field] = value
                refused = remote(f"identity-{field}", "handshake", override=config, expected=2)
                assert any(error["code"] == code for error in refused["errors"])
            unknown = remote("unknown-principal", "handshake", identity="unknown", expected=2)
            assert unknown["data"]["principal"] is None

            lost_body = submission("lost-ack")
            wire = {"protocol": 1, "method": "submit", "inputs": lost_body}
            validators["runner-wire-request"].validate(wire)
            with raw_tls() as stream:
                raw = encoded(wire)
                stream.sendall(struct.pack("!I", len(raw)) + raw)
                # No response read and no submit retry: only the durable identity
                # is used below to discover what the target actually committed.
            reconciled = await_state("lost-ack-status", "lost-ack")
            assert reconciled["data"]["result"]["execution"]["status"] == "succeeded"
            lifecycles.append({"case": "lost-ack", "status_only_reconciliation": True, "job_id": reconciled["data"]["job"]["job_id"]})
            with raw_tls() as stream:
                invalid = encoded({"protocol": 2, "method": "submit", "inputs": submission("bad-protocol")})
                stream.sendall(struct.pack("!I", len(invalid)) + invalid)
                refusal = receive(stream)
                retain("wire/unsupported-protocol.json", refusal)
                assert refusal["ok"] is False and refusal["error"] == "runner_invalid_protocol"
            assert remote("bad-protocol-not-admitted", "status", "bad-protocol")["data"]["admission"] == "not_found"
            await_state("principal-b-terminal", "task-once", identity="b")
            host, port = current["ready"]["address"].rsplit(":", 1)
            stalled = socket.create_connection((host, int(port)), timeout=2)
            time.sleep(0.05)
            stop()
            stalled.close()
            configuration["grants"] = []
            start()
            revoked = remote("revoked-historical-status", "status", "task-once")
            assert revoked["data"]["result_sha256"] == task_digest
            remote("revoked-new-submit", "submit", body=submission("revoked-new"), expected=2)
            stop()

            # Extension admission pins both independently installed package and
            # target policy bytes; no package metadata can create the grant.
            for executable, version, name in ((peer, 1, "capacity"), (adversary, 2, "adversary")):
                path, package_pin = package(executable, version)
                installed = local(f"install-{name}", ["--extension-state-dir", str(root / "extensions"), "extension", "install", str(path), "--expected-package-sha256", package_pin])
                assert installed["data"]["enabled"] is False
                extension_grant(package_pin)
                start()
                if name == "capacity":
                    remote("capacity-submit", "submit", body=submission("capacity-real", CAPACITY, 3600000))
                    capacity = await_state("capacity-terminal", "capacity-real")
                    child = capacity["data"]["result"]
                    assert child["execution"]["status"] == "succeeded"
                    assert child["effective_limits"]["timeout_ms"] > 300000
                    assert child["data"]["terminal"]["data"]["headroom_bytes"] == 500
                    assert child["data"]["terminal"]["data"]["days_remaining_floor"] == 5
                    stop()
                    continue
                for mode in ("stubborn", "timeout"):
                    marker = Path(f"/tmp/extension-peer-fixture-{mode}.pid")
                    if marker.exists():
                        marker.unlink()
                remote("remote-cancel-submit", "submit", body=submission("fixture-stubborn", CAPACITY, 30000))
                marker, pid, fd = child_ready("stubborn")
                acknowledgement = remote("remote-cancel-intent", "cancel", "fixture-stubborn")
                assert acknowledgement["data"]["job"]["cancel_requested"] is True
                cancelled = await_state("remote-cancel-observed", "fixture-stubborn")
                assert cancelled["data"]["result"]["execution"]["status"] == "cancelled"
                assert cancelled["data"]["result"]["data"]["supervisor"]["cleanup"]["confirmed"] is True
                lifecycles.append({"case": "remote-cancellation", "intent_separate_from_terminal": True, "child_pid": pid})
                # Principal B owns an independent same-ID timeout request. Its
                # connection disappears without consuming the ACK; the original
                # finite runtime remains in force on the target.
                disconnected = submission("fixture-timeout", CAPACITY, 2000)
                started = time.monotonic()
                with raw_tls("b") as stream:
                    wire = encoded({"protocol": 1, "method": "submit", "inputs": disconnected})
                    stream.sendall(struct.pack("!I", len(wire)) + wire)
                timed_out = await_state("remote-disconnected-runtime", "fixture-timeout", identity="b")
                assert timed_out["data"]["job"]["max_runtime_ms"] == 2000
                assert timed_out["data"]["result"]["execution"]["status"] == "timed_out"
                assert timed_out["data"]["result"]["effective_limits"]["timeout_ms"] <= 2000
                assert timed_out["data"]["result"]["data"]["supervisor"]["cleanup"]["confirmed"] is True
                assert time.monotonic() - started < 8
                lifecycles.append({"case": "disconnected-finite-runtime", "principal": "principal.b", "runtime_ms": 2000, "terminal": "timed_out", "cleanup_confirmed": True})
                timeout_marker = Path("/tmp/extension-peer-fixture-timeout.pid")
                if timeout_marker.exists():
                    timeout_marker.unlink()
                crash_body = submission("fixture-timeout", CAPACITY, 30000)
                crash_admission = remote("remote-crash-submit", "submit", body=crash_body)
                marker, pid, fd = child_ready("timeout")
                marker_before = marker.read_bytes()
                stop(kill=True)
                signal.pidfd_send_signal(fd, 0)
                start()
                recovered = await_state("remote-crash-reconciliation", "fixture-timeout", desired=("recovery_required",))
                assert recovered["data"]["result"] is None and recovered["data"]["job"]["effect_outcome"] == "unknown"
                duplicate = remote("remote-crash-duplicate", "submit", body=crash_body)
                assert duplicate["data"]["job"]["job_id"] == crash_admission["data"]["job"]["job_id"]
                assert marker.read_bytes() == marker_before
                signal.pidfd_send_signal(fd, 0)
                lifecycles.append({"case": "server-sigkill-restart", "state": "recovery_required", "no_automatic_replay": True, "original_fixture_left_alive": True})
                # Cleanup owns a live pidfd obtained while this was a verified
                # direct child of the test server, not a PID recovered from disk.
                signal.pidfd_send_signal(fd, signal.SIGKILL)
                stop()
            for path in state.rglob("*"):
                assert path.stat().st_mode & 0o077 == 0, path
            if options.small_store is not None:
                small = options.small_store.resolve()
                assert small.parent == Path("/tmp/evidence-small-disk") and not small.exists()
                configuration["state_dir"] = str(small)
                configuration["grants"] = task_grants
                configuration.pop("extension_state_dir", None)
                configuration.pop("extension_policy_path", None)
                start()
                unknown = False
                failure_observed = False
                for index in range(64):
                    response = remote(f"small-store-submit-{index}", "submit", body=submission(f"disk-full-{index}"), expected=None)
                    unknown |= response["effect_outcome"] == "unknown" or response["data"]["admission"] == "unknown"
                    if response["execution"]["status"] != "succeeded" or current["process"].poll() is not None:
                        failure_observed = True
                        break
                assert failure_observed, "small private filesystem did not exercise persistence failure"
                process = current["process"]
                if process.poll() is None:
                    process.terminate()
                stdout, stderr = process.communicate(timeout=5)
                retain("disk-full/server-exit.stdout", stdout)
                retain("disk-full/server-exit.stderr", stderr)
                assert not stderr
                if stdout:
                    exit_receipt = validate(stdout)
                    unknown |= exit_receipt["effect_outcome"] == "unknown"
                current = None
                assert unknown, "remote persistence uncertainty was not disclosed"
                before = {str(path.relative_to(small)): sha(path.read_bytes()) for path in small.rglob("*") if path.is_file()}
                assert before
                refusal = local("disk-full-restart-refused", ["--record", "always", "runner", "serve", "--server-config", str(server_config)], expected=None)
                assert refusal["execution"]["status"] != "succeeded"
                after = {str(path.relative_to(small)): sha(path.read_bytes()) for path in small.rglob("*") if path.is_file()}
                assert before == after, "interrupted remote store bytes changed during refusal"
                retain("disk-full/files.json", before)
                lifecycles.append({"case": "remote-private-64kib-enospc", "uncertainty_disclosed": True, "partial_store_preserved": True, "files": len(before)})
        finally:
            for process in processes:
                if process.poll() is None:
                    process.terminate()
                    try:
                        process.communicate(timeout=4)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.communicate(timeout=3)
            for fd in orphan_handles:
                try:
                    signal.pidfd_send_signal(fd, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                finally:
                    os.close(fd)

    helpers.source_guard(ROOT, options.source_manifest, options.expected_source_manifest_sha256)
    for path, expected in originals.items():
        assert sha(path.read_bytes()) == expected
    retain("lifecycles.json", lifecycles)
    retain("verification.json", {"passed": True, "commands": commands, "lifecycles": lifecycles,
                                 "binary_sha256": options.expected_binary_sha256, "peer_sha256": options.expected_peer_sha256,
                                 "adversary_sha256": options.expected_adversary_sha256,
                                 "source_manifest_sha256": options.expected_source_manifest_sha256})
    print(f"PASS: {len(commands)} installed commands; authenticated identity, durable admission, reconciliation and remote cancellation")


if __name__ == "__main__":
    main()
