"""Public connectivity mission against private DNS/TLS/kernel fixtures; use test-sandbox.sh."""
from __future__ import annotations

import argparse
import base64
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import signal
import shutil
import socket
import ssl
import struct
import subprocess
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[1]


def certificates(directory: Path) -> dict[str, Path]:
    specification = importlib.util.spec_from_file_location("workbench_grafana_fixture", ROOT / "tools/verify-grafana.py")
    module = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(module)
    result = module.certificates(directory)
    extension = directory / "server.ext"
    extension.write_text(extension.read_text().replace("DNS:localhost,IP:127.0.0.1", "DNS:diagnostics.workbench.test,IP:127.0.0.1,IP:::1"))
    subprocess.run(["/usr/bin/openssl", "x509", "-req", "-in", "server.csr", "-CA", "ca.pem", "-CAkey", "ca.key",
        "-CAcreateserial", "-days", "2", "-sha256", "-extfile", "server.ext", "-out", "server.pem"], cwd=directory,
        env={"PATH":"/usr/bin:/bin", "OPENSSL_CONF":"/dev/null"}, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, timeout=10, check=True)
    return result


def process_identity(pid: int) -> dict:
    fields = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
    uids = next(line.split()[1:] for line in Path(f"/proc/{pid}/status").read_text().splitlines() if line.startswith("Uid:"))
    return {"backend":"linux-process-v1", "pid":pid, "start_time_ticks":fields[19], "uid":int(uids[0])}


class Dns:
    def __init__(self) -> None:
        self.socket = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.socket.bind(("127.0.0.1", 0))
        self.socket.settimeout(0.05)
        self.port = self.socket.getsockname()[1]
        self.addresses = ["127.0.0.1"]
        self.events: list[dict] = []
        self.sent = 0
        self.after_send = None
        self.answers: dict[tuple[str, int], list[tuple[str, int, str]]] = {}
        self.drop_names: set[str] = set()
        self.close_names: set[str] = set()
        self.tcp_connections_started = 0
        self.delay = 0.0
        self.flags = 0x8180
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self.serve, daemon=True)
        self.thread.start()
        self.tcp = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.tcp.bind(("127.0.0.1", self.port))
        self.tcp.listen(4)
        self.tcp.settimeout(0.05)
        self.workers: list[threading.Thread] = []
        self.tcp_connections: set[socket.socket] = set()
        self.tcp_thread = threading.Thread(target=self.serve_tcp, daemon=True)
        self.tcp_thread.start()

    def reply(self, packet: bytes, peer: tuple, transport: str) -> bytes | None:
        offset = 12
        labels = []
        while offset < len(packet) and packet[offset]:
            length = packet[offset]
            labels.append(packet[offset + 1:offset + 1 + length].decode("ascii"))
            offset += length + 1
        offset += 1
        kind, klass = struct.unpack("!HH", packet[offset:offset + 4])
        name = ".".join(labels).lower()
        self.events.append({"name":name, "kind":kind, "peer":peer, "transport":transport})
        question = packet[12:offset + 4]
        if name in self.drop_names:
            return None
        if (name, kind) in self.answers:
            def encoded_name(value: str) -> bytes:
                return b"".join(bytes([len(label)]) + label.encode("ascii") for label in value.rstrip('.').split('.')) + b"\x00"
            records = b""
            selected = self.answers[name, kind]
            for owner, record_type, value in selected:
                data = encoded_name(value) if record_type == 5 else socket.inet_pton(socket.AF_INET6 if record_type == 28 else socket.AF_INET, value)
                records += encoded_name(owner) + struct.pack("!HHIH", record_type, klass, 0, len(data)) + data
            return packet[:2] + struct.pack("!HHHHH", self.flags, 1, len(selected), 0, 0) + question + records
        addresses = [address for address in self.addresses if (":" in address) == (kind == 28)] if kind in (1, 28) else []
        records = b"".join(b"\xc0\x0c" + struct.pack("!HHIH", kind, klass, 0, 16 if kind == 28 else 4)
                           + socket.inet_pton(socket.AF_INET6 if kind == 28 else socket.AF_INET, address)
                           for address in addresses)
        return packet[:2] + struct.pack("!HHHHH", self.flags, 1, len(addresses), 0, 0) + question + records

    def serve(self) -> None:
        while not self.stop.is_set():
            try:
                packet, peer = self.socket.recvfrom(4096)
            except socket.timeout:
                continue
            except OSError:
                break
            if len(packet) < 17:
                continue
            response = self.reply(packet, peer, "udp")
            if self.stop.wait(self.delay):
                break
            if response is None:
                continue
            try:
                self.socket.sendto(response, peer)
                self.sent += 1
                if self.after_send:
                    self.after_send()
            except OSError:
                break

    def serve_tcp(self) -> None:
        while not self.stop.is_set():
            try:
                connection, peer = self.tcp.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            connection.settimeout(8)
            self.tcp_connections_started += 1
            self.tcp_connections.add(connection)
            worker = threading.Thread(target=self.tcp_exchange, args=(connection, peer), daemon=True)
            self.workers.append(worker)
            worker.start()

    def tcp_exchange(self, connection: socket.socket, peer: tuple) -> None:
        def read_exact(count: int) -> bytes:
            value = b""
            while len(value) < count:
                chunk = connection.recv(count - len(value))
                if not chunk:
                    raise EOFError
                value += chunk
            return value
        try:
            while not self.stop.is_set():
                length = struct.unpack("!H", read_exact(2))[0]
                if not 17 <= length <= 4096:
                    break
                response = self.reply(read_exact(length), peer, "tcp")
                if self.events[-1]["name"] in self.close_names:
                    break
                if response is None:
                    continue
                if self.stop.wait(self.delay):
                    break
                connection.sendall(struct.pack("!H", len(response)) + response)
                self.sent += 1
                if self.after_send:
                    self.after_send()
        except (OSError, EOFError):
            pass
        finally:
            self.tcp_connections.discard(connection)
            connection.close()

    def close(self) -> None:
        self.stop.set()
        self.socket.close()
        self.tcp.close()
        for connection in tuple(self.tcp_connections):
            try:
                connection.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
        self.thread.join(2)
        self.tcp_thread.join(2)
        for worker in self.workers:
            worker.join(2)
        assert not self.thread.is_alive() and not self.tcp_thread.is_alive() and not any(worker.is_alive() for worker in self.workers), "DNS fixture worker cleanup"


