"""Focused public witnesses for the diagnostics R0 findings; run only in the private sandbox."""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import ipaddress
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
specification = importlib.util.spec_from_file_location("diagnostics_fixture", ROOT / "tools/verify-diagnostics.py")
fixture = importlib.util.module_from_spec(specification)
specification.loader.exec_module(fixture)
HOST = "diagnostics.workbench.test"
ALIAS = "service-alias.workbench.test"
IP6_REVERSE = ipaddress.IPv6Address("::1").reverse_pointer
FINDINGS = ["synthetic-zone", "synthetic-cname", "udp-delay", "udp-loss", "tcp-loss", "tcp-close", "answer-owner", "chunk-budget"]
CASES = FINDINGS + [
    "synthetic-zone-tcp", "synthetic-ipv6-zone", "synthetic-ipv6-zone-tcp", "synthetic-cname-tcp",
    "synthetic-ipv6-cname", "synthetic-ipv6-cname-tcp", "reverse-server-answer", "reverse-server-answer-tcp",
    "tcp-delay", "cname-udp", "cname-tcp", "inline-cname-udp", "inline-cname-tcp",
    "cname-cycle-udp", "cname-cycle-tcp", "cname-conflict", "cname-address-conflict", "answer-type",
    "answer-owner-unrelated-denied", "cname-address-denied", "cname-eight-udp", "cname-eight-tcp",
    "cname-nine-udp", "cname-nine-tcp", "chunk-exact", "chunk-crossing", "head-exact", "head-crossing",
    "cname-parent-deadline-tcp", "cname-step-deadline-tcp",
]


def padded_head(initial: bytes, header_bytes: int) -> bytes:
    header = initial
    index = 0
    while len(header) + 2 < header_bytes:
        available = header_bytes - len(header) - 2 - len(f"X{index}: \r\n")
        count = available if available <= 916 else 900
        if count < 0:
            raise ValueError("Header target cannot be encoded")
        header += f"X{index}: ".encode() + b"x" * count + b"\r\n"
        index += 1
    header += b"\r\n"
    assert len(header) == header_bytes
    return header


