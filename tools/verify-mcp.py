"""Independent raw stdio MCP contract checks; run only in the private test sandbox."""
from __future__ import annotations

import argparse
import copy
import fcntl
import json
import os
from pathlib import Path
import select
import signal
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
MODERN = "2026-07-28"
LEGACY = "2025-11-25"
MAX_FRAME = 3 * 1024 * 1024


def request(ident: str | int, method: str, params: dict | None = None,
            modern: bool = True) -> dict:
    params = copy.deepcopy(params if params is not None else {})
    if modern:
        params["_meta"] = {"io.modelcontextprotocol/protocolVersion": MODERN,
                           "io.modelcontextprotocol/clientCapabilities": {}}
    return {"jsonrpc": "2.0", "id": ident, "method": method, "params": params}


def task_arguments(inputs: dict | None = None) -> dict:
    return {"name": "workbench_task_run", "arguments": {
        "id": "error-budget", "version": 1,
        "input": inputs if inputs is not None else {"slo": 99.9, "bad_minutes": 20}}}


def encode(value: object) -> bytes:
    return json.dumps(value, ensure_ascii=False, allow_nan=False, separators=(",", ":")).encode() + b"\n"


def unique_object(pairs: list[tuple[str, object]]) -> dict:
    result = {}
    for key, value in pairs:
        assert key not in result, "duplicate key in server response"
        result[key] = value
    return result


class Client:
    """Own exactly one server process and bounded nonblocking parent pipe buffers."""
    def __init__(self, binary: Path, args=(), env: dict | None = None):
        self.process = subprocess.Popen([str(binary), *(args or ("mcp", "serve"))],
                                        stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        stderr=subprocess.PIPE, env=env, bufsize=0)
        self.input = self.process.stdin
        self.output = self.process.stdout
        self.diagnostic = self.process.stderr
        self.buffer = bytearray()
        self.diagnostics = bytearray()
        for stream in (self.input, self.output, self.diagnostic):
            os.set_blocking(stream.fileno(), False)

    def __enter__(self):
        return self

    def close_input(self) -> None:
        if not self.input.closed:
            self.input.close()

    def wait(self, timeout: float = 7) -> int:
        return self.process.wait(timeout=timeout)

    def __exit__(self, exc_type, exc_value, traceback):
        self.close_input()
        try:
            self.wait()
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=2)
            if exc_type is None:
                raise AssertionError("MCP process exceeded cleanup deadline") from None
        finally:
            self.output.close()
            self.diagnostic.close()

    def raw(self, data: bytes) -> None:
        deadline = time.monotonic() + 3
        view = memoryview(data)
        while view:
            remaining = deadline - time.monotonic()
            assert remaining > 0, "client input write deadline"
            _, ready, _ = select.select([], [self.input], [], remaining)
            assert ready, "MCP stopped reading input"
            try:
                written = os.write(self.input.fileno(), view)
            except BlockingIOError:
                continue
            view = view[written:]

    def send(self, value: dict) -> None:
        self.raw(encode(value))

    def read(self, timeout: float = 7) -> dict:
        deadline = time.monotonic() + timeout
        while b"\n" not in self.buffer:
            remaining = deadline - time.monotonic()
            assert remaining > 0, "MCP response deadline"
            ready, _, _ = select.select([self.output, self.diagnostic], [], [], remaining)
            assert ready, "MCP response deadline"
            if self.diagnostic in ready:
                data = os.read(self.diagnostic.fileno(), 4096)
                self.diagnostics.extend(data)
                assert len(self.diagnostics) <= 16384, "unbounded MCP diagnostics"
            if self.output in ready:
                data = os.read(self.output.fileno(), 65536)
                assert data, f"MCP stdout closed before a complete frame (exit={self.process.poll()})"
                self.buffer.extend(data)
                assert len(self.buffer) <= MAX_FRAME + 65536, "MCP response exceeded bound"
        frame, _, rest = self.buffer.partition(b"\n")
        self.buffer = bytearray(rest)
        assert len(frame) + 1 <= MAX_FRAME
        value = json.loads(frame, object_pairs_hook=unique_object,
                           parse_constant=lambda value: (_ for _ in ()).throw(ValueError(value)))
        assert isinstance(value, dict) and value.get("jsonrpc") == "2.0", "not a JSON-RPC envelope"
        assert ("result" in value) != ("error" in value), "ambiguous JSON-RPC response"
        return value

    def request(self, method: str, params: dict | None = None,
                ident: str | int = "request") -> dict:
        self.send(request(ident, method, params))
        response = self.read()
        assert type(response.get("id")) is type(ident) and response["id"] == ident
        return response