class Https:
    def __init__(self, cert: Path, key: Path, address: str = "127.0.0.1") -> None:
        self.socket = socket.socket(socket.AF_INET6 if ":" in address else socket.AF_INET, socket.SOCK_STREAM)
        self.socket.bind((address, 0))
        self.socket.listen(16)
        self.socket.settimeout(0.05)
        self.port = self.socket.getsockname()[1]
        self.context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        self.context.load_cert_chain(cert, key)
        self.context.set_alpn_protocols(["http/1.1"])
        self.response = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 5\r\n\r\nhello"
        self.delay = 0.0
        self.body_delay = 0.0
        self.events: list[dict] = []
        self.connections = 0
        self.sockets: set[socket.socket] = set()
        self.workers: list[threading.Thread] = []
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self.serve, daemon=True)
        self.thread.start()

    def serve(self) -> None:
        while not self.stop.is_set():
            try:
                connection, peer = self.socket.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            self.connections += 1
            connection.settimeout(1)
            worker = threading.Thread(target=self.exchange, args=(connection, peer), daemon=True)
            self.workers.append(worker)
            worker.start()

    def exchange(self, connection: socket.socket, peer: tuple) -> None:
        stream = connection
        try:
            stream = self.context.wrap_socket(connection, server_side=True)
            self.sockets.add(stream)
            request = b""
            while not request.endswith(b"\r\n\r\n") and len(request) < 8192:
                chunk = stream.recv(1024)
                if not chunk:
                    return
                request += chunk
            self.events.append({"request":request.decode("ascii"), "peer":peer, "at":time.monotonic()})
            if self.stop.wait(self.delay):
                return
            if self.body_delay:
                header, body = self.response.split(b"\r\n\r\n", 1)
                stream.sendall(header + b"\r\n\r\n")
                if self.stop.wait(self.body_delay):
                    return
                stream.sendall(body)
            else:
                stream.sendall(self.response)
        except (OSError, ssl.SSLError):
            pass  # Failed TLS and cancelled sockets are expected fixture observations.
        finally:
            self.sockets.discard(stream)
            stream.close()

    def close(self) -> None:
        self.stop.set()
        self.socket.close()
        for stream in tuple(self.sockets):
            try:
                stream.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
        self.thread.join(2)
        for worker in self.workers:
            worker.join(2)
        assert not self.thread.is_alive() and not any(worker.is_alive() for worker in self.workers), "HTTPS fixture cleanup"