def chunk_response(header_bytes: int) -> bytes:
    return padded_head(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n", header_bytes) + b"1\r\nx\r\n" * 255 + b"0\r\n\r\n"


def configure(case: str, target: dict, dns, https) -> dict:
    expected = {"dns_state":"succeeded", "all_steps":True, "queries":2}
    if case.endswith("-tcp") or case.startswith("tcp-"):
        target["resolver"]["transport"] = "tcp"
    if case.startswith("synthetic-"):
        zone = IP6_REVERSE if "ipv6" in case else "127.in-addr.arpa"
        if "cname" in case:
            for kind in (1, 28):
                dns.answers[HOST, kind] = [(HOST, 5, zone)]
            expected["queries"] = 4
        else:
            target["host"] = zone
        dns.drop_names = {zone}
        expected.update(dns_state="failed", reason="dns_lookup_failed", all_steps=False)
    elif case.startswith("reverse-server-answer"):
        target["host"] = "127.in-addr.arpa"
        target["allowed_ips"] = ["127.0.0.1", "127.0.0.2"]
        dns.addresses = ["127.0.0.2"]
        expected.update(addresses=["127.0.0.2"], all_steps=False)
    elif case in ("udp-delay", "tcp-delay"):
        dns.delay = 0.7
    elif case in ("udp-loss", "tcp-loss"):
        dns.drop_names = {HOST}
        expected.update(dns_state="failed", reason="dns_lookup_failed", all_steps=False)
    elif case == "tcp-close":
        for kind in (1, 28):
            dns.answers[HOST, kind] = [(HOST, 5, ALIAS)]
        dns.close_names = {ALIAS}
        expected.update(dns_state="failed", reason="dns_lookup_failed", all_steps=False, queries=None)
    elif case.startswith("inline-cname-"):
        for kind, address in [(1, "127.0.0.1"), (28, "::1")]:
            dns.answers[HOST, kind] = [("last.workbench.test", kind, address),
                (ALIAS.upper(), 5, "LAST.workbench.test"), ("unrelated.workbench.test", 1, "127.0.0.2"),
                (HOST.upper(), 5, ALIAS)]
        expected.update(chain=[HOST, ALIAS, "last.workbench.test"], addresses=["127.0.0.1", "::1"])
    elif case in ("cname-udp", "cname-tcp", "cname-address-denied") or case.startswith("cname-cycle-"):
        for kind in (1, 28):
            dns.answers[HOST, kind] = [(HOST, 5, ALIAS)]
            if case.startswith("cname-cycle-"):
                dns.answers[ALIAS, kind] = [(ALIAS, 5, HOST.upper())]
        expected.update(queries=4, chain=[HOST, ALIAS])
        if case.startswith("cname-cycle-"):
            expected.update(dns_state="failed", reason="dns_cname_cycle", all_steps=False)
        elif case == "cname-address-denied":
            dns.addresses = ["127.0.0.1", "127.0.0.2"]
            expected.update(dns_state="failed", reason="dns_address_denied", all_steps=False)
    elif case in ("cname-conflict", "cname-address-conflict"):
        for kind, address in [(1, "127.0.0.1"), (28, "::1")]:
            dns.answers[HOST, kind] = [(HOST, 5, ALIAS),
                (HOST, 5, "other.workbench.test") if case == "cname-conflict" else (HOST, kind, address)]
        expected.update(dns_state="failed", reason="dns_cname_conflict", all_steps=False)
    elif case == "answer-type":
        dns.answers[HOST, 1] = [(HOST, 28, "::1")]
        expected.update(dns_state="failed", reason="dns_answer_type_mismatch", all_steps=False)
    elif case in ("answer-owner", "answer-owner-unrelated-denied"):
        address, unrelated = ("127.0.0.2", "127.0.0.1") if case == "answer-owner" else ("127.0.0.1", "127.0.0.2")
        target["allowed_ips"] = ["127.0.0.1", "127.0.0.2"] if case == "answer-owner" else ["127.0.0.1"]
        dns.answers[HOST, 1] = [(HOST, 1, address), ("unrelated.workbench.test", 1, unrelated)]
        expected.update(addresses=[address], all_steps=case != "answer-owner")
    elif case.startswith("cname-eight-") or case.startswith("cname-nine-"):
        hops = 8 if case.startswith("cname-eight-") else 9
        names = [HOST] + [f"hop{index}.workbench.test" for index in range(1, hops + 1)]
        for owner, alias in zip(names, names[1:]):
            for kind in (1, 28):
                dns.answers[owner, kind] = [(owner, 5, alias)]
        expected.update(queries=18, chain=names)
        if hops == 9:
            expected.update(dns_state="failed", reason="dns_cname_limit_exceeded", all_steps=False)
    elif case == "cname-parent-deadline-tcp":
        for kind in (1, 28):
            dns.answers[HOST, kind] = [(HOST, 5, ALIAS)]
        dns.delay = 0.06
        dns.drop_names = {ALIAS}
        expected.update(dns_state="timed_out", reason="parent_deadline_exceeded", all_steps=False, queries=4,
            extra=["--timeout", "150ms"], execution="timed_out")
    elif case == "cname-step-deadline-tcp":
        names = [HOST] + [f"hop{index}.workbench.test" for index in range(1, 9)]
        for owner, alias in zip(names, names[1:]):
            for kind in (1, 28):
                dns.answers[owner, kind] = [(owner, 5, alias)]
        dns.delay = 0.6
        expected.update(dns_state="timed_out", reason="step_deadline_exceeded", all_steps=False, queries=None, execution="partial")
    elif case in ("chunk-budget", "chunk-exact", "chunk-crossing", "head-exact", "head-crossing"):
        if case.startswith("chunk-"):
            https.response = chunk_response({"chunk-budget":7422, "chunk-exact":6912, "chunk-crossing":6913}[case])
        else:
            https.response = padded_head(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n", 8192 if case == "head-exact" else 8193)
        if case in ("chunk-budget", "chunk-crossing", "head-crossing"):
            expected.update(all_steps=False, http_failure="http_headers_too_large")
    else:
        raise AssertionError("Unknown witness case " + case)
    return expected


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--case", choices=CASES + ["all"], default="all")
    parser.add_argument("--findings-only", action="store_true")
    parser.add_argument("--validate", action="store_true", help="Validate receipts against the candidate's current schemas (omit for the frozen R0 binary)")
    parser.add_argument("--json-results", action="store_true")
    options = parser.parse_args()
    selected = (FINDINGS if options.findings_only else CASES) if options.case == "all" else [options.case]
    validators = {}
    if options.validate:
        from jsonschema import Draft202012Validator, FormatChecker
        from referencing import Registry, Resource
        schemas = {path.name.removesuffix(".schema.json"):json.loads(path.read_text()) for path in (ROOT / "docs/sre-workbench/schemas").glob("*.schema.json")}
        registry = Registry().with_resources((schema["$id"], Resource.from_contents(schema)) for schema in schemas.values())
        validators = {name:Draft202012Validator(schemas[name], registry=registry, format_checker=FormatChecker()) for name in ("result", "diagnostics-data")}
    failures = []
    with tempfile.TemporaryDirectory(prefix="workbench-diagnostics-review-") as temporary:
        directory = Path(temporary)
        binary = directory / "installed" / "save"
        binary.parent.mkdir()
        shutil.copyfile(options.binary, binary)
        binary.chmod(0o755)
        print("BINARY_SHA256 " + hashlib.sha256(binary.read_bytes()).hexdigest(), flush=True)
        certificates = fixture.certificates(directory)
        dns = None
        https = fixture.Https(certificates["server.pem"], certificates["server.key"])
        config_file = directory / "config.json"
        input_file = directory / "input.json"
        input_file.write_text('{}')
        env = {"PATH":"/usr/bin:/bin", "LANG":"C.UTF-8", "LC_ALL":"C.UTF-8", "TZ":"UTC"}
        try:
            for case in selected:
                if dns is not None:
                    dns.close()
                dns = fixture.Dns()
                https.response = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n"
                target = {"id":"fixture-web", "host":HOST, "port":https.port, "path":"/probe", "allowed_ips":["127.0.0.1", "::1"],
                    "resolver":{"kind":"explicit", "address":"127.0.0.1", "port":dns.port, "transport":"udp"},
                    "tls_ca_file":str(certificates["ca.pem"]), "process":fixture.process_identity(os.getpid())}
                expected = configure(case, target, dns, https)
                config_file.write_text(json.dumps({"schema_version":1, "targets":[target]}))
                event_start = len(dns.events)
                connections_start = dns.tcp_connections_started
                https_start = https.connections
                started = time.monotonic()
                output = subprocess.run([str(binary), "--diagnostics-config", str(config_file), "--json", "diagnose", "--target", "fixture-web", "--input", str(input_file), *expected.get("extra", [])],
                    cwd=directory, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=12, check=False)
                receipt = json.loads(output.stdout)
                events = dns.events[event_start:]
                item = {"label":case, "exit":output.returncode, "elapsed_seconds":round(time.monotonic() - started, 3),
                    "dns_queries":events, "tcp_connections":dns.tcp_connections_started - connections_start, "receipt":receipt}
                if options.json_results:
                    print("DIAGNOSTICS_REVIEW_RESULT " + json.dumps(item, separators=(",", ":")), flush=True)
                try:
                    if validators:
                        validators["result"].validate(receipt)
                        validators["diagnostics-data"].validate(receipt["data"])
                    step = receipt["data"]["steps"][0]
                    if expected.get("queries") is not None:
                        assert len(events) == expected["queries"], f"Expected one transmission per name/type: {expected['queries']} questions, observed {len(events)}"
                    assert step["state"] == expected["dns_state"], f"DNS must {expected['dns_state']} from configured-server data, observed {step['state']}"
                    if expected["dns_state"] in ("failed", "timed_out"):
                        assert step["reason"] == expected["reason"], f"Expected named failure {expected['reason']}, observed {step['reason']}"
                        assert https.connections == https_start, "A failed DNS result must not start a service connection"
                        assert all(item["state"] == "blocked" for item in receipt["data"]["steps"][1:4])
                    if expected.get("execution"):
                        assert receipt["execution"]["status"] == expected["execution"]
                        assert output.returncode == 1 and receipt["data"]["steps"][4]["state"] == "succeeded"
                        assert any(event["name"] != HOST for event in events), "Deadline witness must enter real alias work"
                        if expected["reason"] == "step_deadline_exceeded":
                            assert 10000 <= step["duration_ms"] < 11000 and item["elapsed_seconds"] < 11.5
                            assert receipt["effective_limits"]["timeout_ms"] == 60000
                        else:
                            assert item["elapsed_seconds"] < 1
                    if case == "tcp-close":
                        alias_events = [event for event in events if event["name"] == ALIAS]
                        assert len([event for event in events if event["name"] == HOST]) == 2 and alias_events, "TCP closure witness must reach a reused stream after real original and alias requests"
                        assert len(alias_events) <= 2 and len({event["kind"] for event in alias_events}) == len(alias_events), "Closed reused TCP connection must not resend a CNAME query"
                        assert item["tcp_connections"] == 1, "Closed reused TCP connection must not reconnect"
                    if expected.get("addresses") is not None:
                        assert step["observation"]["addresses"] == expected["addresses"], "Only matching service/CNAME-owner answers may choose a peer"
                        assert step["observation"]["selected_peer"] == f"{expected['addresses'][0]}:{https.port}"
                    if expected.get("chain") is not None and step["state"] == "succeeded":
                        assert step["observation"]["cname_chains"] == {"a":expected["chain"], "aaaa":expected["chain"]}
                    if expected.get("http_failure") is not None:
                        http = receipt["data"]["steps"][3]
                        assert http["state"] == "failed" and http["reason"] == expected["http_failure"], "Every consumed HTTP header/framing byte must fit the 8192 ceiling"
                        assert receipt["coverage"]["state"] != "complete"
                    if expected["all_steps"]:
                        assert output.returncode == 0 and all(item["state"] == "succeeded" for item in receipt["data"]["steps"]), "Positive witness must finish all five real observations"
                        assert receipt["coverage"]["state"] == "complete"
                    wire = [(event["name"], event["kind"]) for event in events]
                    assert len(set(wire)) == len(wire), "A name/type question must never be retransmitted"
                    assert all(event["transport"] == target["resolver"]["transport"] for event in events), "No transport fallback"
                    assert item["tcp_connections"] == (1 if target["resolver"]["transport"] == "tcp" else 0), "No TCP reconnect or fallback"
                    if options.validate and step["observation"].get("resolver"):
                        profile = step["observation"]["resolver"]
                        questions = [(entry["name"], 1 if entry["type"] == "A" else 28) for entry in profile["questions"]]
                        assert len(questions) <= 18 and len(set(questions)) == len(questions)
                        assert all(sum(kind == entry[1] for entry in questions) <= 9 for kind in (1, 28))
                        assert set(wire) <= set(questions), "Resolver provenance must contain each transmitted question"
                        if step["state"] == "succeeded":
                            assert sorted(wire) == sorted(questions), "A complete DNS witness must account for all its transport requests"
                    print("PASS " + case, flush=True)
                except AssertionError as error:
                    failures.append((case, str(error)))
                    print("FAIL " + case + ": " + str(error), flush=True)
        finally:
            if dns is not None:
                dns.close()
            https.close()
    print(f"REVIEW_WITNESSES {len(selected) - len(failures)} passed; {len(failures)} failed", flush=True)
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
