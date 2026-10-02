# Shared operation contracts

This is the proposed normative contract for the product. Draft version 0.1 is not a released API.
Machine schemas constrain structure; the semantic invariants below require implementation tests.
The [schema index](schemas/README.md) distinguishes schema checks from runtime guarantees.

## Operation identity and discovery

An operation has a stable dot-separated ID, an integer contract major, input/output schema
references, effect classification, execution mode, supported platforms, required dependencies and
stability level. IDs are never reused for a different meaning. Examples include process.exec,
grafana.dashboard.get, grafana.query and evidence.compare.

Discovery returns installed capabilities and their availability for the effective caller:
available, denied, missing_dependency, unconfigured, unsupported_platform or incompatible.
An unavailable entry explains the category without exposing secret or protected configuration.
Tool descriptions and extension manifests are data. Only trusted runtime configuration supplies
grants. Unknown permissions/effects mean unavailable, never a permissive default.

## Request

| Field | Meaning |
|---|---|
| spec_version | Envelope version; initially 0.1 |
| request_id | Caller correlation ID, unique within its authenticated context; not an authorization token |
| operation | Stable operation ID |
| operation_version | Supported major version |
| target | Explicit local/connection/service target selector |
| inputs | Object validated against the selected operation input schema |
| limits | Requested timeout, output and work ceilings; effective limits can only be tighter |
| record | auto, always or never for optional local evidence persistence |

The wire request deliberately has no credential values, executable authorization, caller role,
effective grants or permission-override fields. Those arrive through the trusted execution context.
A later change operation can accept a plan/approval reference within its own inputs, but must
resolve and validate it through an independent trusted authority.

Agents supply absolute working directories for process requests. Human CLI convenience may resolve
a relative directory against the invoking directory before creating the normalized request. Target
resolution returns the selected origin/org/resource identities and configuration revision; a missing,
ambiguous or stale required mapping prevents dispatch. No automatic environment switch is allowed.

## Results and meaning

Every terminal result identifies the run, operation/version, effective target, timings, execution
status, optional target assessment, coverage, output, errors and artifact references.

| Axis | Values and interpretation |
|---|---|
| Execution status | succeeded, failed, partial, denied, unsupported, cancelled, timed_out, unknown |
| Target assessment | not_assessed, healthy, degraded, unhealthy, unknown; only a defined check may assess |
| Coverage | complete, partial, not_established, not_applicable, measured against an explicit scope |
| Evidence state | verified, sourced, unverified; confidence does not remove untrusted content |
| Effect outcome | not_attempted, not_applicable, succeeded, failed, unknown |

Execution succeeded means the operation met its own completion contract. A check may succeed and
find an unhealthy service. A query may succeed with coverage not_established. A process exit of
zero does not establish target health. A transport failure after a possible live write leaves
effect outcome unknown even if the execution status is timed_out or cancelled.

The result contains sanitized output and a separate structured data object validated by the
operation's schema. Generic command stdout remains text; no adapter silently promotes it to
structured facts. Error records use stable codes and static safe explanations, optionally with
an actionable next step. Native exception text, server error bodies and arbitrary stderr require
the same output policy as successful data.

Artifacts carry run-relative path, media type, byte size, SHA-256 digest and classification.
Paths must be normalized, contained, and free of traversal or link escapes. Hashes detect byte
changes; they do not establish provenance or authenticity. Payload content remains untrusted.

## Exit status mapping

The product uses normalized exit codes; the original child exit status is preserved in the result.
There is no implicit exit-code passthrough.

| CLI exit | Result |
|---|---|
| 0 | Execution succeeded, including a completed check with negative findings |
| 1 | Failed, partial, timed_out, unknown, or cancellation without an OS signal |
| 2 | Invalid usage, denied request, unsupported operation/platform or incompatible contract |
| 130 | SIGINT or mapped console Ctrl-C interrupted this invocation |
| 143 | SIGTERM on platforms where supported |

A check-specific --fail-on-findings option changes the CLI exit to 1 when its documented finding
threshold is met; the execution status still records that the check completed. Result examples and
help must show this distinction. MCP isError is true for operational failure/denial/unsupported
outcomes, not merely for a completed check finding a problem. Malformed protocol envelopes use
protocol errors according to the negotiated MCP version.

The installed Python hook's 42/43/44 protocol is separate. The product must not silently translate
these into generic command success or replace existing hook wiring.

## Execution lifecycle

Internal states are received, validated, authorized, running, finalizing and terminal. Rejections
before running use effect outcome not_attempted. Only the supervisor finalizes a terminal record.
Progress events do not imply success. A process that exits without a valid adapter result fails.

Cancellation stops admission of new work, signals owned children, drains bounded output and
records unfinished effects. Proposed grace period is two seconds before forced termination.
The executor must verify whether its owned process tree actually stopped. A failed kill or escaped
child is reported and blocks claims of complete cancellation. Process groups on POSIX and job
objects on Windows are implementation candidates, with platform conformance tests [SRC-04](sources.md).

Async jobs later add accepted/queued handles; an accepted job is not a terminal operation success.
Jobs retain owner, execution location, submitted request and status revision. Restart recovery
reconciles running/uncertain work rather than blindly dispatching it again.

