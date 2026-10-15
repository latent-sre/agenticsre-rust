"""Installed extension protocol mission; execute only through tools/test-sandbox.sh."""
from __future__ import annotations

import argparse
import base64
import copy
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
OP = "ext.example.capacity.project"


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
    options = parser.parse_args()
    assert os.environ.get("CARGO_HOME") == "/tmp/cargo" and Path("/toolchain").is_dir(), "Use tools/test-sandbox.sh"
    spec = importlib.util.spec_from_file_location("runtime_helpers", ROOT / "tools/verify-evidence.py")
    helpers = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helpers)
    source = helpers.source_guard(ROOT, options.source_manifest, options.expected_source_manifest_sha256)
    assert source["tools/verify-extension-runtime.py"] == sha(Path(__file__).read_bytes())
    binary, peer, adversary = (p.resolve(strict=True) for p in (options.binary, options.peer, options.adversary))
    originals = {binary:options.expected_binary_sha256, peer:options.expected_peer_sha256, adversary:options.expected_adversary_sha256}
    for p, expected in originals.items():
        assert sha(p.read_bytes()) == expected
    evidence = options.evidence_dir.resolve()
    evidence.mkdir(mode=0o700, parents=True, exist_ok=False)
    schemas = {p.stem.removesuffix(".schema"): helpers.strict_json(p.read_bytes()) for p in (ROOT / "docs/sre-workbench/schemas").glob("*.schema.json")}
    registry = Registry().with_resources((s["$id"], Resource.from_contents(s)) for s in schemas.values())
    validators = {n:Draft202012Validator(schemas[n], registry=registry, format_checker=FormatChecker())
                  for n in ("result", "extension-data", "extension-run-data", "capability", "extension-package", "extension-policy")}
    commands, package_hashes, peer_checks = [], {}, []

    def retain(name, value):
        raw = value if isinstance(value, bytes) else (json.dumps(value, sort_keys=True, indent=2) + "\n").encode()
        p = evidence / name
        p.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        p.write_bytes(raw)

    def validate(result):
        validators["result"].validate(result)
        assert result["assessment"] == "not_assessed"
        data = result["data"]
        if "packages" in data:
            validators["extension-data"].validate(data)
        if data.get("format_version") == 1 and "protocol" in data:
            validators["extension-run-data"].validate(data)
            terminal = data["terminal"]
            detail = {"progress":data["progress"], "terminal_data":terminal["data"] if terminal else None,
                "terminal_error_message":terminal["error"]["message"] if terminal and terminal["error"] else None,
                "stderr":data["diagnostics"]["stderr"]}
            assert len(encoded(detail)) == data["detail_bytes"], (len(encoded(detail)), data)
            assert data["detail_bytes"] <= data["detail_budget_bytes"] <= result["effective_limits"]["max_output_bytes"]
            if result["execution"]["child_exit_code"] is not None or data["supervisor"] is not None:
                assert result["effect_outcome"] == "unknown"
        if "capabilities" in data:
            for row in data["capabilities"]:
                validators["capability"].validate(row["capability"])
        if data.get("capability") and "availability" in data:
            validators["capability"].validate(data["capability"])

    with tempfile.TemporaryDirectory(prefix="installed-extension-runtime-") as directory:
        scratch = Path(directory)
        installed = scratch / "prefix/bin/save"
        installed.parent.mkdir(parents=True)
        shutil.copy2(binary, installed)
        installed.chmod(0o755)
        cwd = scratch / "unrelated"
        cwd.mkdir()
        store, policy = scratch / "store", scratch / "policy.json"
        typed, request_path = scratch / "inputs.json", scratch / "request.json"
        environment = {"PATH":"/usr/bin:/bin", "LANG":"C.UTF-8", "TZ":"UTC"}
        hello = {"protocol":1,"request_id":"compatibility","run_id":"run-compatibility","sequence":0,"kind":"hello"}
        future_request = {**hello,"sequence":1,"kind":"request","operation":OP,"operation_version":2,
            "inputs":{"capacity_bytes":1500,"used_bytes":1000,"growth_bytes_per_day":100},
            "limits":{"timeout_ms":30000,"max_output_bytes":1048576}}
        for name, frames in (
            ("future-protocol", [{**hello,"protocol":2}]),
            ("array-hello", [[1,"compatibility","run-compatibility",0,"hello"]]),
            ("future-operation", [hello,future_request]),
        ):
            transcript=b"".join(encoded(frame)+b"\n" for frame in frames)
            observed=subprocess.run([str(peer)],input=transcript,cwd=cwd,env=environment,
                capture_output=True,timeout=5,check=False)
            retain(f"peer-compatibility/{name}.stdin",transcript)
            retain(f"peer-compatibility/{name}.stdout",observed.stdout)
            retain(f"peer-compatibility/{name}.stderr",observed.stderr)
            if name=="future-operation":
                terminal=helpers.strict_json(observed.stdout.splitlines()[-1])
                assert observed.returncode==0 and terminal["status"]=="failed" and terminal["data"] is None
                assert terminal["error"]["code"]=="unsupported_operation"
            else:
                assert observed.returncode!=0 and not observed.stdout
            peer_checks.append({"case":name,"exit":observed.returncode,"refused":True})

        def argv(args, authorized=True, text=False):
            result = [str(installed), *([] if text else ["--json"]), "--extension-state-dir", str(store)]
            if authorized:
                result += ["--extension-policy", str(policy)]
            return [*result, *args]

        def finish(name, output, expected=0, text=False):
            retain(f"commands/{name}.stdout" + (".txt" if text else ".json"), output.stdout)
            retain(f"commands/{name}.stderr", output.stderr)
            commands.append({"case":name,"exit":output.returncode})
            assert output.returncode == expected and not output.stderr, (name, output.returncode, output.stdout[:4000], output.stderr[:300])
            if text:
                return output.stdout.decode()
            assert output.stdout.count(b"\n") == 1 and output.stdout.endswith(b"\n") and len(output.stdout) <= 2*1024*1024
            result = helpers.strict_json(output.stdout)
            validate(result)
            return result

        def run(name, args, expected=0, authorized=True, text=False):
            output = subprocess.run(argv(args, authorized, text), cwd=cwd, env=environment,
                capture_output=True, timeout=15, check=False)
            return finish(name, output, expected, text)

        def package(path, version):
            manifest = json.loads((ROOT / "examples/extensions/capacity/manifest.json").read_text())
            raw = path.read_bytes()
            manifest["version"] = version
            manifest["executable"].update(sha256="sha256:"+sha(raw), bytes=len(raw))
            value = {"package_format":1,"manifest":manifest,"executable_base64":base64.b64encode(raw).decode()}
            validators["extension-package"].validate(value)
            content = encoded(value)
            target = scratch / f"package-{version}.json"
            target.write_bytes(content)
            pin = "sha256:" + sha(content)
            package_hashes[str(version)] = pin
            return target, pin

        def install(name, path, pin):
            result = run(name,["extension","install",str(path),"--expected-package-sha256",pin], authorized=False)
            assert result["data"]["enabled"] is False
            assert result["data"]["execution_available"] is False
            return result

        def grant(pins):
            value = {"policy_version":1,"execution_profile":"operator-reviewed-v1",
                "packages":[{"package_sha256":pin,"capabilities":[OP]} for pin in pins]}
            validators["extension-policy"].validate(value)
            policy.write_bytes(encoded(value))
            policy.chmod(0o600)
            return "sha256:"+sha(policy.read_bytes())

        default_input = {"capacity_bytes":1500,"used_bytes":1000,"growth_bytes_per_day":100}
        def make_request(request_id="installed-runtime", inputs=None, timeout=30000, budget=1048576, version=1):
            request = {"spec_version":"0.1","request_id":request_id,"operation":OP,"operation_version":version,
                "target":{"kind":"local","id":"workstation"},"inputs":default_input if inputs is None else inputs,
                "limits":{"timeout_ms":timeout,"max_output_bytes":budget},"record":"never"}
            request_path.write_bytes(encoded(request))
            return ["call","--request",str(request_path)]

        peer_package, peer_pin = package(peer,1)
        install("install-peer", peer_package, peer_pin)
        disabled = run("disabled-dispatch", make_request(), 2, authorized=False)
        assert disabled["execution"]["child_exit_code"] is None
        description = run("disabled-description", ["capabilities","describe",OP], authorized=False)
        assert description["data"]["availability"]["status"] != "available"
        policy_digest = grant([peer_pin])
        enabled = run("authorized-inventory", ["extension","list"])
        assert enabled["data"]["enabled"] and enabled["data"]["execution_available"]
        assert enabled["data"]["policy_sha256"] == policy_digest
        discovery = run("authorized-discovery", ["capabilities","list"])
        external = [r for r in discovery["data"]["capabilities"] if r["capability"]["id"] == OP]
        assert len(external)==1 and external[0]["availability"]["status"] == "available"
        assert external[0]["capability"]["effects"] == "unclassified"
        description = run("authorized-description", ["capabilities","describe",OP])
        assert description["data"]["capability"] == external[0]["capability"]
        typed.write_bytes(encoded(default_input))
        actual = run("capacity-cli", ["extension","run",OP,"--input",str(typed)])
        Draft202012Validator(description["data"]["capability"]["output_schema"],
            registry=registry, format_checker=FormatChecker()).validate(actual)
        terminal = actual["data"]["terminal"]
        assert terminal["status"] == "succeeded" and terminal["data"]["headroom_bytes"] == 500
        assert terminal["data"]["days_remaining_floor"] == 5
        assert actual["data"]["package"]["package_sha256"] == peer_pin
        assert actual["data"]["authorization"]["policy_sha256"] == policy_digest
        assert actual["data"]["supervisor"]["cleanup"]["confirmed"]
        human = run("capacity-text", ["extension","run",OP,"--input",str(typed)], text=True)
        assert "headroom_bytes" in human and "500" in human and "untrusted" in human.lower()
        parity = run("structured-unicode-id", make_request("unicode-π\ncorrelation"))
        assert parity["data"]["terminal"]["data"] == terminal["data"]
        for name, inputs, expected in (
            ("no-growth", {"capacity_bytes":100,"used_bytes":50,"growth_bytes_per_day":0}, (50,0,False,False)),
            ("exhausted", {"capacity_bytes":100,"used_bytes":101,"growth_bytes_per_day":1}, (0,0,True,True)),
            ("subday", {"capacity_bytes":100,"used_bytes":95,"growth_bytes_per_day":10}, (5,0,False,True)),
            ("safe-maximum", {"capacity_bytes":9007199254740991,"used_bytes":0,"growth_bytes_per_day":1}, (9007199254740991,9007199254740991,False,True)),
        ):
            result = run(name, make_request(inputs=inputs))["data"]["terminal"]["data"]
            assert tuple(result[k] for k in ("headroom_bytes","days_remaining_floor","exhausted","has_finite_horizon")) == expected
        for name, args in (("old-operation-version",make_request(version=2)),):
            rejected=run(name,args,2);assert rejected["execution"]["child_exit_code"] is None
        for name, inputs in (("wire-grant",{**default_input,"grant":True}),("fractional-input",{**default_input,"used_bytes":1.5}),("unsafe-integer",{**default_input,"used_bytes":9007199254740992})):
            rejected=run(name,make_request(inputs=inputs),2);assert rejected["execution"]["child_exit_code"] is None
        policy.chmod(0o644)
        rejected=run("public-policy",make_request(),2);assert rejected["execution"]["child_exit_code"] is None
        grant(["sha256:"+"0"*64])
        rejected=run("wrong-policy-pin",make_request(),2);assert rejected["execution"]["child_exit_code"] is None
        grant([peer_pin])
        saved=store / f"package-{peer_pin[7:]}.json"
        original=saved.read_bytes();saved.write_bytes(original+b" ")
        rejected=run("replaced-package",make_request(),1);assert rejected["execution"]["child_exit_code"] is None
        saved.write_bytes(original)
        adversary_package, adversary_pin = package(adversary,2)
        install("install-adversary",adversary_package,adversary_pin)
        # A newer installed version does not implicitly replace the one authorized pin.
        original_run=run("one-grant-selects-original",make_request())
        assert original_run["data"]["package"]["package_sha256"] == peer_pin
        grant([peer_pin,adversary_pin])
        ambiguous=run("ambiguous-grants",make_request(),2)
        assert ambiguous["execution"]["child_exit_code"] is None
        assert any(e["code"]=="extension_ambiguous_operation" for e in ambiguous["errors"])
        grant([adversary_pin])
        for mode in ("unsupported","correlation","malformed","invalid-utf8","oversized","sequence",
                     "early-result","duplicate-terminal","trailing-progress","missing-terminal",
                     "progress-flood","invalid-output","unterminated","duplicate-key","array-frame","crash-beforehello"):
            result=run("peer-"+mode,make_request("fixture-"+mode),1)
            assert result["execution"]["status"] == "failed", (mode,result)
            assert result["effect_outcome"] == "unknown"
            assert result["data"]["supervisor"]["cleanup"]["confirmed"], (mode,result)
        nonzero=run("peer-nonzero",make_request("fixture-nonzero"),1)
        assert nonzero["execution"]["status"] == "failed" and nonzero["execution"]["child_exit_code"]==7
        failed=run("peer-failed",make_request("fixture-failed"),1)
        assert failed["data"]["terminal"]["status"] == "failed" and failed["execution"]["status"]=="failed"
        stderr=run("stderr-limit",make_request("fixture-stderr-flood"),1)
        assert stderr["execution"]["status"]=="partial" and stderr["data"]["diagnostics"]["truncated"]
        bounded=run("detail-budget",make_request("fixture-stderr-flood",budget=1024),1)
        assert bounded["data"]["detail_bytes"]<=1024 and bounded["data"]["detail_budget_bytes"]==1024
        assert bounded["data"]["diagnostics"]["truncated"]
        timeout=run("peer-timeout",make_request("fixture-timeout",timeout=150),1)
        assert timeout["execution"]["status"]=="timed_out"
        assert timeout["data"]["supervisor"]["cleanup"]["confirmed"]
        timeout_marker=Path("/tmp/extension-peer-fixture-timeout.pid")
        if timeout_marker.exists():
            timeout_marker.unlink()

        def wait_marker(child, marker):
            end=time.monotonic()+8
            while time.monotonic()<end:
                if marker.exists():
                    return int(marker.read_text().strip())
                assert child.poll() is None, "Host exited before fixture reached request"
                time.sleep(0.01)
            raise AssertionError("Peer readiness marker did not appear")

        def absent(pid):
            try:
                os.kill(pid,0)
            except ProcessLookupError:
                return True
            return False

        marker=Path("/tmp/extension-peer-fixture-stubborn.pid")
        assert not marker.exists()
        args=make_request("fixture-stubborn",timeout=15000)
        child=subprocess.Popen(argv(args),cwd=cwd,env=environment,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        try:
            pid=wait_marker(child,marker)
            child.send_signal(signal.SIGTERM)
            out,err=child.communicate(timeout=8)
            result=finish("stubborn-cancellation",subprocess.CompletedProcess(child.args,child.returncode,out,err),143)
            assert result["execution"]["status"]=="cancelled" and result["data"]["cancellation_signal"]==15
            assert result["data"]["supervisor"]["cleanup"]["kill_sent"] and result["data"]["supervisor"]["cleanup"]["confirmed"]
            assert absent(pid)
        finally:
            if child.poll() is None:
                child.kill();child.communicate(timeout=5)
        marker.unlink()
        # Losing the output consumer must also terminate a blocked dialogue and its peer.
        args=make_request("fixture-stubborn",timeout=15000)
        child=subprocess.Popen(argv(args),cwd=cwd,env=environment,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        try:
            pid=wait_marker(child,marker)
            child.stdout.close()
            exit_code=child.wait(timeout=8)
            err=child.stderr.read()
            assert not err and absent(pid)
            retain("closed-consumer.json",{"host_exit":exit_code,"peer_pid":pid,"peer_absent":True,"stderr_bytes":len(err)})
        finally:
            if child.poll() is None:
                child.kill();child.wait(timeout=5)
            child.stderr.close()
        marker.unlink()
        assert sha(installed.read_bytes())==options.expected_binary_sha256
        assert peer_package.read_bytes()==encoded(helpers.strict_json(peer_package.read_bytes()))
        assert sha(peer_package.read_bytes())==peer_pin[7:]
        assert sha(adversary_package.read_bytes())==adversary_pin[7:]
    helpers.source_guard(ROOT,options.source_manifest,options.expected_source_manifest_sha256)
    for p,expected in originals.items():
        assert sha(p.read_bytes())==expected
    retain("package-sha256.json",package_hashes)
    retain("verification.json",{"passed":True,"commands":commands,"peer_compatibility":peer_checks,"source_files":len(source),
        "source_manifest_sha256":options.expected_source_manifest_sha256,"binary_sha256":options.expected_binary_sha256,
        "peer_sha256":options.expected_peer_sha256,"adversary_sha256":options.expected_adversary_sha256,
        "installed_copy_outside_checkout":True,"no_credentials_or_network":True,"closed_consumer_cleanup":True,
        "package_inputs_unchanged":True,"reviewed_operator_execution_only":True})
    print(f"PASS: {len(commands)} installed commands, {len(peer_checks)} peer compatibility checks and closed-consumer cleanup; real capacity protocol, policy, negative peers, limits and cancellation")


if __name__ == "__main__":
    main()
