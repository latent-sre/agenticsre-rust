"""Installed bounded fleet mission; execute only through tools/test-sandbox.sh."""
from __future__ import annotations

import argparse
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import selectors
import shutil
import signal
import subprocess
import tempfile
import time

from jsonschema import Draft202012Validator, FormatChecker
from referencing import Registry, Resource

ROOT = Path(__file__).resolve().parents[1]


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def encoded(value):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"), allow_nan=False).encode()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--expected-binary-sha256", required=True)
    parser.add_argument("--source-manifest", type=Path, required=True)
    parser.add_argument("--expected-source-manifest-sha256", required=True)
    parser.add_argument("--evidence-dir", type=Path, required=True)
    parser.add_argument("--small-store", type=Path, help="Fresh fleet store under the sandbox's private 64KiB /tmp/evidence-small-disk")
    options = parser.parse_args()
    assert os.environ.get("CARGO_HOME") == "/tmp/cargo" and Path("/toolchain").is_dir(), "Use tools/test-sandbox.sh"
    spec = importlib.util.spec_from_file_location("fleet_helpers", ROOT / "tools/verify-evidence.py")
    helpers = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helpers)
    source = helpers.source_guard(ROOT, options.source_manifest, options.expected_source_manifest_sha256)
    assert source["tools/verify-fleet.py"] == sha(Path(__file__).read_bytes())
    original = options.binary.resolve(strict=True)
    assert sha(original.read_bytes()) == options.expected_binary_sha256
    evidence = options.evidence_dir.resolve()
    evidence.mkdir(mode=0o700, parents=True, exist_ok=False)
    schemas = {path.stem.removesuffix(".schema"): helpers.strict_json(path.read_bytes())
               for path in (ROOT / "docs/sre-workbench/schemas").glob("*.schema.json")}
    registry = Registry().with_resources((schema["$id"], Resource.from_contents(schema)) for schema in schemas.values())
    validators = {name: Draft202012Validator(schemas[name], registry=registry, format_checker=FormatChecker())
                  for name in ("result", "fleet-data", "fleet-inventory", "fleet-collect-input", "runner-data", "runner-client-config", "runner-server-config", "capability")}
    commands, lifecycles, active = [], [], []
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
        if result["operation"].startswith("fleet.") and "aggregate" in result["data"]:
            validators["fleet-data"].validate(result["data"])
            assert sum(result["data"]["counts"].values()) == len(result["data"]["targets"])
            assert len({target["id"] for target in result["data"]["targets"]}) == len(result["data"]["targets"])
        elif result["operation"].startswith("runner.") and "admission" in result["data"]:
            validators["runner-data"].validate(result["data"])
        return result

    with tempfile.TemporaryDirectory(prefix="installed-fleet-") as temporary:
        root = Path(temporary)
        binary = root / "prefix/bin/save"
        binary.parent.mkdir(parents=True)
        shutil.copyfile(original, binary)
        binary.chmod(0o500)
        cwd = root / "unrelated"
        cwd.mkdir()
        pki = root / "pki"
        pki.mkdir(mode=0o700)
        inventory_path, store = root / "inventory.json", root / "fleet-store"
        nodes = []

        def private(path, raw):
            path.write_bytes(raw)
            path.chmod(0o600)

        def openssl(*args):
            completed = subprocess.run(["/usr/bin/openssl", *map(str, args)], cwd=pki,
                                       env={**environment, "OPENSSL_CONF": "/dev/null"}, capture_output=True, timeout=10)
            assert completed.returncode == 0, (args[0], completed.stderr[:300])
            return completed.stdout

        openssl("req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256", "-nodes",
                "-keyout", "ca.key", "-out", "ca.pem", "-days", "2", "-subj", "/CN=Fleet acceptance CA",
                "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign,cRLSign")
        openssl("x509", "-in", "ca.pem", "-outform", "DER", "-out", "ca.der")
        pins = {}
        for serial, identity in enumerate(("client", "server-0", "server-1"), 1):
            hostname = f"{identity}.test"
            openssl("req", "-new", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256", "-nodes", "-keyout", f"{identity}.key.pem", "-out", f"{identity}.csr", "-subj", f"/CN={hostname}")
            usage = "clientAuth" if identity == "client" else "serverAuth"
            private(pki / "extensions.cnf", f"basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage={usage}\nsubjectAltName=DNS:{hostname}\n".encode())
            openssl("x509", "-req", "-in", f"{identity}.csr", "-CA", "ca.pem", "-CAkey", "ca.key", "-set_serial", str(serial), "-days", "1", "-extfile", "extensions.cnf", "-out", f"{identity}.pem")
            openssl("x509", "-in", f"{identity}.pem", "-outform", "DER", "-out", f"{identity}.der")
            openssl("pkcs8", "-topk8", "-nocrypt", "-in", f"{identity}.key.pem", "-outform", "DER", "-out", f"{identity}.key.der")
            pins[identity] = "sha256:" + sha((pki / f"{identity}.der").read_bytes())
        for path in pki.iterdir():
            path.chmod(0o600)
        retain("tls-identities.json", {"leaf_sha256": pins, "openssl_version": openssl("version").decode().strip(), "private_material_retained": False})

        def finish(name, completed, expected=0, args=()):
            retain(f"commands/{name}.stdout.json", completed.stdout)
            retain(f"commands/{name}.stderr", completed.stderr)
            value = validate(completed.stdout)
            commands.append({"name": name, "exit_code": completed.returncode, "args": list(args)})
            assert (expected is None or completed.returncode == expected) and not completed.stderr, (name, completed.returncode, value, completed.stderr[:300])
            return value

        def local(name, args, expected=0):
            request_ms = int(args[args.index("--timeout-ms") + 1]) if "--timeout-ms" in args else 30000
            # The watchdog includes the public operation budget, its 2.5 s
            # cleanup window and fixture overhead; it must not kill valid waits.
            completed = subprocess.run([str(binary), "--json", *args], cwd=cwd, env=environment, capture_output=True, timeout=request_ms / 1000 + 4)
            return finish(name, completed, expected, args)

        binding = local("task-description", ["task", "describe", "error-budget"])["data"]["source"]["binding_digest"]
        for index in range(2):
            directory = root / f"runner-{index}"
            directory.mkdir(mode=0o700)
            runner_id = f"runner.{index}"
            server = {"config_version": 1, "runner_id": runner_id, "listen": "127.0.0.1:0", "state_dir": str(directory / "state"),
                      "ca_certificate": str(pki / "ca.der"), "certificate": str(pki / f"server-{index}.der"), "private_key": str(pki / f"server-{index}.key.der"),
                      "principals": [{"id": "fleet.operator", "certificate_sha256": pins["client"]}],
                      "targets": [{"id": "observed.target", "snapshot": {"name": f"Target {index}", "environment": "isolated-sandbox"}}],
                      "grants": [{"principal": "fleet.operator", "target": "observed.target", "operation": "task.run", "operation_version": 1,
                                  "task": {"id": "error-budget", "version": 1, "binding_sha256": binding}, "max_runtime_ms": 30000}]}
            client = {"config_version": 1, "endpoint": "127.0.0.1:0", "server_name": f"server-{index}.test", "runner_id": runner_id,
                      "server_certificate_sha256": pins[f"server-{index}"], "ca_certificate": str(pki / "ca.der"),
                      "certificate": str(pki / "client.der"), "private_key": str(pki / "client.key.der")}
            nodes.append({"index": index, "root": directory, "server": server, "client": client, "process": None, "generation": 0})

        def start(node):
            node["generation"] += 1
            validators["runner-server-config"].validate(node["server"])
            path = node["root"] / "server.json"
            private(path, encoded(node["server"]))
            retain(f"servers/{node['index']}-{node['generation']}-config.json", node["server"])
            process = subprocess.Popen([str(binary), "--json", "--record", "always", "runner", "serve", "--server-config", str(path)], cwd=cwd, env=environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            active.append(process)
            node["process"] = process
            with selectors.DefaultSelector() as selector:
                selector.register(process.stdout, selectors.EVENT_READ)
                deadline, raw = time.monotonic() + 10, bytearray()
                while b"\n" not in raw:
                    assert process.poll() is None, ("server exited before readiness", process.stderr.read(4096))
                    remaining = deadline - time.monotonic()
                    assert remaining > 0 and selector.select(remaining), "runner readiness timed out"
                    chunk = os.read(process.stdout.fileno(), 65536 - len(raw))
                    assert chunk, "readiness EOF"
                    raw.extend(chunk)
                    assert len(raw) < 65536
            ready = helpers.strict_json(bytes(raw).strip())
            assert ready["runner_id"] == node["server"]["runner_id"] and ready["server_certificate_sha256"] == node["client"]["server_certificate_sha256"]
            retain(f"servers/{node['index']}-{node['generation']}-ready.json", ready)
            node["server"]["listen"] = ready["address"]
            node["client"]["endpoint"] = ready["address"]
            validators["runner-client-config"].validate(node["client"])
            private(node["root"] / "client.json", encoded(node["client"]))

        def stop(node):
            process = node["process"]
            if process is None:
                return
            if process.poll() is None:
                process.send_signal(signal.SIGCONT)
                process.terminate()
            stdout, stderr = process.communicate(timeout=5)
            retain(f"servers/{node['index']}-{node['generation']}-exit.stdout", stdout)
            retain(f"servers/{node['index']}-{node['generation']}-exit.stderr", stderr)
            assert process.returncode == 143 and not stdout and not stderr, (process.returncode, stdout, stderr)
            node["process"] = None

        inventory = {"inventory_version": 1, "revision": "fixture-v1", "targets": [
            {"id": f"target.{node['index']}", "runner_id": node["server"]["runner_id"], "target": "observed.target",
             "runner_config": str(node["root"] / "client.json"), "principal": "fleet.operator"} for node in nodes]}

        def write_inventory():
            validators["fleet-inventory"].validate(inventory)
            private(inventory_path, encoded(inventory))

        def body(request_id, concurrency=2, threshold=2, targets=None):
            return {"request_id": request_id, "targets": ["target.0", "target.1"] if targets is None else targets,
                    "task": {"id": "error-budget", "version": 1, "input": {"slo": 99.9, "bad_minutes": 40.32}},
                    "concurrency": concurrency, "failure_threshold": threshold}

        def arguments(method, request_id=None, value=None, timeout=30000, budget=1048576, record=True):
            args = ["--fleet-inventory", str(inventory_path), "--fleet-state-dir", str(store)]
            if method != "status" and record:
                args += ["--record", "always"]
            args += ["fleet", method]
            if method == "collect":
                path = root / "collection.json"
                private(path, encoded(value))
                args += ["--input", str(path)]
            else:
                args += [request_id]
            return args + ["--timeout-ms", str(timeout), "--max-output-bytes", str(budget)]

        def fleet(name, method, request_id=None, value=None, expected=0, **kwargs):
            return local(name, arguments(method, request_id, value, **kwargs), expected)

        def job_ids(node):
            jobs = node["root"] / "state" / ("principal-" + sha(b"fleet.operator")) / "jobs"
            found = set()
            for path in jobs.glob("job-*"):
                try:
                    value = helpers.strict_json((path / "state.json").read_bytes())
                    if isinstance(value, dict) and (path / "request.json").is_file() and (path / "lease").is_file():
                        found.add(path.name)
                except (OSError, ValueError):
                    pass
            return found

        def child_jobs():
            return [job_ids(node) for node in nodes]

        def controller_start(request_id, concurrency=1, timeout=30000):
            args = arguments("collect", value=body(request_id, concurrency), timeout=timeout)
            stdout_path, stderr_path = root / "controller.stdout", root / "controller.stderr"
            with stdout_path.open("wb") as stdout, stderr_path.open("wb") as stderr:
                process = subprocess.Popen([str(binary), "--json", *args], cwd=cwd, env=environment, stdout=stdout, stderr=stderr)
            active.append(process)
            return process, args, stdout_path, stderr_path

        def controller_finish(name, controller, expected=1):
            process, args, stdout, stderr = controller
            process.wait(timeout=8)
            return finish(name, subprocess.CompletedProcess(args, process.returncode, stdout.read_bytes(), stderr.read_bytes()), expected, args)

        def pause_after_admission(controller, baseline):
            process = controller[0]
            deadline = time.monotonic() + 8
            while True:
                assert process.poll() is None, "controller completed before crash/cancel boundary"
                current = child_jobs()
                if any(current[index] - baseline[index] for index in range(2)):
                    for node in nodes:
                        node["process"].send_signal(signal.SIGSTOP)
                    current = child_jobs()
                    assert sum(len(current[index] - baseline[index]) for index in range(2)) == 1, "pause only after first serial admission"
                    assert process.poll() is None
                    return current
                assert time.monotonic() < deadline, "remote admission not observed"
                time.sleep(0.001)

        def resume_servers():
            for node in nodes:
                if node["process"] is not None and node["process"].poll() is None:
                    node["process"].send_signal(signal.SIGCONT)

        try:
            for node in nodes:
                start(node)
            write_inventory()
            description = local("fleet-description", ["capabilities", "describe", "fleet.collect"])
            validators["capability"].validate(description["data"]["capability"])
            output_schema = Draft202012Validator(description["data"]["capability"]["output_schema"], registry=registry)
            selected = body("frozen-once")
            validators["fleet-collect-input"].validate(selected)
            complete = fleet("complete", "collect", value=selected)
            output_schema.validate(complete)
            assert complete["data"]["aggregate"] == "succeeded" and complete["data"]["counts"]["succeeded"] == 2
            assert {target["id"] for target in complete["data"]["targets"]} == {"target.0", "target.1"}
            for target in complete["data"]["targets"]:
                child = target["receipt"]["data"]["result"]
                assert child["execution"]["status"] == "succeeded" and child["data"]["calculation"]["status"]["remaining"] == 0
                assert target["evidence_ref"] and target["result_sha256"]
            baseline = child_jobs()
            duplicate = fleet("duplicate", "collect", value=selected)
            assert duplicate["data"]["run"]["id"] == complete["data"]["run"]["id"] and child_jobs() == baseline
            conflict = fleet("conflict", "collect", value=body("frozen-once", targets=["target.0"]), expected=2)
            assert any(error["code"] == "fleet_request_conflict" for error in conflict["errors"])
            inventory["revision"], inventory["targets"] = "after-freeze", []
            write_inventory()
            observed = fleet("immutable-selection", "status", "frozen-once")
            assert observed["data"]["targets"] == complete["data"]["targets"] and child_jobs() == baseline
            inventory["revision"] = "fixture-v1"
            inventory["targets"] = [{"id": f"target.{node['index']}", "runner_id": node["server"]["runner_id"], "target": "observed.target", "runner_config": str(node["root"] / "client.json"), "principal": "fleet.operator"} for node in nodes]
            write_inventory()
            limited = fleet("limited-status", "status", "frozen-once", budget=1024, expected=1)
            assert limited["execution"]["status"] == "partial" and limited["data"]["counts"] == complete["data"]["counts"]
            for bounded, full in zip(limited["data"]["targets"], complete["data"]["targets"], strict=True):
                for key in ("id", "request_id", "outcome", "result_sha256", "evidence_ref"):
                    assert bounded[key] == full[key]
                assert bounded["receipt"] is None and bounded["receipt_omitted"]
            text_args = arguments("status", "frozen-once")
            human = subprocess.run([str(binary), *text_args], cwd=cwd, env=environment, capture_output=True, timeout=10)
            retain("commands/text-status.stdout.txt", human.stdout)
            retain("commands/text-status.stderr", human.stderr)
            assert human.returncode == 0 and not human.stderr
            for token in ("frozen-once", "target.0", "target.1", "succeeded"):
                assert token.encode() in human.stdout
            commands.append({"name": "text-status", "exit_code": human.returncode})
            for name, targets in (("empty", []), ("duplicate-target", ["target.0", "target.0"]), ("unknown-target", ["unknown.target"])):
                refused = fleet(name, "collect", value=body(name, targets=targets), expected=2)
                assert refused["effect_outcome"] == "not_attempted"
            refused = fleet("recording-required", "collect", value=body("no-consent"), record=False, expected=2)
            assert refused["effect_outcome"] == "not_attempted"

            stop(nodes[1])
            nodes[1]["server"]["grants"][0]["max_runtime_ms"] = 1
            start(nodes[1])
            mixed = fleet("mixed-timeout", "collect", value=body("mixed-timeout"), expected=1)
            assert mixed["data"]["aggregate"] == "partial" and mixed["data"]["counts"]["succeeded"] == mixed["data"]["counts"]["failed"] == 1
            failed = next(target for target in mixed["data"]["targets"] if target["outcome"] == "failed")
            assert failed["receipt"]["data"]["result"]["execution"]["status"] == "timed_out"
            stop(nodes[1])
            unreachable = fleet("unreachable", "collect", value=body("unreachable"), expected=1)
            assert unreachable["data"]["counts"]["succeeded"] == unreachable["data"]["counts"]["failed"] == 1
            assert len(unreachable["data"]["targets"]) == 2
            start(nodes[1])
            stop(nodes[0])
            nodes[0]["server"]["grants"][0]["max_runtime_ms"] = 1
            start(nodes[0])
            rounds = []
            for number in range(2):
                result = fleet(f"threshold-round-{number}", "collect", value=body(f"threshold-{number}", 1, 1), expected=1)
                assert result["data"]["counts"]["failed"] == result["data"]["counts"]["skipped"] == 1
                rounds.append(next(target["id"] for target in result["data"]["targets"] if target["outcome"] == "failed"))
            assert rounds[0] != rounds[1]
            lifecycles.append({"case": "persistent-rotation", "first_admissions": rounds, "failure_threshold": 1, "concurrency": 1})
            for node in nodes:
                stop(node)
                node["server"]["grants"][0]["max_runtime_ms"] = 30000
                start(node)

            baseline = child_jobs()
            interrupted = controller_start("controller-crash")
            admitted = pause_after_admission(interrupted, baseline)
            interrupted[0].kill()
            interrupted[0].wait(timeout=3)
            assert interrupted[0].returncode == -signal.SIGKILL
            retain("controller-crash.stdout", interrupted[2].read_bytes())
            retain("controller-crash.stderr", interrupted[3].read_bytes())
            resume_servers()
            orphan = fleet("orphan-status", "status", "controller-crash")
            assert orphan["data"]["aggregate"] == "unknown"
            original_ids = {target["id"]: target["request_id"] for target in orphan["data"]["targets"]}
            dispatched = next(index for index in range(2) if admitted[index] - baseline[index])
            profile = nodes[dispatched]["root"] / "client.json"
            original_profile = profile.read_bytes()
            private(profile, original_profile + b"\n")
            drift = fleet("profile-drift-resume", "resume", "controller-crash", expected=1)
            assert drift["data"]["aggregate"] == "unknown" and child_jobs() == admitted
            assert any(target["reason"] == "fleet_profile_changed" for target in drift["data"]["targets"])
            private(profile, original_profile)
            # The same client certificate is now mapped to a different principal.
            # Its profile bytes remain unchanged; server-enforced expected identity
            # must prevent recovery from crossing into that new namespace.
            node = nodes[dispatched]
            stop(node)
            node["server"]["principals"][0]["id"] = "remapped.operator"
            node["server"]["grants"][0]["principal"] = "remapped.operator"
            start(node)
            assert profile.read_bytes() == original_profile
            remapped = fleet("principal-remap-resume", "resume", "controller-crash", timeout=1000, expected=1)
            assert remapped["data"]["aggregate"] == "unknown" and child_jobs() == admitted
            remapped_target = next(target for target in remapped["data"]["targets"] if target["id"] == f"target.{dispatched}")
            assert remapped_target["receipt"]["data"]["remote_error"] == "runner_precondition_failed"
            new_namespace = node["root"] / "state" / ("principal-" + sha(b"remapped.operator")) / "jobs"
            assert not list(new_namespace.glob("job-*"))
            stop(node)
            node["server"]["principals"][0]["id"] = "fleet.operator"
            node["server"]["grants"][0]["principal"] = "fleet.operator"
            start(node)
            resumed = fleet("crash-resume", "resume", "controller-crash", expected=1)
            assert resumed["data"]["counts"]["skipped"] == 1 and resumed["data"]["counts"]["unknown"] == 0
            assert {target["id"]: target["request_id"] for target in resumed["data"]["targets"]} == original_ids
            assert child_jobs() == admitted
            lifecycles.append({"case": "controller-sigkill-after-admission", "profile_drift_refused": True, "principal_remap_refused": True, "original_request_ids_preserved": True, "remote_job_sets_unchanged": True})

            baseline = child_jobs()
            cancelled_controller = controller_start("controller-cancel")
            pause_after_admission(cancelled_controller, baseline)
            intent = fleet("cancel-intent", "cancel", "controller-cancel")
            assert intent["data"]["run"]["cancel_requested"] is True
            resume_servers()
            cancelled = controller_finish("cancel-observed", cancelled_controller)
            assert len(cancelled["data"]["targets"]) == 2 and cancelled["data"]["counts"]["skipped"] == 1
            assert cancelled["data"]["counts"]["unknown"] == 0
            lifecycles.append({"case": "active-cancellation", "all_targets_accounted_for": True})

            nodes[1]["process"].send_signal(signal.SIGSTOP)
            started = time.monotonic()
            bounded_controller = controller_start("controller-deadline", concurrency=2, timeout=300)
            bounded = controller_finish("global-deadline", bounded_controller, expected=None)
            resume_servers()
            assert time.monotonic() - started < 4
            assert len(bounded["data"]["targets"]) == 2 and bounded["data"]["aggregate"] != "succeeded"
            lifecycles.append({"case": "global-deadline", "deadline_ms": 300, "all_targets_accounted_for": True})

            # Unknown on-disk additions must be preserved, never repaired into a
            # successful historical result during status lookup.
            interrupted_bytes = store / "interrupted-unknown-file"
            private(interrupted_bytes, b"partial persistence fixture")
            before = {str(path.relative_to(store)): sha(path.read_bytes()) for path in store.rglob("*") if path.is_file()}
            refused = fleet("partial-store-refused", "status", "frozen-once", expected=1)
            assert refused["execution"]["status"] != "succeeded"
            after = {str(path.relative_to(store)): sha(path.read_bytes()) for path in store.rglob("*") if path.is_file()}
            assert before == after
            retain("partial-store-files.json", before)
            lifecycles.append({"case": "unknown-partial-store", "fixture_injected": True, "bytes_preserved": True})

            if options.small_store is not None:
                small = options.small_store.resolve()
                assert small.parent == Path("/tmp/evidence-small-disk") and not small.exists()
                store = small
                failed = False
                for number in range(32):
                    result = fleet(f"disk-full-{number}", "collect", value=body(f"disk-full-{number}"), expected=None)
                    if result["execution"]["status"] != "succeeded":
                        assert result["effect_outcome"] == "unknown", result
                        failed = True
                        break
                assert failed, "private tiny filesystem did not exercise fleet persistence failure"
                before = {str(path.relative_to(store)): sha(path.read_bytes()) for path in store.rglob("*") if path.is_file()}
                refused = fleet("disk-full-preserved", "status", "disk-full-0", expected=1)
                assert refused["execution"]["status"] != "succeeded"
                after = {str(path.relative_to(store)): sha(path.read_bytes()) for path in store.rglob("*") if path.is_file()}
                assert before == after and before
                retain("disk-full-files.json", before)
                lifecycles.append({"case": "private-64kib-enospc", "uncertainty_disclosed": True, "bytes_preserved": True})
            for node in nodes:
                stop(node)
        finally:
            # SIGCONT precedes cleanup even when an assertion failed while a
            # test-owned server was deliberately stopped at the crash boundary.
            for process in active:
                if process.poll() is None:
                    process.send_signal(signal.SIGCONT)
                    process.terminate()
                    try:
                        process.wait(timeout=4)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait(timeout=3)

    helpers.source_guard(ROOT, options.source_manifest, options.expected_source_manifest_sha256)
    assert sha(original.read_bytes()) == options.expected_binary_sha256
    retain("lifecycles.json", lifecycles)
    retain("verification.json", {"passed": True, "commands": commands, "lifecycles": lifecycles,
                                 "binary_sha256": options.expected_binary_sha256, "source_manifest_sha256": options.expected_source_manifest_sha256})
    print(f"PASS: {len(commands)} installed commands; frozen fleet identity, explicit child outcomes, bounded scheduling and recovery")


if __name__ == "__main__":
    main()
