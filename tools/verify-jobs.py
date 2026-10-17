"""Installed durable-job lifecycle mission; run only through tools/test-sandbox.sh."""
from __future__ import annotations

import argparse
import base64
import hashlib
import importlib.util
import json
import os
from pathlib import Path
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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--peer", type=Path, required=True)
    parser.add_argument("--expected-binary-sha256", required=True)
    parser.add_argument("--expected-fixture-sha256", required=True)
    parser.add_argument("--expected-peer-sha256", required=True)
    parser.add_argument("--source-manifest", type=Path, required=True)
    parser.add_argument("--expected-source-manifest-sha256", required=True)
    parser.add_argument("--evidence-dir", type=Path, required=True)
    parser.add_argument("--small-store", type=Path, help="Fresh store under the sandbox's private 64KiB /tmp/evidence-small-disk for ENOSPC acceptance")
    options = parser.parse_args()
    assert os.environ.get("CARGO_HOME") == "/tmp/cargo" and Path("/toolchain").is_dir(), "Use tools/test-sandbox.sh"
    spec = importlib.util.spec_from_file_location("job_helpers", ROOT / "tools/verify-evidence.py")
    helpers = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helpers)
    source = helpers.source_guard(ROOT, options.source_manifest, options.expected_source_manifest_sha256)
    assert source["tools/verify-jobs.py"] == sha(Path(__file__).read_bytes())
    original, original_fixture = options.binary.resolve(strict=True), options.fixture.resolve(strict=True)
    assert sha(original.read_bytes()) == options.expected_binary_sha256
    assert sha(original_fixture.read_bytes()) == options.expected_fixture_sha256
    peer = options.peer.resolve(strict=True)
    assert sha(peer.read_bytes()) == options.expected_peer_sha256
    evidence = options.evidence_dir.resolve()
    evidence.mkdir(mode=0o700, parents=True, exist_ok=False)
    schemas = {p.stem.removesuffix(".schema"): helpers.strict_json(p.read_bytes()) for p in (ROOT / "docs/sre-workbench/schemas").glob("*.schema.json")}
    registry = Registry().with_resources((s["$id"], Resource.from_contents(s)) for s in schemas.values())
    validators = {name: Draft202012Validator(schemas[name], registry=registry, format_checker=FormatChecker()) for name in ("result", "job-data", "capability")}
    commands, lifecycles = [], []
    active = []

    def retain(name, value):
        raw = value if isinstance(value, bytes) else (json.dumps(value, sort_keys=True, indent=2) + "\n").encode()
        path = evidence / name
        path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        path.write_bytes(raw)

    def validate(raw):
        result = helpers.strict_json(raw)
        validators["result"].validate(result)
        assert result["assessment"] == "not_assessed"
        if result["data"].get("format_version") == 1 and "job" in result["data"]:
            validators["job-data"].validate(result["data"])
        return result

    with tempfile.TemporaryDirectory(prefix="installed-jobs-") as temporary:
        root = Path(temporary)
        binary, fixture = root / "save", root / "runner-fixture"
        shutil.copyfile(original, binary)
        shutil.copyfile(original_fixture, fixture)
        binary.chmod(0o500)
        fixture.chmod(0o500)
        store = root / "jobs"
        base = [str(binary), "--json", "--job-state-dir", str(store)]

        def command(name, args, expected=0):
            completed = subprocess.run([*base, *args], cwd=root, capture_output=True, timeout=15)
            retain(f"commands/{name}.stdout.json", completed.stdout)
            retain(f"commands/{name}.stderr", completed.stderr)
            result = validate(completed.stdout)
            commands.append({"name":name, "args":args, "exit_code":completed.returncode})
            assert completed.returncode == expected, (name, completed.returncode, result)
            assert not completed.stderr, (name, completed.stderr)
            return result

        def request(name, args, timeout=30000):
            value = {"spec_version":"0.1", "request_id":name, "operation":"process.exec", "operation_version":1,
                     "target":{"kind":"local", "id":"workstation"}, "inputs":{"program":str(fixture), "args":args, "cwd":str(root)},
                     "limits":{"timeout_ms":timeout,"max_output_bytes":4096}, "record":"never"}
            path = root / f"{name}.json"
            path.write_text(json.dumps(value))
            return path

        def submit(name, path, runtime=30000, retention=32, expected=0, extra=()):
            return command(name, [*extra,"--record","always","job","submit","--request",str(path),"--max-runtime-ms",str(runtime),"--retain-events",str(retention)], expected)

        def job_id(result):
            return result["data"]["job"]["job_id"]

        def state(result):
            return result["data"]["job"]["state"]

        def wait_file(path, process):
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                if path.exists():
                    return
                assert process.poll() is None, "Worker exited before child readiness"
                time.sleep(0.02)
            raise AssertionError("Child readiness deadline exceeded")

        def start_worker(identifier):
            worker = subprocess.Popen([*base,"--record","always","job","work",identifier], cwd=root, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            active.append(worker)
            return worker

        def finish_worker(name, worker, expected=0):
            stdout, stderr = worker.communicate(timeout=12)
            retain(f"commands/{name}.stdout.json", stdout)
            retain(f"commands/{name}.stderr", stderr)
            result = validate(stdout)
            commands.append({"name":name,"exit_code":worker.returncode})
            assert worker.returncode == expected, (name, result)
            assert not stderr
            return result

        try:
            descriptions = {}
            for operation, effect in (("job.submit","local_write"),("job.work","unclassified")):
                described = command(operation.replace(".","-") + "-description",["capabilities","describe",operation])
                capability = described["data"]["capability"]
                validators["capability"].validate(capability)
                assert capability["effects"] == effect
                descriptions[operation] = capability
            marker = root / "single-dispatch"
            path = request("durable-once", ["mark",str(marker)])
            admitted = submit("submit", path, retention=2)
            identifier = job_id(admitted)
            assert state(admitted) == "queued" and not marker.exists()
            assert admitted["data"]["result"] is None
            assert admitted["record_mode"] == "always"
            Draft202012Validator(descriptions["job.submit"]["output_schema"],registry=registry).validate(admitted)
            duplicate = submit("duplicate-queued", path, retention=2)
            assert job_id(duplicate) == identifier and duplicate["data"]["action"] == "existing"
            changed = json.loads(path.read_text())
            changed["inputs"]["args"] = ["exit","0"]
            conflict = root / "conflict.json"
            conflict.write_text(json.dumps(changed))
            submit("conflicting-request", conflict, retention=2, expected=2)
            worked = command("work", ["--record","always","job","work",identifier])
            assert state(worked) == "terminal" and marker.read_bytes() == b"executed"
            assert worked["record_mode"] == "always"
            assert worked["data"]["result"]["execution"]["status"] == "succeeded"
            terminal = command("terminal-status", ["job","status",identifier])
            original_digest = terminal["data"]["result_sha256"]
            human = subprocess.run([argument for argument in base if argument != "--json"] + ["job","status",identifier],cwd=root,capture_output=True,timeout=10)
            retain("commands/terminal-text.stdout.txt",human.stdout)
            retain("commands/terminal-text.stderr",human.stderr)
            assert human.returncode == 0 and not human.stderr
            assert identifier.encode() in human.stdout and b'Stored child receipt' in human.stdout and b'"state":"terminal"' in human.stdout
            commands.append({"name":"terminal-text","exit_code":human.returncode})
            marker.write_bytes(b"do-not-replay")
            submit("duplicate-terminal", path, retention=2)
            command("work-terminal", ["--record","always","job","work",identifier])
            command("cancel-terminal", ["--record","always","job","cancel",identifier])
            again = command("terminal-immutable", ["job","status",identifier])
            assert marker.read_bytes() == b"do-not-replay" and again["data"]["result_sha256"] == original_digest
            watch = command("watch-gap", ["job","watch",identifier,"--after-sequence","0"])
            assert watch["data"]["cursor"]["gap"] is not None
            sequences = [event["sequence"] for event in watch["data"]["events"]]
            assert sequences == sorted(set(sequences))
            cursor = watch["data"]["cursor"]["next_sequence"]
            reconnect = command("watch-reconnect", ["job","watch",identifier,"--after-sequence",str(cursor)])
            assert not reconnect["data"]["events"]
            command("future-event-cursor",["job","watch",identifier,"--after-sequence","18446744073709551615"],2)
            control_request = json.loads(path.read_text())
            control_request.update(operation="job.work",inputs={"job_id":identifier},record="always")
            control_path = root / "generic-worker.json"
            control_path.write_text(json.dumps(control_request))
            generic = command("generic-worker-refused",["call","--request",str(control_path)],2)
            assert any(error["code"] == "job_worker_entry_required" for error in generic["errors"])
            nested_null = json.loads(path.read_text())
            nested_null["request_id"] = "invalid-nested-null"
            nested_null["target"]["environment"] = None
            control_request.update(operation="job.submit",inputs={"request":nested_null,"max_runtime_ms":30000,"retain_events":32})
            control_path.write_text(json.dumps(control_request))
            command("nested-null-control-refused",["call","--request",str(control_path)],2)

            long_path = request("long-runtime", ["exit","0"])
            long_job = submit("one-hour-admission", long_path, runtime=3600000)
            long_result = command("one-hour-work", ["--record","always","job","work",job_id(long_job)])
            assert long_result["data"]["result"]["effective_limits"]["timeout_ms"] > 300000
            submit("host-runtime-ceiling", request("ceiling",["exit","0"]), runtime=2000, expected=2, extra=("--job-max-runtime-ms","1000"))
            command("foreground-runtime-refused", ["call","--request",str(request("invalid-foreground",["exit","0"],timeout=3600000))], 2)
            no_consent = command("recording-required", ["job","submit","--request",str(path)], 2)
            assert no_consent["effect_outcome"] == "not_attempted"

            task_request = {"spec_version":"0.1","request_id":"fixed-task-job","operation":"task.run","operation_version":1,
                            "target":{"kind":"local","id":"workstation"},
                            "inputs":{"id":"error-budget","version":1,"input":{"slo":99,"bad_events":1,"total_events":100}},
                            "limits":{"timeout_ms":30000,"max_output_bytes":16384},"record":"never"}
            task_path = root / "task-request.json"
            task_path.write_text(json.dumps(task_request))
            task_job = submit("fixed-task-submit",task_path,runtime=600000)
            task_result = command("fixed-task-work",["--record","always","job","work",job_id(task_job)])
            assert task_result["data"]["result"]["execution"]["status"] == "succeeded"
            assert task_result["data"]["result"]["data"]["calculation"]["status"]["state"] == "exhausted"

            # Reconstruct the durable snapshot immediately after result publication,
            # before terminal-state publication. The receipt comes from a real child;
            # only the final state transition is rolled back in this crash fixture.
            final_marker = root / "published-result-marker"
            published = submit("published-result-submit",request("published-result",["mark",str(final_marker)]))
            published_id = job_id(published)
            committed = command("published-result-work",["--record","always","job","work",published_id])
            assert final_marker.read_bytes() == b"executed"
            published_digest = committed["data"]["result_sha256"]
            state_path = store / published_id / "state.json"
            crash_state = json.loads(state_path.read_bytes())
            assert crash_state["state"] == "terminal" and crash_state["events"][-1]["kind"] == "terminal"
            crash_state["events"].pop()
            assert crash_state["events"][-1]["kind"] == "finalizing"
            crash_state["last_sequence"] = crash_state["events"][-1]["sequence"]
            crash_state["state"] = "finalizing"
            crash_state["finished_at"] = None
            state_path.write_text(json.dumps(crash_state,separators=(",",":")))
            final_marker.write_bytes(b"published-receipt-no-replay")
            during = command("cancel-during-finalization",["--record","always","job","cancel",published_id])
            assert during["data"]["job"]["cancel_requested"] is True
            assert during["data"]["job"]["stored_state"] == "finalizing"
            adopted = command("adopt-published-result",["--record","always","job","work",published_id])
            assert state(adopted) == "terminal" and adopted["data"]["result_sha256"] == published_digest
            assert adopted["data"]["result"]["execution"]["status"] == "succeeded"
            assert adopted["data"]["job"]["cancel_requested"] is True
            assert final_marker.read_bytes() == b"published-receipt-no-replay"
            lifecycles.append({"case":"published-result-crash-snapshot","cancel_during_finalizing":True,"prior_result_adopted":True,"replayed":False})

            cancelled_marker = root / "cancelled-before-start"
            queued = submit("queued-cancel-submit",request("queued-cancel",["mark",str(cancelled_marker)]))
            queued_id = job_id(queued)
            command("queued-cancel",["--record","always","job","cancel",queued_id])
            command("queued-cancel-work",["--record","always","job","work",queued_id])
            queued_state = command("queued-cancel-status",["job","status",queued_id])
            assert state(queued_state) == "terminal" and not cancelled_marker.exists()

            ready, final = root / "cancel-ready", root / "cancel-final"
            running = submit("running-cancel-submit",request("running-cancel",["ready-delay-mark",str(ready),str(final),"5000"]))
            running_id = job_id(running)
            worker = start_worker(running_id)
            wait_file(ready, worker)
            live = command("running-status",["job","status",running_id])
            assert state(live) == "running"
            command("running-cancel",["--record","always","job","cancel",running_id])
            cancelled = finish_worker("cancelled-worker",worker)
            assert state(cancelled) == "terminal" and cancelled["data"]["result"]["execution"]["status"] == "cancelled"
            assert not final.exists()

            ready, final = root / "consumer-ready", root / "consumer-final"
            consumer = submit("consumer-submit",request("consumer",["ready-delay-mark",str(ready),str(final),"500"]))
            consumer_id = job_id(consumer)
            worker = start_worker(consumer_id)
            wait_file(ready, worker)
            worker.stdout.close()
            worker.wait(timeout=10)
            stderr = worker.stderr.read()
            assert worker.returncode == 1 and not stderr and final.exists()
            observed = command("consumer-terminal",["job","status",consumer_id])
            assert state(observed) == "terminal" and observed["data"]["result"]["execution"]["status"] == "succeeded"
            lifecycles.append({"case":"closed-worker-output","exit_code":worker.returncode,"terminal_persisted":True})

            ready, final = root / "crash-ready", root / "crash-final"
            crash = submit("crash-submit",request("crash",["ready-delay-mark",str(ready),str(final),"1000"]))
            crash_id = job_id(crash)
            worker = start_worker(crash_id)
            wait_file(ready, worker)
            worker.kill()
            stdout, stderr = worker.communicate(timeout=5)
            assert worker.returncode == -signal.SIGKILL
            retain("crashed-worker.stdout",stdout)
            retain("crashed-worker.stderr",stderr)
            observed = command("crash-observed",["job","status",crash_id])
            assert state(observed) == "recovery_required" and observed["data"]["result"] is None
            recovered = command("crash-reconcile",["--record","always","job","work",crash_id], 1)
            assert state(recovered) == "recovery_required"
            deadline = time.monotonic() + 4
            while not final.exists() and time.monotonic() < deadline:
                time.sleep(0.02)
            assert final.exists(), "Prior child must finish independently; no persisted-PID kill"
            final.write_bytes(b"no-replay-after-crash")
            command("crash-no-replay",["--record","always","job","work",crash_id],1)
            assert final.read_bytes() == b"no-replay-after-crash"
            lifecycles.append({"case":"worker-sigkill","observed_state":"recovery_required","prior_child_completed":True,"replayed":False})

            executable = peer.read_bytes()
            manifest = json.loads((ROOT / "examples/extensions/capacity/manifest.json").read_text())
            manifest["executable"]["sha256"] = "sha256:" + sha(executable)
            manifest["executable"]["bytes"] = len(executable)
            package = {"package_format":1,"manifest":manifest,"executable_base64":base64.b64encode(executable).decode("ascii")}
            package_bytes = json.dumps(package,sort_keys=True,separators=(",",":")).encode()
            package_pin = "sha256:" + sha(package_bytes)
            package_path = root / "capacity.save-package.json"
            package_path.write_bytes(package_bytes)
            extension_store, policy = root / "extensions", root / "policy.json"
            operation = "ext.example.capacity.project"
            base.extend(["--extension-state-dir",str(extension_store)])
            command("extension-install",["extension","install",str(package_path),"--expected-package-sha256",package_pin])
            policy_value = {"policy_version":1,"execution_profile":"operator-reviewed-v1","packages":[{"package_sha256":package_pin,"capabilities":[operation]}]}
            policy.write_text(json.dumps(policy_value))
            policy.chmod(0o600)
            base.extend(["--extension-policy",str(policy)])
            extension_request = {"spec_version":"0.1","request_id":"capacity-job","operation":operation,"operation_version":1,
                                 "target":{"kind":"local","id":"workstation"},"inputs":{"capacity_bytes":1000,"used_bytes":100,"growth_bytes_per_day":30},
                                 "limits":{"timeout_ms":30000,"max_output_bytes":4096},"record":"never"}
            extension_path = root / "extension-request.json"
            extension_path.write_text(json.dumps(extension_request))
            extension_job = submit("extension-submit",extension_path,runtime=3600000)
            extension_result = command("extension-work",["--record","always","job","work",job_id(extension_job)])
            child = extension_result["data"]["result"]
            assert child["execution"]["status"] == "succeeded"
            assert child["data"]["terminal"]["data"]["headroom_bytes"] == 900
            assert child["data"]["terminal"]["data"]["days_remaining_floor"] == 30
            assert child["effective_limits"]["timeout_ms"] > 300000
            extension_request["request_id"] = "capacity-changed-policy"
            extension_path.write_text(json.dumps(extension_request))
            pinned_job = submit("extension-policy-pinned",extension_path)
            policy_value["packages"] = []
            policy.write_text(json.dumps(policy_value))
            denied = command("extension-policy-changed",["--record","always","job","work",job_id(pinned_job)])
            assert denied["data"]["result"]["execution"]["status"] == "denied"
            assert denied["data"]["result"]["effect_outcome"] == "not_attempted"

            count = sum(entry.is_dir() for entry in store.iterdir())
            for index in range(count,64):
                submit(f"quota-admit-{index}",request(f"quota-{index}",["exit","0"]))
            full = submit("quota-refused",request("quota-overflow",["exit","0"]),expected=1)
            assert any(error["code"] == "job_quota_exceeded" for error in full["errors"])
            assert full["effect_outcome"] == "not_attempted"
            assert sum(entry.is_dir() for entry in store.iterdir()) == 64
            duplicate_full = submit("duplicate-at-capacity",path,retention=2)
            assert job_id(duplicate_full) == identifier and duplicate_full["data"]["action"] == "existing"

            if options.small_store is not None:
                small = options.small_store.resolve()
                assert small.parent == Path("/tmp/evidence-small-disk") and not small.exists()
                base[3] = str(small)
                oversized_admission = request("disk-full",["argv",*["x" * 15000] * 4])
                failed = submit("disk-full-submit",oversized_admission,expected=1)
                assert failed["execution"]["status"] in ("failed","unknown")
                assert failed["effect_outcome"] != "not_attempted"
                before = {str(path.relative_to(small)):sha(path.read_bytes()) for path in small.rglob("*") if path.is_file()}
                refused = command("disk-full-preserved",["job","status",identifier],1)
                assert any(error["code"] == "job_store_recovery_required" for error in refused["errors"])
                after = {str(path.relative_to(small)):sha(path.read_bytes()) for path in small.rglob("*") if path.is_file()}
                assert before == after and before
                retain("disk-full-files.json",before)
                lifecycles.append({"case":"private-64kib-enospc","partial_store_preserved":True,"files":len(before)})
                base[3] = str(store)

            for entry in [store,*store.rglob("*")]:
                assert entry.stat().st_mode & 0o077 == 0, entry
            retain("lifecycles.json",lifecycles)
        finally:
            for worker in active:
                if worker.poll() is None:
                    worker.terminate()
                    try:
                        worker.wait(timeout=4)
                    except subprocess.TimeoutExpired:
                        worker.kill()
                        worker.wait(timeout=3)

    helpers.source_guard(ROOT, options.source_manifest, options.expected_source_manifest_sha256)
    assert sha(original.read_bytes()) == options.expected_binary_sha256
    assert sha(original_fixture.read_bytes()) == options.expected_fixture_sha256
    assert sha(peer.read_bytes()) == options.expected_peer_sha256
    retain("verification.json",{"passed":True,"commands":commands,"lifecycles":lifecycles,"binary_sha256":options.expected_binary_sha256,"fixture_sha256":options.expected_fixture_sha256,"peer_sha256":options.expected_peer_sha256,"source_manifest_sha256":options.expected_source_manifest_sha256})
    print(f"PASS: {len(commands)} installed commands; duplicate admission, runtime boundaries, events, cancellation, output loss and crash recovery")


if __name__ == "__main__":
    main()
