# Stdio MCP — next implementation boundary

This is the next CAP-06 increment after the local GUI PoC. Execution status belongs
in [the roadmap](roadmap.md). It is a contract for local fixture work, not an installed host
integration or a grant to access production systems.

## Mission

A controlled MCP client discovers only its launcher-granted operations, runs a permitted command,
error-budget calculation and configured Grafana fixture observation, and receives the same core
results as the CLI. Denied tools, unsupported versions, malformed inputs and cancellation retain
their actual meanings. Protocol success never establishes infrastructure health.

Use the [existing tool mapping](sre-workbench/interfaces.md#mcp-adapter). Add a matching
`workbench_command_inspect` tool for advisory inspection. The server normalizes typed arguments
into the current core request and calls the same dispatcher; do not spawn a second `save` process
and parse terminal output as the implementation.

## Authority and scope

The trusted launcher explicitly supplies a bounded MCP operation allowlist and, when needed,
the read-policy and Grafana configuration paths. Unknown operations/grants fail closed. Wire
arguments and client identity/capability metadata cannot select policies, roles, executables,
credentials, runtimes or permission overrides. Discovery lists only permitted operations.

Command execution requires the same mandatory restricted profile exercised by the GUI; there is
no operator-local fallback. Its actual namespace-setup-refusal coverage gap remains explicit and
is not closed by MCP protocol tests.
The initial task grant permits only `error-budget`, whose inputs are numerical. The
dashboard-hygiene file task remains unavailable through MCP until a separate descriptor-bound
file-access contract exists. Existing CLI task behavior remains available to operators.
Grafana calls are limited to explicitly granted configured target IDs and operations, retaining
the credential provider's independent origin/org/operation binding and all existing limits.

Use local stdio only. Do not add an HTTP listener, host registration, sampling, elicitation,
arbitrary resource/file reads, task installation, evidence persistence or a live model campaign.
A process under the same OS account does not establish a separate protected identity. Host
installation and actual Claude/Copilot compatibility need their own later acceptance.

## Protocol and dependency decisions

The current specification is [2026-07-28](https://modelcontextprotocol.io/specification/2026-07-28).
It uses per-request version/capability metadata and `server/discover`; older handshake behavior
is a distinct [compatibility path](https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning).
Declare and test every supported revision. Do not label a legacy-only implementation as current
protocol support or advertise an untested SDK default version set.

The official Rust SDK's published candidate is
[`rmcp` 3.5.0](https://crates.io/crates/rmcp/3.5.0), verified in registry metadata on 2026-10-02:
Apache-2.0, minimum Rust 1.88, archive checksum
`fae7019994ae0fe4ada40b732f798f3ff26f0f04facb1477f1bf37eb4f18a2d3`.
Use its protocol model types with default features disabled. An application-owned Linux stdio
loop owns framing, admission, cancellation, output and cleanup. Source inspection found unbounded
line reads and detached request/notification/send tasks in the SDK service path; wrapping that
path would introduce another lifetime model. The owned loop avoids those tasks while retaining
the pinned SDK wire models. No arbitrary session-message/lifetime ceiling substitutes for bounded
live bookkeeping. Execution and acceptance status remain in the roadmap.

Follow the negotiated revision's
[tool-result contract](https://modelcontextprotocol.io/specification/2026-07-28/server/tools).
`structuredContent` is the complete unchanged core result and matches its published output schema.
A short text summary comes from that result; do not duplicate a multi-megabyte receipt in text.
Operational failure, denial, unsupported execution or incomplete capture sets `isError`.
A completed calculation with an exhausted budget or page finding does not. Invalid routing and
malformed protocol envelopes use the protocol's own errors.

## Bounds and lifecycle

Stdout carries protocol frames only; diagnostics go to bounded stderr without request bodies or
credentials. Enforce limits before unbounded parsing/allocation: 128 KiB per inbound frame,
depth 24 and the existing tighter core-input bounds. Reject duplicate JSON keys before a typed
decoder can discard them. Outbound frames are bounded to 3 MiB including the protocol envelope;
core receipts retain their 2 MiB limit. Malformed/oversized streams cannot start operations.

Allow one active execution per server in this first slice; a concurrent execution receives an
explicit busy error rather than entering an unbounded queue. Keep discovery and cancellation
responsive while synchronous core work runs in a bounded worker. Propagate cancellation,
disconnect and SIGINT/SIGTERM to the core control and wait for bounded cleanup. A dropped async
future alone does not stop a child process. Blocked/closed stdout must not leave execution alive.
No protocol or transport failure automatically retries an operation.

## Concrete first adapter

`save [--read-policy POLICY] [--config CONFIG] mcp serve --transport stdio`
accepts repeated `--allow OPERATION`, `--root ROOT_ID` and `--target TARGET_ID` startup grants.
Only `process.exec`, `command.inspect`, `task.run`, `grafana.dashboard.get` and `grafana.query`
are initially executable through this adapter. Empty grants produce an empty tool list. Unknown
or duplicate grants and missing prerequisites fail startup; wire metadata never widens them.
Command grants require explicitly selected policy root IDs; Grafana grants require explicitly
selected configuration target IDs. The five wire tools are:

| Tool | Arguments before core normalization |
|---|---|
| `workbench_command_run` | `root_id`, `program` (`git` or `rg`), literal `args`, optional `limits` |
| `workbench_command_inspect` | Same command shape; inspection does not execute the tool |
| `workbench_task_run` | `id=error-budget`, `version=1`, numerical `input`, optional `limits` |
| `workbench_grafana_dashboard_get` | `target_id`, `uid`, optional `limits` |
| `workbench_grafana_query` | `target_id`, `datasource`, `kind`, `from`, `to`, `expr`, optional `limits` |

Tool objects reject unknown fields. Limits use existing core names, defaults and operation-specific
ceilings; normalized requests must still fit 64 KiB/depth 16. Schemas embed the authoritative core
input/result definitions with local references, never network resolution. Tool discovery itself
uses `server/discover` and `tools/list`; the broader capability-list/describe tool mapping remains
planned until the core has effective-grant metadata semantics. Do not fabricate a modified core
receipt by filtering an unrestricted `capability.list` result in this adapter.

Startup pins the read-policy digest, Grafana configuration digest and configured custom-CA bytes.
The core checks identities at their actual loads, before provider lookup or HTTP. Its new trusted
entrypoint denies command or Grafana requests missing the corresponding pinned identity. Existing
operator/GUI wrappers retain their accepted behavior. Discovery never reads credential values.

Support exactly modern `2026-07-28` and legacy `2025-11-25`, tested separately. Every modern
request, including `ping`, supplies protocol version and client capabilities in `params._meta`;
client information is optional and untrusted. `server/discover` is optional before a modern call.
Modern successful replies include `resultType=complete`; legacy replies retain the legacy shape.
Legacy initialization is explicit: one initialize reply, then `notifications/initialized` before
ordinary legacy calls. Unsupported initialize revisions receive the supported legacy alternative;
duplicate initialization is refused. Explicit modern metadata is evaluated independently even
after legacy initialization. See [versioning](https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning)
and the [base protocol](https://modelcontextprotocol.io/specification/2026-07-28/basic).

One poll-loop owner tracks at most eight live request/output slots and 6 MiB of serialized
output/reservations. Reserve a complete 3 MiB response slot before dispatching the single owned
core worker; never queue executions. A concurrent execution returns application error `1001`
(`workbench_busy`). Output capacity exhaustion closes admission and starts cleanup. Keep IDs until
their response is fully written or cancellation cleanup finishes, then forget them. Numeric and
string IDs are distinct; a live-ID collision closes the stream rather than producing ambiguous
responses. Completed IDs may be reused. Request IDs are strings up to 128 UTF-8 bytes or integers
within the interoperable signed safe-integer range; reject null/fractional/oversized IDs.

Matching cancellation suppresses queued output before signalling the owned `RunControl` and
retains the worker slot until cleanup finishes. No cancellation acknowledgement or final canceled
receipt is sent. Cancellation during a partially written response closes the transport because
that frame cannot be retracted or followed safely by another frame. Ignore unknown/completed or
malformed cancellation notifications, and do not cancel legacy initialization. These wire rules
follow the [stdio binding](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/stdio)
and [cancellation](https://modelcontextprotocol.io/specification/2026-07-28/basic/patterns/cancellation).

Each queued output frame has a five-second deadline from enqueue, unaffected by trickle progress.
Serialize through a capped writer before sending, rather than allocating an unbounded buffer.
Use fixed-size I/O work per loop iteration and a maximum 25 ms poll wait so discovery, signals,
worker completion and cancellation remain responsive. EOF, signal, stdout failure/saturation and
worker panic close admission and signal the worker; allow five seconds for owned cleanup. An
unconfirmed cleanup exits unsuccessfully and never reopens admission or fabricates a receipt.

This initial Linux transport requires stdin/stdout pipes or FIFOs. Validate access/type, reopen
fixed `/proc/self/fd/0` and `/proc/self/fd/1` as independent nonblocking descriptors and verify
their identities against the originals; never change inherited file-status flags. Refuse sockets,
terminals and regular files before work. Ordinary subprocess MCP clients use pipes; actual host
compatibility is still separately tested. Stderr uses bounded best-effort fixed metadata through
an independent nonblocking pipe or verified null device; unsupported sinks disable diagnostics.
No request bodies, cancellation reasons, child output, credentials or panic payloads enter logs.

## Acceptance

Freeze the complete adapter and core bytes for independent correctness/security review and
execution. Use a raw controlled stdio client independent of the server's decoding path, plus a
pinned SDK-client compatibility check where useful. Exercise supported-version negotiation,
empty/granted discovery, direct calls to hidden/ungranted tools, exact output schemas, CLI/core
parity, malicious output treated as data, duplicate/oversized/deep input, unknown fields, active
request-ID conflicts, busy behavior, cancellation, disconnect, blocked output and process cleanup.
Prove an unconfigured command grant never falls back to unrestricted execution. Keep current CLI,
task, Grafana and restricted-profile regressions. Test fixtures alone do not establish installed
host behavior, protected remote credentials or live target compatibility.