def successful(response: dict, modern: bool = True) -> dict:
    assert "error" not in response, response
    result = response["result"]
    if modern:
        assert result.get("resultType") == "complete", result
    else:
        assert "resultType" not in result, result
    return result


def error(response: dict, code: int | None = None) -> None:
    assert "error" in response and "result" not in response, response
    if code is not None:
        assert response["error"]["code"] == code, response


def local_references(schema: object, root: dict | None = None) -> None:
    """Resolve every advertised JSON Pointer locally, with no network schema retrieval."""
    if root is None:
        root = schema
    if isinstance(schema, dict):
        if "$ref" in schema:
            ref = schema["$ref"]
            assert ref.startswith("#/"), ref
            node = root
            for part in ref[2:].split("/"):
                node = node[part.replace("~1", "/").replace("~0", "~")]
        for child in schema.values():
            local_references(child, root)
    elif isinstance(schema, list):
        for child in schema:
            local_references(child, root)


def full_pipe() -> tuple[int, int]:
    reader, writer = os.pipe()
    os.set_blocking(writer, False)
    try:
        while True:
            os.write(writer, b"p" * 4096)
    except BlockingIOError:
        pass
    os.set_blocking(writer, True)
    return reader, writer


def check_fds(binary: Path) -> None:
    """Hold duplicates of child ends to observe shared-open-description flag changes."""
    reader, input_writer = os.pipe()
    output_reader, writer = os.pipe()
    before = [fcntl.fcntl(fd, fcntl.F_GETFL) for fd in (reader, writer)]
    process = subprocess.Popen([str(binary), "mcp", "serve"], stdin=reader,
                               stdout=writer, stderr=subprocess.PIPE)
    try:
        os.write(input_writer, encode(request(1, "ping")))
        ready, _, _ = select.select([output_reader], [], [], 3)
        assert ready, "pipe reopen readiness"
        response = os.read(output_reader, 65536)
        successful(json.loads(response))
        assert before == [fcntl.fcntl(fd, fcntl.F_GETFL) for fd in (reader, writer)], "inherited flags changed"
        os.close(input_writer)
        input_writer = -1
        assert process.wait(timeout=7) == 0
    finally:
        if process.poll() is None:
            process.kill()
            process.wait(timeout=2)
        process.stderr.close()
        for fd in (reader, input_writer, output_reader, writer):
            if fd >= 0:
                os.close(fd)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--no-schema", action="store_true", help="Disclose JSON Schema validation skipped")
    options = parser.parse_args()
    assert __debug__, "verification requires assertions enabled"
    binary = options.binary.resolve(strict=True)
    count = 0

    def passed(name: str) -> None:
        nonlocal count
        count += 1
        print(f"PASS {count}: {name}", flush=True)

    def validate(schema: dict, value: object) -> None:
        local_references(schema)
        if not options.no_schema:
            from jsonschema import Draft202012Validator, FormatChecker
            Draft202012Validator.check_schema(schema)
            Draft202012Validator(schema, format_checker=FormatChecker()).validate(value)

    with Client(binary) as client:
        assert successful(client.request("tools/list"))["tools"] == []
        for name in ("workbench_task_run", "workbench_command_run", "workbench_command_inspect",
                     "workbench_grafana_dashboard_get", "workbench_grafana_query"):
            error(client.request("tools/call", {"name": name, "arguments": {}}), -32602)
        passed("empty grants hide and deny every operation")
        discover = successful(client.request("server/discover"))
        assert set(discover["supportedVersions"]) == {MODERN, LEGACY}
        passed("discovery advertises exactly the two supported revisions")
        error(client.request("resources/read", {"uri": "file:///etc/passwd"}), -32601)
        error(client.request("sampling/createMessage", {}), -32601)
        passed("unimplemented file and model surfaces are absent")

    with Client(binary, ("mcp", "serve", "--allow", "task.run")) as client:
        tools = successful(client.request("tools/list"))["tools"]
        assert [tool["name"] for tool in tools] == ["workbench_task_run"]
        tool = tools[0]
        local_references(tool["inputSchema"])
        local_references(tool["outputSchema"])
        passed("granted numerical tool has self-contained schemas")
        with tempfile.TemporaryDirectory(prefix="workbench-mcp-contract-") as directory:
            input_path = Path(directory) / "input.json"
            for inputs in ({"slo": 99.9, "bad_minutes": 20},
                           {"slo": 99.9, "bad_minutes": 40.32},
                           {"slo": 99, "sli_long": 85.6, "sli_short": 85.6},
                           {"slo": 99.5, "bad_events": 10.5, "total_events": 10000.5}):
                arguments = task_arguments(inputs)
                validate(tool["inputSchema"], arguments["arguments"])
                result = successful(client.request("tools/call", arguments))
                assert result["isError"] is False, result
                receipt = result["structuredContent"]
                validate(tool["outputSchema"], receipt)
                assert receipt["execution"]["status"] == "succeeded"
                assert receipt["assessment"] == "not_assessed" and receipt["record_mode"] == "never"
                assert len(result["content"]) == 1 and len(result["content"][0]["text"]) < 512
                input_path.write_bytes(encode(inputs))
                cli = subprocess.run([str(binary), "--json", "task", "run", "error-budget", "--input", str(input_path)],
                                     capture_output=True, timeout=10, check=False)
                assert cli.returncode == 0, cli.stderr[:500]
                direct = json.loads(cli.stdout)
                for key in ("input", "calculation", "source", "task", "runtime"):
                    assert receipt["data"][key] == direct["data"][key], key
                passed("schema-valid numerical receipt and CLI parity " + str(inputs))
            arguments = task_arguments()
            arguments["arguments"]["limits"] = {"max_output_bytes": 1048577}
            result = successful(client.request("tools/call", arguments))
            assert result["isError"] is False
            input_path.write_bytes(encode(arguments["arguments"]["input"]))
            cli = subprocess.run([str(binary), "--json", "task", "run", "error-budget", "--input",
                                  str(input_path), "--max-output-bytes", "1048577"],
                                 capture_output=True, timeout=10, check=False)
            assert cli.returncode == 0
            assert result["structuredContent"]["effective_limits"] == json.loads(cli.stdout)["effective_limits"]
            passed("core-admitted output limit retains CLI effective-limit normalization")
        for patch in ({"cwd": "/"}, {"environment": {"PATH": "/tmp"}}, {"runtime": "/tmp/python"},
                      {"policy": "operator-local"}, {"id": "dashboard-hygiene"}, {"version": 2},
                      {"input": {"slo": True}}, {"input": {"slo": 99, "file": "/etc/passwd"}},
                      {"limits": {"concurrency": 2}}, {"limits": {"timeout_ms": 300001}},
                      {"limits": {"max_output_bytes": 2097153}}):
            arguments = task_arguments()
            arguments["arguments"].update(patch)
            error(client.request("tools/call", arguments), -32602)
        passed("tool normalization rejects authority overrides and invalid core controls")
        client.send(request("init", "initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                    "clientInfo": {"name": "independent-raw-client", "version": "1"}}, modern=False))
        assert successful(client.read(), modern=False)["protocolVersion"] == LEGACY
        client.send(request("early", "tools/list", modern=False))
        error(client.read())
        client.send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        client.send(request("legacy", "tools/call", task_arguments(), modern=False))
        legacy = successful(client.read(), modern=False)
        assert legacy["isError"] is False
        validate(tool["outputSchema"], legacy["structuredContent"])
        successful(client.request("ping"))
        client.send(request("again", "initialize", {"protocolVersion": LEGACY, "capabilities": {},
                    "clientInfo": {"name": "raw", "version": "1"}}, modern=False))
        error(client.read())
        passed("legacy fallback, initialized gate, receipt shape and modern coexistence")

    with Client(binary) as client:
        for params in ({}, {"_meta": {}}, {"_meta": {"io.modelcontextprotocol/protocolVersion": MODERN}},
                       {"_meta": {"io.modelcontextprotocol/clientCapabilities": {}}}):
            client.send(request("meta", "ping", params, modern=False))
            error(client.read(), -32602)
        unsupported = request("version", "ping")
        unsupported["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"] = "2099-01-01"
        client.send(unsupported)
        error(client.read(), -32022)
        passed("modern metadata is required on every request")
        for raw in (b'{"jsonrpc":"2.0","id":1,"id":2,"method":"ping"}\n',
                    b'{"jsonrpc":"2.0","id":1,"method":"ping","params":{"x":NaN}}\n',
                    b'[]\n', b'{"a":' + b'[' * 26 + b'0' + b']' * 26 + b'}\n'):
            client.raw(raw)
            error(client.read())
        for invalid in (None, 0.5, 9007199254740992, "x" * 129, "é" * 65):
            value = request("invalid", "ping")
            value["id"] = invalid
            client.send(value)
            error(client.read())
        successful(client.request("ping", ident=-9007199254740991))
        successful(client.request("ping", ident="é" * 64))
        passed("duplicate, nonfinite, deep, batch and invalid-ID envelopes are refused")
        for index in range(1100):
            successful(client.request("ping", ident="reused"))
        passed("completed IDs can be reused beyond 1024 requests")

    check_fds(binary)
    passed("child pipe reopen preserves inherited descriptor flags")
    for sig, expected in ((signal.SIGINT, 130), (signal.SIGTERM, 143)):
        with Client(binary) as client:
            successful(client.request("ping"))
            client.process.send_signal(sig)
            assert client.wait() == expected
        passed(f"idle {sig.name} exits {expected}")
    with Client(binary) as client:
        successful(client.request("ping"))
        client.output.close()
        client.send(request("closed", "ping"))
        assert client.wait() != 0
    passed("closed stdout ends the session")

    reader, writer = full_pipe()
    process = subprocess.Popen([str(binary), "mcp", "serve"], stdin=subprocess.PIPE,
                               stdout=writer, stderr=subprocess.PIPE)
    try:
        started = time.monotonic()
        process.stdin.write(encode(request("blocked", "ping")))
        process.stdin.flush()
        assert process.wait(timeout=7) != 0
        assert time.monotonic() - started < 7
        assert not fcntl.fcntl(writer, fcntl.F_GETFL) & os.O_NONBLOCK
    finally:
        if process.poll() is None:
            process.kill()
            process.wait(timeout=2)
        process.stdin.close()
        process.stderr.close()
        os.close(reader)
        os.close(writer)
    passed("blocked stdout has a fixed bounded deadline")

    reader, writer = full_pipe()
    try:
        process = subprocess.Popen([str(binary), "mcp", "serve", "--invalid"], stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=writer)
        try:
            output, _ = process.communicate(timeout=3)
            assert process.returncode != 0 and output == b""
            assert not fcntl.fcntl(writer, fcntl.F_GETFL) & os.O_NONBLOCK
        finally:
            if process.poll() is None:
                process.kill()
                process.communicate(timeout=2)
    finally:
        os.close(reader)
        os.close(writer)
    passed("blocked stderr cannot stall invalid startup")

    for args in (("--allow", "bogus"), ("--allow", "task.run", "--allow", "task.run"),
                 ("--allow", "process.exec"), ("--allow", "grafana.query")):
        process = subprocess.run([str(binary), "mcp", "serve", *args], input=b"", capture_output=True,
                                 timeout=3, check=False)
        assert process.returncode != 0 and process.stdout == b""
    passed("unknown, duplicate and unconfigured grants refuse startup")
    with tempfile.TemporaryFile() as stream:
        for stdin, stdout in ((stream, subprocess.PIPE), (subprocess.PIPE, stream)):
            process = subprocess.Popen([str(binary), "mcp", "serve"], stdin=stdin,
                                       stdout=stdout, stderr=subprocess.PIPE)
            try:
                output, diagnostic = process.communicate(timeout=3)
                assert process.returncode != 0 and not output
                assert stream.tell() == 0 and len(diagnostic) <= 512
            finally:
                if process.poll() is None:
                    process.kill()
                    process.communicate(timeout=2)
    passed("regular file transport is refused before effects")
    with Client(binary) as client:
        oversized = b'{"jsonrpc":"2.0","id":1,"method":"ping","params":{"padding":"' + b'x' * 131073
        try:
            client.raw(oversized)
        except BrokenPipeError:
            pass
        assert client.wait() != 0
        assert client.output.read() in (None, b""), "oversized input produced a response"
    passed("oversized unterminated input closes without dispatch")
    print(f"PASS: {count} independent MCP groups; schema validation {'SKIPPED' if options.no_schema else 'enabled'}.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