## Limits

These are proposed starting defaults. Operation and host policy may lower them; callers cannot
raise a host maximum by passing flags.

| Limit | Proposed default | Initial hard ceiling or rule |
|---|---|---|
| Request envelope | 64 KiB, depth 16 | Reject over limit before dispatch |
| Foreground process | 30 seconds | 300 seconds; longer work uses an explicit job contract |
| Grafana request | 20 seconds each | 60-second whole-operation deadline |
| Captured stdout and stderr | 1 MiB each | Continue bounded draining; report truncation; no unbounded buffering |
| Structured response | 2 MiB | Reject oversized structured payload; artifact route for designed larger data |
| Grafana range | Up to 24 hours | 1,000 requested points or 500 requested Loki lines; coverage still unproven |
| Diagnostic pack | 10 checks, concurrency 2 | 60-second pack deadline for the initial built-in pack |
| Retries | None | At most one explicitly safe read retry within the original deadline |
| Agent interactive stdin | Closed | No password prompt or implicit confirmation |

Lossless evidence collection fails or returns partial when a bound is hit; it cannot claim complete
coverage. Draining discarded bytes prevents a child blocking on a full pipe but still consumes its
time budget. A closed downstream pipe stops streaming without a traceback and initiates owned-child
cancellation. Internal diagnostics never corrupt JSON output.

The encoded response ceiling applies after JSON escaping and envelope overhead, even when a raw
stream has not reached its own cap. Reserve space for status/errors and reduce captured text with
explicit truncation metadata; never cut a serialized JSON object. Operation-specific structured
data that cannot be bounded safely fails or uses its designed artifact representation.

## Configuration

Ordinary options use explicit flags, then approved environment overrides, then an explicitly
selected user config file, then built-in defaults. Security policy has its own precedence:
administrator/launcher grant intersected with operation and target restrictions. Workspace files
cannot override it. No automatic discovery or execution of configuration from the working tree.

Connection records contain alias, HTTPS origin, expected organization, adapter/API profile,
credential reference and optional resource mappings. Secrets are resolved only by the selected
protected provider. A credential reference is not proof of isolation. Arbitrary executable
credential hooks are excluded from the first provider contract.

The provider independently binds each credential reference to allowed origins, organizations and
operations. Caller-editable connection data cannot redirect that credential to another host.

Human-only local profiles may permit broader commands; they do not prove a different OS principal.
Where a human and agent share an OS identity, the host must supply any enforceable distinction.
No caller-controlled --human, --agent or --allow flag widens permission.

## Time, events and recording

Use UTC wall timestamps for correlation, monotonic elapsed time for local duration, and per-run
sequence numbers for event order. Cross-host timestamps require recorded clock uncertainty;
timestamp proximity alone does not prove causality.

Events contain envelope version, run ID, sequence, observed_at, type and data. Types initially
include started, progress, output, warning and completed. The completed event points to the terminal
result. Sequence gaps and lost events are disclosed. Stdout is JSONL only in event mode; progress
otherwise goes to stderr. Output events follow the same redaction and bounds as final results.

Recording defaults to auto: bounded metadata for ordinary operations and selected artifacts for
collection/report operations. Operators can choose never when no mandatory audit is required.
The effective mode and omissions are reported. No raw terminal transcript, environment dump or
credential-bearing request header is stored by default.

## Task, query and workflow definitions

A named task contains ID/version, fixed installed entrypoint, executable digest, interpreter
requirement, input/output schema, fixed argument mapping, allowed environment keys, effects, limits
and supported platforms. Inputs are values, never interpolated shell source. Entry points are bound
to the installed package, not a same-named workspace script.

A saved query contains ID/version, backend dialect, target/data-source selector, text/template,
typed parameters, range/series limits, owner and semantic description. Parameter substitution uses
a dialect-aware implementation with test vectors. Never concatenate arbitrary strings into query
syntax or claim SQL is read-only because it begins with SELECT.

A diagnostic pack contains ID/version, description, typed inputs and a bounded acyclic list of
steps. Each step selects a capability/version and arguments. A guided runbook adds explicit
prerequisites, human checkpoints and documented branches over typed outputs. The first version
permits scalar equality/existence conditions only; it has no eval, templated shell, loops or hidden
recursive task calls. Step limits and parent deadlines include descendants. A proposed
[workflow schema](schemas/workflow.schema.json) captures the shared structural portion.

## Compatibility and evolution

Envelope and operation versions evolve independently. During draft, changes update all examples
and the design revision. After release, incompatible field/status/semantic changes require a new
major. Consumers reject unsupported majors before execution. Optional additive response fields may
be ignored by tolerant readers; new enum values and request-control semantics require an explicit
compatibility review. Strict authored schemas describe one exact revision.

Namespaced extensions may carry informational data, never alternate authorization controls.
Deprecation gives a replacement, migration example and supported overlap period selected at release.
Rollback must account for stored-data schema: keep backward-readable formats or use a reversible
migration with backup; a binary downgrade alone is not a storage rollback.
