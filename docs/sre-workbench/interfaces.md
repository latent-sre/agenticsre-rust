# CLI and agent interface specification

All commands in this document are proposed. No executable is delivered by this planning package.
The examples use synthetic targets and do not identify a real system or supply credentials.

## CLI grammar

Global options precede the command: save [--json | --events] [--config PATH] [--no-input]
[--record auto|always|never] COMMAND. --json and --events are mutually exclusive. Human-readable
text is the default. --help, --version and completion generation perform no network operations.

| Command family | Proposed shape | Capability |
|---|---|---|
| Discovery | capabilities list; capabilities describe OPERATION | CAP-05, CAP-17 |
| Common commands | exec --cwd PATH --timeout 15s -- PROGRAM ARG... | CAP-01 |
| PowerShell | powershell run TASK --input FILE | CAP-02 |
| Explicit shell | shell run TASK --input FILE | CAP-02 |
| Grafana | grafana dashboard get --target ALIAS --uid UID | CAP-03 |
| Grafana queries | grafana query --target ALIAS --datasource UID --kind prometheus|loki --from UTC --to UTC --expr TEXT | CAP-03 |
| Named tasks | task list; task describe ID; task run ID --input FILE | CAP-04 |
| Context | context resolve --service ID --env ID | CAP-07 |
| Diagnostics | diagnose --target ALIAS --pack ID --input FILE | CAP-08 |
| Collection | collect --service ID --env ID --pack ID --since 30m | CAP-09 |
| Comparison | compare BEFORE AFTER | CAP-10 |
| Saved queries | query list; query run ID --target ALIAS --input FILE --since 30m | CAP-11 |
| Runbooks | runbook show ID; runbook run ID --target ALIAS --input FILE | CAP-12 |
| Configuration | check KIND --file PATH [--fail-on-findings] | CAP-13 |
| Offline | evidence open BUNDLE; replay BUNDLE --check ID | CAP-14 |
| Reports | report RUN --format markdown|json --out PATH | CAP-15 |
| Toil | toil summary --from UTC --to UTC | CAP-16 |
| Extensions | extension list; extension inspect PACKAGE; extension install PACKAGE | CAP-17 |
| Jobs | job status ID; job watch ID; job cancel ID | CAP-18 |
| Remote | remote run --runner ID --request FILE | CAP-19 |
| Multiple targets | fleet collect --targets FILE --pack ID --max-targets N | CAP-20 |
| Schedules | schedule validate FILE; schedule create FILE; schedule pause ID | CAP-21 |
| Changes | change plan --request FILE; change apply --plan ID --approval-ref REF | CAP-22 |
| UI | ui serve --bind LOOPBACK | CAP-23 |
| Setup | doctor [--target ALIAS] [--online]; config validate --file PATH | CAP-25 |
| Agent server | mcp serve --transport stdio | CAP-06 |

The generic structured route, save call --request FILE, uses the same request schema and policy as
every named command. It cannot invoke an uninstalled or ungranted capability. Extension-provided
operations initially use this route and discovery; popular operations may later gain curated
shortcuts without a second execution path.

## Ordinary command behavior

Everything after -- is the requested program and its literal argument vector. The CLI shell has
already performed its own parsing; the product must not join the vector into a new shell string.
The product resolves a trusted executable path and validates the operation/arguments before spawn.
Wildcards, pipes, substitutions and redirects are not interpreted by the direct executor.
Rust's process API documents platform-specific exceptions for shell/batch programs [SRC-01](sources.md).

~~~text
save exec -- git status --short
save exec --cwd F:/repos/trading-app --timeout 15s -- git diff --stat
save --json exec --cwd F:/repos/trading-app -- rg --files
~~~

In human mode, omitted cwd means the invoking directory, resolved and displayed in the receipt.
In agent mode, cwd is explicit and checked against configured roots. Git configuration, pagers,
external diff tools, aliases, environment and flags can introduce effects; the read profile must
control the supported subset rather than trusting the executable name alone.

