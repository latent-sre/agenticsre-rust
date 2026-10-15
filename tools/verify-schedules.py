"""Installed persistent observational schedule mission; execute only through tools/test-sandbox.sh."""
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
    parser.add_argument("--small-store", type=Path, help="Fresh schedule store under the sandbox's private 64KiB /tmp/evidence-small-disk")
    options = parser.parse_args()
    assert os.environ.get("CARGO_HOME") == "/tmp/cargo" and Path("/toolchain").is_dir(), "Use tools/test-sandbox.sh"
    spec = importlib.util.spec_from_file_location("fleet_helpers", ROOT / "tools/verify-evidence.py")
    helpers = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helpers)
    source = helpers.source_guard(ROOT, options.source_manifest, options.expected_source_manifest_sha256)
    assert source["tools/verify-schedules.py"] == sha(Path(__file__).read_bytes())
    original = options.binary.resolve(strict=True)
    assert sha(original.read_bytes()) == options.expected_binary_sha256
    evidence = options.evidence_dir.resolve()
    evidence.mkdir(mode=0o700, parents=True, exist_ok=False)
    schemas = {path.stem.removesuffix(".schema"): helpers.strict_json(path.read_bytes())
               for path in (ROOT / "docs/sre-workbench/schemas").glob("*.schema.json")}
    registry = Registry().with_resources((schema["$id"], Resource.from_contents(schema)) for schema in schemas.values())
    validators = {name: Draft202012Validator(schemas[name], registry=registry, format_checker=FormatChecker())
                  for name in ("result", "fleet-data", "fleet-inventory", "schedule-data", "schedule-create-input", "runner-data", "runner-client-config", "runner-server-config", "capability")}
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
        elif result["operation"].startswith("schedule.") and "schedules" in result["data"]:
            validators["schedule-data"].validate(result["data"])
        elif result["operation"].startswith("runner.") and "admission" in result["data"]:
            validators["runner-data"].validate(result["data"])
        return result

    with tempfile.TemporaryDirectory(prefix="installed-schedules-") as temporary:
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

        schedule_store = root / "schedule-store"

        def arguments(method, value=None, identifier=None, revision=None, request_id=None, timeout=10000, budget=1048576):
            args = ["--schedule-state-dir", str(schedule_store), "--fleet-inventory", str(inventory_path), "--fleet-state-dir", str(store)]
            if method not in ("status", "list", "preview"):
                args += ["--record", "always"]
            args += ["schedule", method]
            if method in ("create", "update"):
                path = root / "definition.json"
                private(path, encoded(value))
                args += ["--input", str(path)]
            elif identifier is not None:
                args += [identifier]
            if revision is not None:
                args += ["--expected-revision", str(revision)]
            if request_id is not None:
                args += ["--request-id", request_id]
            if method == "work":
                args += ["--max-occurrences", "1"]
            if method == "preview":
                args += ["--count", "4"]
            return args + ["--timeout-ms", str(timeout), "--max-output-bytes", str(budget)]

        def schedule(name, method, expected=0, **kwargs):
            return local(name, arguments(method, **kwargs), expected)

        def definition(identifier, limit=32):
            return {"id": identifier, "definition": {
                "enabled": False, "timezone": "America/Chicago", "cadence": {"kind": "daily", "at": "02:30"},
                "collection": {"targets": ["target.0", "target.1"], "task": {"id": "error-budget", "version": 1, "input": {"slo": 99.9, "bad_minutes": 40.32}},
                               "concurrency": 1, "failure_threshold": 2, "timeout_ms": 5000},
                "misfire_grace_ms": 60000, "retention_limit": limit}}

        def summary(result, identifier):
            return next(row for row in result["data"]["schedules"] if row["id"] == identifier)

        def occurrence(result, request_id):
            return next(row for row in result["data"]["occurrences"] if row["manual_request_id"] == request_id)

        def child_jobs():
            result = []
            for node in nodes:
                base = node["root"] / "state" / ("principal-" + sha(b"fleet.operator")) / "jobs"
                admitted = set()
                for path in base.glob("job-*"):
                    # A directory appears before durable admission is complete.
                    try:
                        if path.is_dir() and (path / "lease").is_file():
                            helpers.strict_json((path / "request.json").read_bytes())
                            helpers.strict_json((path / "state.json").read_bytes())
                            admitted.add(path.name)
                    except FileNotFoundError:
                        pass
                result.append(admitted)
            return result

        def work_start(name="worker"):
            args = arguments("work", timeout=10000)
            stdout, stderr = root / f"{name}.stdout", root / f"{name}.stderr"
            with stdout.open("wb") as out, stderr.open("wb") as err:
                process = subprocess.Popen([str(binary), "--json", *args], cwd=cwd, env=environment, stdout=out, stderr=err)
            active.append(process)
            return process, args, stdout, stderr

        def resume_servers():
            for node in nodes:
                if node["process"] is not None and node["process"].poll() is None:
                    node["process"].send_signal(signal.SIGCONT)

        try:
            for node in nodes:
                start(node)
            write_inventory()
            described = local("schedule-description", ["capabilities", "describe", "schedule.create"])
            validators["capability"].validate(described["data"]["capability"])
            invalid = definition("invalid.zone")
            invalid["definition"]["timezone"] = "Not/A_Timezone"
            denied = schedule("invalid-timezone", "create", value=invalid, expected=2)
            assert denied["effect_outcome"] == "not_attempted" and not schedule_store.exists()
            human_refusal = subprocess.run([str(binary), "schedule", "status", "missing.schedule"], cwd=cwd,
                                           env=environment, capture_output=True, timeout=10)
            retain("commands/text-refusal.stdout.txt", human_refusal.stdout)
            retain("commands/text-refusal.stderr", human_refusal.stderr)
            assert human_refusal.returncode == 2 and not human_refusal.stderr
            assert b"schedule_store_required" in human_refusal.stdout
            commands.append({"name": "text-refusal", "exit_code": human_refusal.returncode})
            selected = definition("daily.budget")
            validators["schedule-create-input"].validate(selected)
            created = schedule("create", "create", value=selected)
            row = summary(created, "daily.budget")
            assert row["revision"] == row["generation"] == 1 and row["enabled"] is False
            assert row["tzdb_version"] == "2025b"
            direct = {"spec_version": "0.1", "request_id": "generic-worker", "operation": "schedule.work",
                      "operation_version": 1, "target": {"kind": "local", "id": "workstation"}, "inputs": {"max_occurrences": 1}, "record": "always",
                      "limits": {"timeout_ms": 10000, "max_output_bytes": 1048576}}
            direct_path = root / "generic-worker.json"
            private(direct_path, encoded(direct))
            refused = local("generic-worker-refused", ["--schedule-state-dir", str(schedule_store),
                            "--fleet-inventory", str(inventory_path), "--fleet-state-dir", str(store),
                            "call", "--request", str(direct_path)], expected=2)
            assert any(error["code"] == "schedule_worker_entry_required" for error in refused["errors"])
            assert refused["effect_outcome"] == "not_attempted"
            observed = schedule("status", "status", identifier="daily.budget")
            assert summary(observed, "daily.budget") == row
            listed = schedule("list", "list")
            assert len(listed["data"]["schedules"]) == 1
            preview = schedule("preview", "preview", identifier="daily.budget")
            assert len(preview["data"]["preview"]) == 4
            assert all(item["due_utc"].endswith("Z") and item["local"] for item in preview["data"]["preview"])
            human_preview = subprocess.run([str(binary), *arguments("preview", identifier="daily.budget")], cwd=cwd,
                                           env=environment, capture_output=True, timeout=10)
            retain("commands/text-preview.stdout.txt", human_preview.stdout)
            retain("commands/text-preview.stderr", human_preview.stderr)
            assert human_preview.returncode == 0 and not human_preview.stderr and b"2025b" in human_preview.stdout
            for item in preview["data"]["preview"]:
                assert item["due_utc"].encode() in human_preview.stdout and item["local"].encode() in human_preview.stdout
            commands.append({"name": "text-preview", "exit_code": human_preview.returncode})
            baseline = child_jobs()
            queued = schedule("run-now", "run-now", identifier="daily.budget", revision=1, request_id="manual-one")
            original_occurrence = occurrence(queued, "manual-one")
            assert original_occurrence["state"] == "queued" and child_jobs() == baseline
            duplicate = schedule("duplicate-manual", "run-now", identifier="daily.budget", revision=1, request_id="manual-one")
            assert occurrence(duplicate, "manual-one")["id"] == original_occurrence["id"] and child_jobs() == baseline
            schedule("work", "work")
            completed = schedule("completed-status", "status", identifier="daily.budget")
            done = occurrence(completed, "manual-one")
            assert done["state"] == "terminal" and done["fleet_aggregate"] == "succeeded"
            assert all(len(after - before) == 1 for before, after in zip(baseline, child_jobs(), strict=True))
            assert done["fleet_run_id"] and done["receipt_sha256"]
            bounded = schedule("bounded-status", "status", identifier="daily.budget", budget=1024, expected=None)
            bounded_row = occurrence(bounded, "manual-one")
            assert bounded_row["receipt"] is None and bounded_row["receipt_omitted"]
            for key in ("id", "fleet_request_id", "fleet_run_id", "fleet_aggregate", "receipt_sha256"):
                assert bounded_row[key] == done[key]
            human_args = arguments("status", identifier="daily.budget")
            human = subprocess.run([str(binary), *human_args], cwd=cwd, env=environment, capture_output=True, timeout=10)
            retain("commands/text-status.stdout.txt", human.stdout)
            retain("commands/text-status.stderr", human.stderr)
            assert human.returncode == 0 and not human.stderr and b"daily.budget" in human.stdout
            commands.append({"name": "text-status", "exit_code": human.returncode})
            changed = copy.deepcopy(selected)
            changed["expected_revision"] = summary(completed, "daily.budget")["revision"]
            changed["definition"]["cadence"]["at"] = "03:30"
            updated = schedule("update", "update", value=changed)
            assert summary(updated, "daily.budget")["generation"] == 2
            assert occurrence(updated, "manual-one")["id"] == original_occurrence["id"]
            old_duplicate = schedule("duplicate-after-update", "run-now", identifier="daily.budget", revision=1, request_id="manual-one")
            assert occurrence(old_duplicate, "manual-one")["id"] == original_occurrence["id"]
            revision = summary(updated, "daily.budget")["revision"]
            schedule("stale-update", "update", value=changed, expected=2)
            paused = schedule("pause", "pause", identifier="daily.budget", revision=revision)
            revision = summary(paused, "daily.budget")["revision"]
            assert not summary(paused, "daily.budget")["enabled"]
            schedule("cancel-idle", "cancel-active", identifier="daily.budget", revision=revision)
            lifecycles.append({"case": "revision-generation-manual-dedup", "history_preserved": True, "paused_manual_explicit": True})

            # Editing a generation cannot erase the retained occurrence ledger or
            # make an old manual request acquire fresh dispatch authority.
            quota = definition("quota.budget", limit=1)
            schedule("quota-create", "create", value=quota)
            quota_queued = schedule("quota-queue", "run-now", identifier="quota.budget", revision=1, request_id="quota-once")
            quota_id = occurrence(quota_queued, "quota-once")["id"]
            quota["expected_revision"] = summary(quota_queued, "quota.budget")["revision"]
            quota["definition"]["cadence"]["at"] = "04:30"
            quota_updated = schedule("quota-new-generation", "update", value=quota)
            retired = occurrence(quota_updated, "quota-once")
            assert retired["state"] == "skipped" and retired["reason"] == "generation_changed" and retired["id"] == quota_id
            quota_baseline = child_jobs()
            refusal = schedule("quota-retained-across-generations", "run-now", identifier="quota.budget",
                               revision=summary(quota_updated, "quota.budget")["revision"], request_id="quota-twice", expected=2)
            assert any(error["code"] == "schedule_quota_exceeded" for error in refusal["errors"])
            assert refusal["effect_outcome"] == "not_attempted" and child_jobs() == quota_baseline
            quota_duplicate = schedule("quota-original-dedup", "run-now", identifier="quota.budget", revision=1, request_id="quota-once")
            assert occurrence(quota_duplicate, "quota-once") == retired
            assert summary(quota_duplicate, "quota.budget")["total_occurrences"] == 1
            lifecycles.append({"case": "retention-across-generations", "retired_identity_preserved": True, "new_admission_refused": True})

            # A real scheduler dies only after its original fleet has admitted a child.
            snapshot = schedule("before-crash", "status", identifier="daily.budget")
            revision = summary(snapshot, "daily.budget")["revision"]
            schedule("queue-crash", "run-now", identifier="daily.budget", revision=revision, request_id="manual-crash")
            baseline = child_jobs()
            worker = work_start()
            deadline = time.monotonic() + 8
            while child_jobs() == baseline:
                assert worker[0].poll() is None and time.monotonic() < deadline, "No live scheduler child admission observed"
                time.sleep(0.001)
            for node in nodes:
                node["process"].send_signal(signal.SIGSTOP)
            admitted = child_jobs()
            assert worker[0].poll() is None
            busy = schedule("concurrent-worker", "work", expected=None)
            assert busy["execution"]["status"] != "succeeded"
            worker[0].kill()
            worker[0].wait(timeout=3)
            assert worker[0].returncode == -signal.SIGKILL
            retain("scheduler-crash.stdout", worker[2].read_bytes())
            retain("scheduler-crash.stderr", worker[3].read_bytes())
            resume_servers()
            crash_status = schedule("crash-status", "status", identifier="daily.budget")
            crashed = occurrence(crash_status, "manual-crash")
            assert crashed["state"] in ("intent", "unknown")
            original_fleet_request = crashed["fleet_request_id"]
            # A newer generation must not overlap an unresolved old occurrence.
            overlapping = copy.deepcopy(selected)
            overlapping["expected_revision"] = summary(crash_status, "daily.budget")["revision"]
            overlapping["definition"]["cadence"]["at"] = "05:30"
            overlap_update = schedule("update-while-unresolved", "update", value=overlapping)
            assert occurrence(overlap_update, "manual-crash")["id"] == crashed["id"]
            overlap_queue = schedule("queue-while-unresolved", "run-now", identifier="daily.budget",
                                     revision=summary(overlap_update, "daily.budget")["revision"], request_id="manual-after-crash")
            assert occurrence(overlap_queue, "manual-after-crash")["state"] == "queued"
            original_store = store
            store = root / "wrong-fleet-store"
            refused = schedule("wrong-store-recovery", "work", expected=None)
            assert refused["execution"]["status"] != "succeeded" and not store.exists()
            store = original_store
            schedule("crash-recovery", "work", expected=None)
            recovered = schedule("recovered-status", "status", identifier="daily.budget")
            restored = occurrence(recovered, "manual-crash")
            assert restored["fleet_request_id"] == original_fleet_request and child_jobs() == admitted
            assert restored["state"] in ("terminal", "unknown")
            assert occurrence(recovered, "manual-after-crash")["state"] == "queued"
            lifecycles.append({"case": "scheduler-sigkill-after-fleet-admission", "original_fleet_request_preserved": True, "remote_jobs_unchanged": True, "wrong_store_refused": True})

            original_stores = store, schedule_store
            try:
                # Hold only owned runners after durable admission; the scheduler
                # remains alive so cancel-active must reach its actual fleet.
                store, schedule_store = root / "cancel-fleet-store", root / "cancel-schedule-store"
                cancellation = schedule("cancel-create", "create", value=definition("cancel.budget"))
                queued_cancel = schedule("cancel-queue", "run-now", identifier="cancel.budget",
                                         revision=summary(cancellation, "cancel.budget")["revision"], request_id="cancel-live")
                baseline = child_jobs()
                cancel_worker = work_start("cancel-worker")
                deadline = time.monotonic() + 8
                while child_jobs() == baseline:
                    assert cancel_worker[0].poll() is None and time.monotonic() < deadline, "No live cancellation admission observed"
                    time.sleep(0.001)
                for node in nodes:
                    node["process"].send_signal(signal.SIGSTOP)
                assert cancel_worker[0].poll() is None
                admitted_cancel = child_jobs()
                assert sum(len(after - before) for before, after in zip(baseline, admitted_cancel, strict=True)) == 1, "Cancellation fixture did not intercept the first admission"
                cancelled = schedule("cancel-active-worker", "cancel-active", identifier="cancel.budget",
                                     revision=summary(queued_cancel, "cancel.budget")["revision"])
                assert occurrence(cancelled, "cancel-live")["cancel_requested"] is True
                # Management remains usable while the worker is blocked on TLS.
                active_status = schedule("cancel-active-status", "status", identifier="cancel.budget")
                assert occurrence(active_status, "cancel-live")["fleet_request_id"] == occurrence(queued_cancel, "cancel-live")["fleet_request_id"]
                resume_servers()
                cancel_worker[0].wait(timeout=14)
                finish("cancel-worker", subprocess.CompletedProcess(cancel_worker[1], cancel_worker[0].returncode,
                       cancel_worker[2].read_bytes(), cancel_worker[3].read_bytes()), expected=None, args=cancel_worker[1])
                fleet_request = occurrence(queued_cancel, "cancel-live")["fleet_request_id"]
                fleet_cancel = local("cancel-fleet-status", ["--fleet-inventory", str(inventory_path), "--fleet-state-dir", str(store),
                                     "fleet", "status", fleet_request, "--timeout-ms", "10000"])
                assert fleet_cancel["data"]["run"]["cancel_requested"] is True
                assert fleet_cancel["data"]["run"]["stop_reason"] == "cancelled"
                assert any(target["phase"] == "skipped" and target["reason"] == "fleet_cancelled" for target in fleet_cancel["data"]["targets"])
                assert child_jobs() == admitted_cancel
                lifecycles.append({"case": "live-schedule-cancellation", "worker_observed_fleet_cancel": True, "undispatched_target_skipped": True})

                # A graceful scheduler stop must not become implicit child cancellation.
                store, schedule_store = root / "shutdown-fleet-store", root / "shutdown-schedule-store"
                stopped_schedule = schedule("shutdown-create", "create", value=definition("shutdown.budget"))
                shutdown_queue = schedule("shutdown-queue", "run-now", identifier="shutdown.budget",
                                          revision=summary(stopped_schedule, "shutdown.budget")["revision"], request_id="shutdown-once")
                baseline = child_jobs()
                shutdown_worker = work_start("shutdown-worker")
                deadline = time.monotonic() + 8
                while child_jobs() == baseline:
                    assert shutdown_worker[0].poll() is None and time.monotonic() < deadline, "No admission before scheduler shutdown"
                    time.sleep(0.001)
                for node in nodes:
                    node["process"].send_signal(signal.SIGSTOP)
                admitted_shutdown = child_jobs()
                assert shutdown_worker[0].poll() is None
                shutdown_worker[0].send_signal(signal.SIGTERM)
                shutdown_worker[0].wait(timeout=4)
                stopped = finish("shutdown-worker", subprocess.CompletedProcess(shutdown_worker[1], shutdown_worker[0].returncode,
                                 shutdown_worker[2].read_bytes(), shutdown_worker[3].read_bytes()), expected=143, args=shutdown_worker[1])
                assert stopped["data"]["cancellation_signal"] == signal.SIGTERM
                original_request = occurrence(shutdown_queue, "shutdown-once")["fleet_request_id"]
                fleet_after_stop = local("shutdown-fleet-status", ["--fleet-state-dir", str(store), "fleet", "status", original_request])
                assert fleet_after_stop["data"]["run"]["cancel_requested"] is False
                assert child_jobs() == admitted_shutdown
                resume_servers()
                schedule("shutdown-recover", "work", expected=None)
                recovered_stop = schedule("shutdown-recovered-status", "status", identifier="shutdown.budget")
                assert occurrence(recovered_stop, "shutdown-once")["fleet_request_id"] == original_request
                assert child_jobs() == admitted_shutdown
                lifecycles.append({"case": "scheduler-sigterm", "remote_cancel_not_requested": True,
                                   "original_fleet_request_preserved": True, "remote_jobs_unchanged": True})

                # A fresh refusal that proves no admission is a resolved failure.
                # Restore credentials without replaying that original manual ID;
                # a separately requested new occurrence may then make progress.
                for drift in ("profile", "credential"):
                    store, schedule_store = root / f"{drift}-fleet-store", root / f"{drift}-schedule-store"
                    identifier = f"{drift}.budget"
                    schedule(f"{drift}-create", "create", value=definition(identifier))
                    drift_queued = schedule(f"{drift}-queue", "run-now", identifier=identifier, revision=1, request_id=f"{drift}-once")
                    baseline = child_jobs()
                    changed_path = nodes[0]["root"] / "client.json" if drift == "profile" else pki / "client.key.der"
                    original_bytes = changed_path.read_bytes()
                    try:
                        if drift == "profile":
                            profile = helpers.strict_json(original_bytes)
                            profile["server_certificate_sha256"] = "sha256:" + "0" * 64
                            private(changed_path, encoded(profile))
                        else:
                            private(changed_path, b"invalid fixture PKCS8")
                        refused = schedule(f"{drift}-work-refused", "work", expected=None)
                        failed = occurrence(refused, f"{drift}-once")
                        assert failed["state"] == "failed" and failed["fleet_run_id"] is None
                        assert failed["fleet_request_id"] == occurrence(drift_queued, f"{drift}-once")["fleet_request_id"]
                        assert failed["receipt"]["effect_outcome"] == "not_attempted"
                        assert any(error["code"] == "fleet_binding_changed" for error in failed["receipt"]["errors"])
                        assert child_jobs() == baseline and not list(store.glob("fleet-*"))
                    finally:
                        private(changed_path, original_bytes)
                    restored = schedule(f"{drift}-restore-no-replay", "work", expected=None)
                    assert occurrence(restored, f"{drift}-once")["state"] == "failed"
                    assert child_jobs() == baseline and not list(store.glob("fleet-*"))
                    duplicate = schedule(f"{drift}-original-manual-dedup", "run-now", identifier=identifier,
                                         revision=1, request_id=f"{drift}-once")
                    assert occurrence(duplicate, f"{drift}-once")["id"] == failed["id"]
                    fresh = schedule(f"{drift}-explicit-new-manual", "run-now", identifier=identifier,
                                     revision=summary(duplicate, identifier)["revision"], request_id=f"{drift}-new")
                    assert occurrence(fresh, f"{drift}-new")["fleet_request_id"] != failed["fleet_request_id"]
                    schedule(f"{drift}-explicit-new-work", "work", expected=None)
                    progressed = schedule(f"{drift}-progress-status", "status", identifier=identifier)
                    assert occurrence(progressed, f"{drift}-new")["state"] == "terminal"
                    assert occurrence(progressed, f"{drift}-new")["fleet_aggregate"] == "succeeded"
                    assert occurrence(progressed, f"{drift}-once")["id"] == failed["id"]
                    assert all(len(after - before) == 1 for before, after in zip(baseline, child_jobs(), strict=True))
                    lifecycles.append({"case": f"{drift}-drift-before-admission", "refusal_admitted_no_jobs": True,
                                       "restoration_does_not_replay": True, "explicit_new_occurrence_progressed": True})
            finally:
                resume_servers()
                store, schedule_store = original_stores

            # Partial/unknown schedule data must be preserved byte-for-byte on refusal.
            private(schedule_store / "unknown-partial-file", b"interrupted synthetic schedule write")
            before = {str(p.relative_to(schedule_store)): sha(p.read_bytes()) for p in schedule_store.rglob("*") if p.is_file()}
            refused = schedule("partial-store-refused", "status", identifier="daily.budget", expected=None)
            assert refused["execution"]["status"] != "succeeded"
            after = {str(p.relative_to(schedule_store)): sha(p.read_bytes()) for p in schedule_store.rglob("*") if p.is_file()}
            assert before == after
            retain("partial-store-files.json", before)
            lifecycles.append({"case": "unknown-partial-store", "bytes_preserved": True})
            if options.small_store is not None:
                schedule_store = options.small_store.resolve()
                assert schedule_store.parent == Path("/tmp/evidence-small-disk") and not schedule_store.exists()
                exhausted = False
                for number in range(16):
                    result = schedule(f"disk-full-{number}", "create", value=definition(f"tiny.{number}"), expected=None)
                    if result["execution"]["status"] != "succeeded":
                        assert result["effect_outcome"] == "unknown", result
                        exhausted = True
                        break
                assert exhausted, "Expected actual private filesystem exhaustion"
                before = {str(p.relative_to(schedule_store)): sha(p.read_bytes()) for p in schedule_store.rglob("*") if p.is_file()}
                refused = schedule("disk-full-preserved", "list", expected=None)
                assert refused["execution"]["status"] != "succeeded"
                assert before == {str(p.relative_to(schedule_store)): sha(p.read_bytes()) for p in schedule_store.rglob("*") if p.is_file()}
                retain("disk-full-files.json", before)
                lifecycles.append({"case": "private-64kib-enospc", "bytes_preserved": True})
            for node in nodes:
                stop(node)
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

    helpers.source_guard(ROOT, options.source_manifest, options.expected_source_manifest_sha256)
    assert sha(original.read_bytes()) == options.expected_binary_sha256
    retain("lifecycles.json", lifecycles)
    retain("verification.json", {"passed": True, "commands": commands, "lifecycles": lifecycles,
                                 "binary_sha256": options.expected_binary_sha256, "source_manifest_sha256": options.expected_source_manifest_sha256})
    print(f"PASS: {len(commands)} installed schedule commands; revision ownership, manual deduplication, real fleet execution and no-replay recovery")


if __name__ == "__main__":
    main()