def comparable(receipt: dict) -> dict:
    value = copy.deepcopy(receipt)
    for key in ("request_id", "run_id", "started_at", "finished_at", "duration_ms"):
        value.pop(key)
    for source in value["sources"]:
        source.pop("observed_at", None)
    value["data"]["vantage"].pop("observer_pid")
    for step in value["data"]["steps"]:
        for key in ("started_offset_ms", "duration_ms", "observed_at"):
            step.pop(key)
        observation = step["observation"]
        observation.pop("local_address", None)
        if isinstance(observation.get("vantage"), dict):
            observation["vantage"].pop("observer_pid")
        if isinstance(observation.get("connection"), dict):
            observation["connection"].pop("local_address", None)
    return value


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--no-schema", action="store_true")
    parser.add_argument("--walking-skeleton-only", action="store_true")
    parser.add_argument("--json-results", action="store_true")
    options = parser.parse_args()
    binary = options.binary.resolve()
    validators = {}
    if not options.no_schema:
        from jsonschema import Draft202012Validator, FormatChecker
        from referencing import Registry, Resource
        schemas = {path.name.removesuffix(".schema.json"):json.loads(path.read_text()) for path in (ROOT / "docs/sre-workbench/schemas").glob("*.schema.json")}
        registry = Registry().with_resources((schema["$id"], Resource.from_contents(schema)) for schema in schemas.values())
        validators = {name:Draft202012Validator(schemas[name], registry=registry, format_checker=FormatChecker()) for name in ("result", "diagnostics-data")}
    count = 0
    with tempfile.TemporaryDirectory(prefix="workbench-diagnostics-") as temporary:
        directory = Path(temporary)
        installed = directory / "installed" / "bin" / "save"
        installed.parent.mkdir(parents=True)
        original_binary_sha = hashlib.sha256(binary.read_bytes()).hexdigest()
        shutil.copyfile(binary, installed)
        installed.chmod(0o755)
        assert hashlib.sha256(installed.read_bytes()).hexdigest() == original_binary_sha
        binary = installed
        certs = certificates(directory)
        dns = Dns()
        https = Https(certs["server.pem"], certs["server.key"])
        try:
            config = {"schema_version":1, "targets":[{"id":"fixture-web", "host":"diagnostics.workbench.test", "port":https.port,
                "path":"/probe", "allowed_ips":["127.0.0.1"], "resolver":{"kind":"explicit", "address":"127.0.0.1", "port":dns.port, "transport":"udp"},
                "tls_ca_file":str(certs["ca.pem"]), "process":process_identity(os.getpid())}]}
            config_file = directory / "config.json"
            config_file.write_text(json.dumps(config))
            input_file = directory / "input.json"
            input_file.write_text('{"capture_body":false,"on_failure":"continue"}')
            request = {"spec_version":"0.1", "request_id":"diagnostics-fixture", "operation":"diagnostics.run", "operation_version":1,
                "target":{"kind":"connection", "id":"fixture-web"}, "inputs":{"id":"connectivity", "version":1, "input":{"capture_body":False, "on_failure":"continue"}},
                "limits":{"timeout_ms":60000, "max_output_bytes":16384, "concurrency":2}, "record":"never"}
            request_file = directory / "request.json"
            request_file.write_text(json.dumps(request))
            env = {"PATH":"/usr/bin:/bin", "LANG":"C.UTF-8", "LC_ALL":"C.UTF-8", "TZ":"UTC"}

            def run(label: str, *, wire: dict | None = None, expected: int = 0, extra: list[str] | None = None, configuration: Path = config_file, environment: dict | None = None) -> dict:
                nonlocal count
                if wire is not None:
                    request_file.write_text(json.dumps(wire))
                    args = ["call", "--request", str(request_file)]
                else:
                    args = ["diagnose", "--target", "fixture-web", "--pack", "connectivity", "--input", str(input_file), *(extra or [])]
                result = subprocess.run([str(binary), "--diagnostics-config", str(configuration), "--json", *args], cwd=directory, env=env | (environment or {}),
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=12, check=False)
                assert result.returncode == expected, (label, result.returncode, result.stdout, result.stderr)
                receipt = json.loads(result.stdout)
                if validators:
                    validators["result"].validate(receipt)
                    if receipt["data"].get("steps"):
                        validators["diagnostics-data"].validate(receipt["data"])
                assert receipt["assessment"] == "not_assessed" and receipt["record_mode"] == "never"
                assert receipt["execution"]["child_exit_code"] is None and receipt["execution"]["signal"] is None
                if options.json_results:
                    print("DIAGNOSTICS_RESULT " + json.dumps({"label":label, "receipt":receipt}, separators=(",", ":")), flush=True)
                count += 1
                return receipt

            mission = run("walking-five-check-cli")
            assert mission["execution"]["status"] == "succeeded" and mission["coverage"]["state"] == "complete"
            steps = mission["data"]["steps"]
            assert [step["id"] for step in steps] == ["dns", "tcp", "tls", "http", "process"]
            assert all(step["attempted"] and step["state"] == "succeeded" and step["observed_at"] for step in steps)
            peer = f"127.0.0.1:{https.port}"
            assert steps[0]["observation"]["addresses"] == ["127.0.0.1"] and steps[0]["observation"]["selected_peer"] == peer
            assert steps[1]["observation"]["peer"] == steps[2]["observation"]["peer"] == steps[3]["observation"]["connection"]["peer"] == peer
            assert steps[2]["observation"]["hostname_verified"] and steps[2]["observation"]["trust_verified"]
            expected_cert = ssl.PEM_cert_to_DER_cert(certs["server.pem"].read_text())
            assert steps[2]["observation"]["certificate_sha256"] == "sha256:" + hashlib.sha256(expected_cert).hexdigest()
            assert steps[3]["observation"]["status"] == 200 and steps[3]["observation"]["body_capture"]["content"] is None
            assert steps[4]["observation"]["pid"] == os.getpid() and steps[4]["observation"]["identity_verified"]
            assert mission["data"]["binding"]["identity"]["configuration_sha256"] == "sha256:" + hashlib.sha256(config_file.read_bytes()).hexdigest()
            assert mission["data"]["binding"]["identity"]["additional_ca_sha256"] == "sha256:" + hashlib.sha256(certs["ca.pem"].read_bytes()).hexdigest()
            definition_bytes = json.dumps(mission["data"]["pack"]["definition"], sort_keys=True, separators=(",", ":")).encode()
            assert mission["data"]["pack"]["sha256"] == "sha256:" + hashlib.sha256(definition_bytes).hexdigest()
            assert {event["kind"] for event in dns.events} == {1, 28} and len(dns.events) == 2
            assert https.connections == 3 and len(https.events) == 1
            assert https.events[0]["request"] == f"GET /probe HTTP/1.1\r\nHost: diagnostics.workbench.test:{https.port}\r\nAccept: */*\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n"
            parity = run("walking-five-check-call", wire=request)
            assert comparable(mission) == comparable(parity)
            print("PASS: real public DNS -> TCP -> TLS -> HTTP and independent private kernel process; CLI/call parity", flush=True)
            if options.walking_skeleton_only:
                return 0

            config["targets"][0].pop("process")
            config_file.write_text(json.dumps(config))
            input_file.write_text('{"on_failure":"stop"}')
            omitted = run("optional-unconfigured-process-does-not-invent-a-stop-failure", expected=1)
            assert all(step["state"] == "succeeded" for step in omitted["data"]["steps"][:4]), "An omitted optional binding is not an observed failure"
            assert omitted["data"]["steps"][4]["state"] == "unconfigured"
            config["targets"][0]["process"] = process_identity(os.getpid())
            config_file.write_text(json.dumps(config))
            input_file.write_text('{"capture_body":false,"on_failure":"continue"}')

            original = copy.deepcopy(config)
            normal_response = https.response
            canary = directory / "external-canary"
            canary.write_bytes(b"unchanged external fixture bytes\n")
            command_marker = directory / "must-not-execute"

            def reset() -> None:
                config.clear()
                config.update(copy.deepcopy(original))
                config_file.write_text(json.dumps(config))
                input_file.write_text('{"capture_body":false,"on_failure":"continue"}')
                dns.addresses = ["127.0.0.1"]
                dns.flags = 0x8180
                dns.delay = 0
                https.response = normal_response
                https.delay = 0
                https.body_delay = 0

            def write_config() -> None:
                config_file.write_text(json.dumps(config))

            def check_step(receipt: dict, index: int, state: str, code: str | None = None) -> dict:
                step = receipt["data"]["steps"][index]
                assert step["state"] == state and step["reason"] == code, step
                if state == "blocked":
                    assert not step["attempted"] and step["started_offset_ms"] is None and step["duration_ms"] is None
                return step

            # DNS binding and one-time peer selection.
            for label, answers in [("dns-denied", ["127.0.0.2"]), ("dns-mixed-denied", ["127.0.0.1", "127.0.0.2"]),
                                   ("dns-denied-ipv6", ["127.0.0.1", "::1"]), ("dns-mapped-answer", ["::ffff:127.0.0.1"])]:
                reset()
                dns.addresses = answers
                before = https.connections
                value = run(label, expected=1)
                check_step(value, 0, "failed", "dns_address_denied")
                check_step(value, 1, "blocked", "prerequisite_failed")
                check_step(value, 4, "succeeded")
                assert https.connections == before, "Denied DNS answers must create no service socket"
            reset()
            dns.addresses = []
            value = run("dns-no-addresses", expected=1)
            check_step(value, 0, "failed", "dns_no_addresses")
            reset()
            responses_before = dns.sent
            config_before = config_file.read_bytes()
            def change_binding_after_dns() -> None:
                if dns.sent >= responses_before + 2:
                    dns.addresses = ["127.0.0.2"]
                    changed = copy.deepcopy(original)
                    changed["targets"][0]["host"] = "127.0.0.2"
                    changed["targets"][0]["path"] = "/private-canary"
                    replacement = directory / "replacement.json"
                    replacement.write_text(json.dumps(changed))
                    replacement.replace(config_file)
                    dns.after_send = None
            dns.after_send = change_binding_after_dns
            before_queries = len(dns.events)
            value = run("changed-dns-and-config-cannot-retarget-active-snapshot")
            assert dns.after_send is None and len(dns.events) == before_queries + 2
            assert config_file.read_bytes() != config_before
            assert value["data"]["binding"]["identity"]["configuration_sha256"] == "sha256:" + hashlib.sha256(config_before).hexdigest()
            assert value["data"]["steps"][3]["observation"]["connection"]["peer"] == f"127.0.0.1:{https.port}"
            assert https.events[-1]["request"].startswith("GET /probe ")
            reset()
            dns.flags = 0x8182
            value = run("dns-servfail-no-partial-family", expected=1)
            check_step(value, 0, "failed", "dns_lookup_failed")
            reset()
            dns.flags = 0x8380
            previous_tcp = len([event for event in dns.events if event["transport"] == "tcp"])
            value = run("dns-truncated-no-tcp-fallback", expected=1)
            check_step(value, 0, "failed", "dns_response_truncated")
            assert len([event for event in dns.events if event["transport"] == "tcp"]) == previous_tcp
            reset()
            config["targets"][0]["resolver"]["transport"] = "tcp"
            write_config()
            before = len(dns.events)
            value = run("dns-explicit-tcp")
            assert {event["transport"] for event in dns.events[before:]} == {"tcp"}
            assert value["data"]["steps"][0]["observation"]["resolver"]["transport"] == "tcp"
            reset()
            config["targets"][0]["host"] = "127.0.0.1"
            write_config()
            before = len(dns.events)
            value = run("literal-ip-no-resolver")
            assert value["data"]["steps"][0]["observation"]["mode"] == "literal_ip" and len(dns.events) == before
            assert value["data"]["steps"][2]["observation"]["hostname"] == "127.0.0.1"
            reset()
            config["targets"][0]["host"] = "127.0.0.2"
            write_config()
            before = https.connections
            value = run("literal-ip-denied", expected=1)
            check_step(value, 0, "failed", "dns_address_denied")
            assert https.connections == before
            reset()
            ipv6 = Https(certs["server.pem"], certs["server.key"], "::1")
            try:
                config["targets"][0].update(host="0:0:0:0:0:0:0:1", port=ipv6.port, allowed_ips=["::1"])
                write_config()
                value = run("literal-ipv6-canonical-name-and-peer")
                assert value["data"]["binding"]["hostname"] == "::1"
                assert value["data"]["steps"][2]["observation"]["peer"] == f"[::1]:{ipv6.port}"
            finally:
                ipv6.close()
            if not Path("/etc/resolv.conf").exists():
                reset()
                config["targets"][0]["resolver"] = {"kind":"system","transport":"udp"}
                write_config()
                value = run("system-resolver-unavailable-private-boundary", expected=1)
                check_step(value, 0, "failed", "dns_system_configuration_unavailable")
                check_step(value, 4, "succeeded")
            else:
                print("SKIP: unavailable system resolver requires an absent controlled /etc/resolv.conf; no ambient resolver is contacted", flush=True)

            # Network failures preserve the independent branch and block dependencies.
            reset()
            unused = socket.socket()
            unused.bind(("127.0.0.1", 0))
            config["targets"][0]["port"] = unused.getsockname()[1]
            unused.close()
            write_config()
            value = run("tcp-refused-independent-process", expected=1)
            check_step(value, 1, "failed", "tcp_connect_failed")
            check_step(value, 2, "blocked", "prerequisite_failed")
            check_step(value, 4, "succeeded")
            reset()
            config["targets"][0].pop("tls_ca_file")
            write_config()
            before = len(https.events)
            value = run("unknown-ca-and-ambient-trust-ignored", expected=1, environment={"SSL_CERT_FILE":str(certs["ca.pem"]), "SSL_CERT_DIR":str(directory)})
            check_step(value, 2, "failed", "tls_validation_failed")
            check_step(value, 3, "blocked", "prerequisite_failed")
            assert len(https.events) == before
            reset()
            wrong = Https(certs["wrong.pem"], certs["wrong.key"])
            try:
                config["targets"][0]["port"] = wrong.port
                write_config()
                value = run("wrong-certificate-name", expected=1)
                check_step(value, 2, "failed", "tls_validation_failed")
                assert not wrong.events
            finally:
                wrong.close()
            reset()
            expired_cert = directory / "expired.pem"
            subprocess.run(["/usr/bin/openssl", "x509", "-req", "-in", "server.csr", "-CA", "ca.pem", "-CAkey", "ca.key",
                "-CAcreateserial", "-days", "-1", "-sha256", "-extfile", "server.ext", "-out", str(expired_cert)], cwd=directory,
                env={"PATH":"/usr/bin:/bin", "OPENSSL_CONF":"/dev/null"}, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, timeout=10, check=True)
            not_after = ssl._ssl._test_decode_cert(str(expired_cert))["notAfter"]
            assert ssl.cert_time_to_seconds(not_after) < time.time(), "Fixture prerequisite: the matching CA/name certificate is actually expired"
            expired = Https(expired_cert, certs["server.key"])
            try:
                config["targets"][0]["port"] = expired.port
                write_config()
                value = run("expired-matching-name-and-trust-certificate", expected=1)
                check_step(value, 2, "failed", "tls_validation_failed")
                assert not expired.events
            finally:
                expired.close()

            # Request hygiene, response errors, framing and encoded bounds.
            reset()
            keylog = directory / "must-not-keylog"
            value = run("proxy-keylog-and-auth-not-inherited", environment={"HTTPS_PROXY":"http://127.0.0.1:1", "ALL_PROXY":"http://127.0.0.1:1",
                "SSLKEYLOGFILE":str(keylog), "HTTP_AUTHORIZATION":"Bearer unrelated-sensitive-canary"})
            assert not keylog.exists() and "unrelated-sensitive-canary" not in json.dumps(value)
            assert "Authorization:" not in https.events[-1]["request"] and "Cookie:" not in https.events[-1]["request"]
            for status in (302, 401, 500):
                reset()
                https.response = f"HTTP/1.1 {status} Fixture\r\nContent-Length: 17\r\nLocation: https://127.0.0.2/private\r\nSet-Cookie: sensitive-canary\r\n\r\nprivate-error-123".encode()
                before = len(https.events)
                value = run(f"http-{status}-no-redirect-retry-error-echo", expected=1)
                step = check_step(value, 3, "failed", "http_status_failed")
                assert step["observation"]["status"] == status and len(https.events) == before + 1
                assert "sensitive-canary" not in json.dumps(value) and "private-error" not in json.dumps(value)
            input_file.write_text('{"capture_body":true}')
            value = run("http-error-body-explicit-opt-in-remains-untrusted", expected=1)
            captured = check_step(value, 3, "failed", "http_status_failed")["observation"]["body_capture"]
            assert base64.b64decode(captured["content"]) == b"private-error-123" and not captured["redaction_applied"]
            assert all("private-error" not in error["message"] for error in value["errors"])
            framing = [
                ("duplicate-length", b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\nx", "http_framing_invalid"),
                ("conflicting-length", b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\nxx", "http_framing_invalid"),
                ("combined-length", b"HTTP/1.1 200 OK\r\nContent-Length: 1,1\r\n\r\nx", "http_framing_invalid"),
                ("length-transfer-conflict", b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n", "http_framing_invalid"),
                ("transfer-chain", b"HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip,chunked\r\n\r\n0\r\n\r\n", "http_framing_invalid"),
                ("duplicate-transfer", b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n", "http_framing_invalid"),
                ("folded-header", b"HTTP/1.1 200 OK\r\n folded: sensitive-canary\r\n\r\n", "http_headers_invalid"),
                ("bare-lf", b"HTTP/1.1 200 OK\n\n", "http_headers_invalid"),
                ("premature-body-eof", b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\n\r\nx", "http_response_incomplete"),
                ("informational", b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n", "http_informational_unsupported"),
                ("protocol-switch", b"HTTP/1.1 101 Switching Protocols\r\n\r\n", "http_informational_unsupported"),
                ("chunk-extension", b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1;foo=bar\r\nx\r\n0\r\n\r\n", "http_chunk_syntax_unsupported"),
                ("chunk-terminator", b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1\r\nxXX0\r\n\r\n", "http_framing_invalid"),
                ("trailer", b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\nSecret: sensitive-canary\r\n\r\n", "http_trailers_unsupported"),
                ("unknown-content-encoding", b"HTTP/1.1 200 OK\r\nContent-Encoding: unknown\r\nContent-Length: 0\r\n\r\n", "http_encoding_unsupported"),
                ("too-long-header", b"HTTP/1.1 200 OK\r\nSecret: " + b"x" * 1100 + b"\r\n\r\n", "http_headers_too_large"),
                ("header-total", b"HTTP/1.1 200 OK\r\n" + b"X-Field: " + b"x" * 900 + b"\r\n" + (b"X-Field: " + b"x" * 900 + b"\r\n") * 9 + b"\r\n", "http_headers_too_large"),
                ("field-count", b"HTTP/1.1 200 OK\r\n" + b"X: a\r\n" * 65 + b"Content-Length: 0\r\n\r\n", "http_headers_too_large"),
                ("chunk-count", b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n" + b"1\r\nx\r\n" * 257 + b"0\r\n\r\n", "http_chunk_limit_exceeded"),
            ]
            for label, response, code in framing:
                reset()
                https.response = response
                value = run("http-framing-" + label, expected=1)
                check_step(value, 3, "failed", code)
                assert value["coverage"]["state"] != "complete" and "sensitive-canary" not in json.dumps(value)
            reset()
            https.response = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nhe\r\n3\r\nllo\r\n0\r\n\r\n"
            input_file.write_text('{"capture_body":true}')
            value = run("http-valid-chunked-opt-in")
            body = value["data"]["steps"][3]["observation"]["body_capture"]
            assert base64.b64decode(body["content"]) == b"hello" and not body["truncated"]
            reset()
            payload = b"\x00\xff$(touch " + str(command_marker).encode() + b")"
            https.response = b"HTTP/1.1 200 OK\r\nContent-Length: " + str(len(payload)).encode() + b"\r\n\r\n" + payload
            input_file.write_text('{"capture_body":true}')
            value = run("body-binary-lossless-untrusted-no-execution")
            body = value["data"]["steps"][3]["observation"]["body_capture"]
            assert base64.b64decode(body["content"]) == payload and body["classification"] == "untrusted_content" and not body["redaction_applied"]
            assert not command_marker.exists()
            reset()
            payload = b"x" * 20000
            https.response = b"HTTP/1.1 200 OK\r\nContent-Length: 20000\r\n\r\n" + payload
            input_file.write_text('{"capture_body":true}')
            value = run("body-opt-in-truncation-and-caller-cap", expected=1, extra=["--max-output-bytes", "1024"])
            body = check_step(value, 3, "partial", "http_body_truncated")["observation"]["body_capture"]
            assert body["truncated"] and not body["framing_complete"] and len(base64.b64decode(body["content"])) == 1024
            assert value["coverage"]["state"] == "partial" and len(json.dumps(value["data"]).encode()) < 65536
            reset()
            import gzip
            compressed = gzip.compress(b"x" * 50000)
            https.response = b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: " + str(len(compressed)).encode() + b"\r\n\r\n" + compressed
            input_file.write_text('{"capture_body":true}')
            value = run("body-encoded-no-expansion")
            body = value["data"]["steps"][3]["observation"]["body_capture"]
            assert base64.b64decode(body["content"]) == compressed and not body["decompressed"]

            # Process identity, graph policy and actual parent cancellation/deadline.
            reset()
            config["targets"][0].pop("process")
            write_config()
            value = run("process-unconfigured", expected=1)
            check_step(value, 4, "unconfigured", "process_unconfigured")
            reset()
            config["targets"][0]["process"]["start_time_ticks"] = "0"
            write_config()
            value = run("process-instance-mismatch", expected=1)
            check_step(value, 4, "failed", "process_identity_mismatch")
            reset()
            config["targets"][0]["process"]["uid"] += 1
            write_config()
            value = run("process-uid-mismatch", expected=1)
            check_step(value, 4, "failed", "process_identity_mismatch")
            reset()
            config["targets"][0]["process"]["backend"] = "future-backend"
            write_config()
            value = run("process-unsupported-no-substitution", expected=1)
            check_step(value, 4, "unsupported", "process_backend_unsupported")
            reset()
            child = subprocess.Popen(["/usr/bin/sleep", "30"], env=env)
            try:
                config["targets"][0]["process"] = process_identity(child.pid)
                child.send_signal(signal.SIGSTOP)
                until = time.monotonic() + 1
                while Path(f"/proc/{child.pid}/stat").read_text().rsplit(")", 1)[1].split()[0] != "T" and time.monotonic() < until:
                    time.sleep(0.002)
                write_config()
                value = run("process-stopped-is-observation-not-health")
                assert value["data"]["steps"][4]["observation"]["state"] == "stopped" and value["assessment"] == "not_assessed"
                child.send_signal(signal.SIGCONT)
                child.terminate()
                until = time.monotonic() + 2
                while Path(f"/proc/{child.pid}/stat").read_text().rsplit(")", 1)[1].split()[0] != "Z" and time.monotonic() < until:
                    time.sleep(0.002)
                assert Path(f"/proc/{child.pid}/stat").read_text().rsplit(")", 1)[1].split()[0] == "Z", "Fixture prerequisite: unreaped instance is a real zombie"
                value = run("process-zombie-is-observation-not-health")
                assert value["data"]["steps"][4]["observation"]["state"] == "zombie" and value["assessment"] == "not_assessed"
                child.wait(timeout=2)
                value = run("process-exited-instance-gone", expected=1)
                check_step(value, 4, "failed", "process_gone")
            finally:
                if child.poll() is None:
                    child.kill()
                    child.wait(timeout=2)
            reset()
            dns.addresses = ["127.0.0.2"]
            input_file.write_text('{"on_failure":"stop"}')
            value = run("stop-serial-prohibits-independent-admission", expected=1, extra=["--concurrency", "1"])
            check_step(value, 4, "blocked", "stopped_after_failure")
            assert value["effective_limits"]["concurrency"] == 1
            reset()
            config["targets"][0]["process"]["start_time_ticks"] = "0"
            write_config()
            input_file.write_text('{"on_failure":"stop"}')
            dns.delay = 0.1
            before = https.connections
            value = run("stop-concurrent-keeps-active-dns-blocks-next-tcp", expected=1)
            check_step(value, 4, "failed", "process_identity_mismatch")
            check_step(value, 0, "succeeded")
            check_step(value, 1, "blocked", "stopped_after_failure")
            assert https.connections == before
            reset()
            dns.delay = 0.4
            before = https.connections
            sent_before = dns.sent
            started = time.monotonic()
            value = run("parent-deadline-pending-dns", expected=1, extra=["--timeout", "80ms"])
            assert time.monotonic() - started < 1 and value["execution"]["status"] == "timed_out"
            check_step(value, 0, "timed_out", "parent_deadline_exceeded")
            check_step(value, 4, "succeeded")
            assert https.connections == before
            dns.delay = 0
            until = time.monotonic() + 1
            while dns.sent < sent_before + 2 and time.monotonic() < until:
                time.sleep(0.002)
            assert dns.sent >= sent_before + 2, "Drain only fixture-owned delayed responses before the next mission"
            reset()
            https.body_delay = 0.4
            value = run("parent-deadline-slow-body", expected=1, extra=["--timeout", "80ms"])
            assert value["execution"]["status"] == "timed_out"
            assert value["data"]["steps"][0]["state"] == value["data"]["steps"][1]["state"] == value["data"]["steps"][2]["state"] == "succeeded"
            reset()
            https.delay = 0.3
            value = run("parent-deadline-slow-headers", expected=1, extra=["--timeout", "80ms"])
            assert value["execution"]["status"] == "timed_out" and value["data"]["steps"][3]["state"] == "timed_out"
            reset()
            https.body_delay = 11
            started = time.monotonic()
            value = run("step-ten-second-ceiling-preserves-parent-and-other-observations", expected=1)
            step = check_step(value, 3, "timed_out", "step_deadline_exceeded")
            assert 10000 <= step["duration_ms"] < 11000 and time.monotonic() - started < 11.5
            assert value["execution"]["status"] == "partial" and value["effective_limits"]["timeout_ms"] == 60000
            for sig, exit_code in [(signal.SIGINT, 130), (signal.SIGTERM, 143)]:
                reset()
                https.body_delay = 0.3
                before = len(https.events)
                process = subprocess.Popen([str(binary), "--diagnostics-config", str(config_file), "--json", "diagnose", "--target", "fixture-web", "--input", str(input_file)],
                    cwd=directory, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                try:
                    until = time.monotonic() + 2
                    while len(https.events) == before and process.poll() is None and time.monotonic() < until:
                        time.sleep(0.002)
                    assert len(https.events) == before + 1 and process.poll() is None, "Cancellation needs an active real HTTP request"
                    process.send_signal(sig)
                    output, errors = process.communicate(timeout=2)
                    assert process.returncode == exit_code, (process.returncode, errors)
                    value = json.loads(output)
                    assert value["execution"]["status"] == "cancelled" and value["data"]["cancellation_signal"] == sig
                    if validators:
                        validators["result"].validate(value)
                        validators["diagnostics-data"].validate(value["data"])
                    if options.json_results:
                        print("DIAGNOSTICS_RESULT " + json.dumps({"label":f"active-http-signal-{sig}", "receipt":value}, separators=(",", ":")), flush=True)
                    count += 1
                    time.sleep(0.02)
                    assert len(https.events) == before + 1, "No new HTTP request after cancellation"
                finally:
                    if process.poll() is None:
                        process.kill()
                        process.communicate(timeout=2)
            reset()
            human = subprocess.run([str(binary), "--diagnostics-config", str(config_file), "diagnose", "--target", "fixture-web", "--input", str(input_file)],
                cwd=directory, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=3)
            assert human.returncode == 0 and b"Assessment: not_assessed" in human.stdout and b"Coverage: complete" in human.stdout
            human_data = json.loads(human.stdout.decode().split("Observation:\n", 1)[1].splitlines()[0])
            assert all(step["state"] == "succeeded" for step in human_data["steps"])
            assert human_data["steps"][3]["observation"]["status"] == 200 and human_data["binding"]["target"] == "fixture-web"
            assert canary.read_bytes() == b"unchanged external fixture bytes\n" and not command_marker.exists()
            assert config_file.read_bytes() == json.dumps(original).encode(), "Read-only product preserves configuration"
            print("PASS: human/JSON observations agree; canaries/source bytes unchanged; fixtures join and close", flush=True)
        finally:
            dns.close()
            https.close()
    print(f"PASS: {count} public diagnostics receipts; " + ("schema checks skipped explicitly" if options.no_schema else "all schema-valid"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