PowerShell cmdlets use reviewed task adapters with named parameters and a fixed script/entrypoint.
General free-form shell code is absent from the initial agent surface. Future operator shell
support is an explicit capability under an appropriate grant, not an automatic fallback for a
command that the direct runner rejected. A task-based pipeline names stages and checks every
stage outcome; a shell pipeline's final exit status cannot hide earlier failures.

## Human presentation

Show operation and resolved target before long work; emit diagnostic progress on stderr. A short
result states execution outcome, findings, coverage gaps and the run ID. Detailed output is
available without requiring a model. Preserve Unicode, honor NO_COLOR and non-TTY output, and
never use terminal styling inside machine output.

--json emits one terminal JSON object even on an operational failure. --events emits bounded
JSONL progress and one completed event. API credentials never belong in arguments. Unexpected
exceptions use a safe error code; --debug adds sanitized detail, not raw headers or environments.

Confirmation is permitted only for an interactive human operation that requires it; --no-input
and non-TTY stdin never answer a prompt automatically. Plan approval for live effects is bound
to a specific action and cannot be replaced by a blanket --yes flag.

## MCP adapter

Initial transport is local stdio. Stdout belongs exclusively to the protocol. The server exposes
a deliberately small approved tool set, with additional capability groups enabled by the host.
Discovery metadata is filtered to relevant permitted operations to avoid overwhelming model context.

| MCP tool | Core operation | Initial phase |
|---|---|---|
| workbench_capabilities_list | capability.list | P2 |
| workbench_capability_describe | capability.describe | P2 |
| workbench_command_run | process.exec | P2 |
| workbench_grafana_dashboard_get | grafana.dashboard.get | P2 |
| workbench_grafana_query | grafana.query | P2 |
| workbench_task_run | task.run | P2 |
| workbench_context_resolve | context.resolve | P3 |
| workbench_collect | evidence.collect | P3 |
| workbench_compare | evidence.compare | P3 |
| workbench_runbook_run | runbook.run | P4 |

Each tool has a JSON input schema derived from its core operation and an output schema for the
shared result. Tool-specific arguments normalize into the request envelope; effective identity
and policy are attached by the server host. Agent request fields cannot replace them.
Structured content and a concise textual explanation derive from the same result. Large
artifacts are exposed only by authorized run-scoped references, not arbitrary filesystem reads.

MCP tool annotations are hints, not permission enforcement [SRC-05](sources.md). The current
specification and the Rust SDK demonstrate a technical path, not compatibility with installed
Claude/Copilot versions. Negotiate supported protocol versions and prove tool discovery,
invocation, failure, cancellation and denied access on each accepted host.

An MCP adapter under the same OS account as an unrestricted agent does not isolate that account's
secrets. The deployment profile must disclose the boundary. Protected remote credentials require
the later broker/runner boundary and target-side permissions.

## Worked structured request

~~~json
{
  "spec_version": "0.1",
  "request_id": "example-command-001",
  "operation": "process.exec",
  "operation_version": 1,
  "target": {"kind": "local", "id": "workstation"},
  "inputs": {
    "program": "git",
    "args": ["status", "--short"],
    "cwd": "F:/repos/trading-app"
  },
  "limits": {"timeout_ms": 15000, "max_output_bytes": 1048576},
  "record": "auto"
}
~~~

The operation's input schema validates program/args/cwd beyond the shared envelope. The server
resolves git against its trusted executable configuration; the string is not an authority grant.
Canonical examples and expected validation outcomes live in [examples](examples/README.md).

## Complete investigation flow

1. Resolve the explicitly selected service/environment and inspect stale or missing mappings.
2. Retrieve the configured dashboard and run the selected bounded query.
3. Run a common local command and an offline named script within their separate grants.
4. Collect the results into a run bundle, retaining failures and coverage qualifications.
5. Compare with a compatible previous run, or report why comparison is invalid.
6. Prepare a report for the human's existing incident channel. Sending it is not implicit.

CLI and MCP acceptance exercise this same flow on fixtures and then on the selected live target.
Unsupported operations explain the missing path; agents must not manufacture shell substitutes
to bypass a denied operation. Browser image evidence remains distinct from dashboard JSON and
query data; future visual adapters require separate session/capture acceptance.
