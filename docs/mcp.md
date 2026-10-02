# Stdio MCP — next implementation boundary

This is the next CAP-06 increment after restricted-command acceptance. Execution status belongs
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

Command execution requires the accepted restricted profile; there is no operator-local fallback.
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
At implementation, inspect and lock that exact source with default features disabled and only
the server/stdio features needed. Inspect framing, cancellation and supported-version behavior;
do not infer application bounds from the SDK's presence. No dependency has been added for this
contract yet.

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
