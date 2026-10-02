# Command runner proof of concept

The owner authorized this PoC on 2026-10-02. It is the first working increment toward all 25
capabilities, not completion of P1 or acceptance for production. Current status and evidence belong
in [the roadmap](roadmap.md). The imported [contracts](sre-workbench/contracts.md) remain the common
target; this document resolves the subset needed to build and test now.

## Decisions for this increment

- Use Rust 1.98.0, already installed on the development host, edition 2024, and a locked dependency
  set. Start with a core library and CLI in one Cargo workspace, implemented by one owner.
- Keep `save` as the experimental CLI name. Naming/distribution checks do not block local builds.
- Prove Linux x86-64 process execution first. Windows x86-64 remains required for P1, with native
  process/job and quoting tests before support is advertised. Unsupported execution fails before
  spawning; portable discovery/help should still work.
- Implement operator-local `process.exec` and `command.inspect`, `call --request`, capability
  discovery, and an offline `doctor`. The same core owns normalization, bounds and result meaning.
- Use `record=never` only in this increment and report it. Reject `auto` and `always` before
  dispatch until persistence exists. Defer the SQLite-versus-files decision to the first actual
  evidence consumer; a process supervisor does not need a database.
- Generic execution is an explicit operator action with unclassified effects. It never establishes
  agent observation permission or service health. After dispatch, external effects remain unknown
  without an operation-specific reconciliation contract, even when child exit is zero.
- Select no live Grafana target or credentials. Build adapters against local fixtures after the
  supervisor; live target selection blocks only the live pilot. Cally is development assistance,
  not a product remote runner or a request to change its infrastructure.

## Public behavior

`save [--json] exec --cwd PATH [--timeout 15s] [--max-output-bytes N] -- PROGRAM ARG...`
executes literal arguments. Human cwd defaults to the invoking directory, then becomes an
absolute canonical path in the receipt. Structured requests require an absolute cwd. Bare programs
resolve from fixed system directories, never workspace/PATH overrides; explicit absolute paths
are permitted for the operator profile and canonicalized before dispatch. Shells and Windows
batch files require the separate named-task work and are rejected by this initial surface.
Executable identity is reported; canonicalization alone is not a protected installation claim.

Environment starts empty, with only documented fixed runtime essentials. Stdin is closed.
Timeout defaults to 30 seconds and cannot exceed 300 seconds. Captured stdout/stderr default to
1 MiB each; the host ceiling is 1 MiB per stream, even though the generic request schema admits
2 MiB for other operations. Continue draining after truncation. Final encoded JSON, including
escaping, must fit 2 MiB; trim text on character boundaries and disclose lost output. Loss of
output makes execution `partial`, not `succeeded` with a hidden caveat.

Use a monotonic deadline through child execution and pipe drainage. On timeout, cancellation or
remaining descendants after the leader exits, terminate the owned group, wait up to a two-second
grace, then force termination with a bounded final drain. Preserve the primary exit status and
signal. Check observed termination and report uncertainty when cleanup cannot be established.
Document that process groups do not contain a deliberately escaping untrusted program.

Exactly one terminal result follows the existing result schema. Text and JSON derive from the
same value. Invalid structured control fields, unsupported versions/targets/record modes and
malformed limits fail before dispatch. Request files are bounded to 64 KiB and depth 16.
CLI exits are 0 for success; 1 for failure/partial/timeout/unknown; 2 for usage/denial/unsupported;
130 for SIGINT; 143 for SIGTERM. Child status remains separate. No success infers healthy service.
Broken output pipes must not panic; cleanup is bounded even when result delivery fails.
Result delivery has its own 500 ms ceiling. A signal during timeout cleanup preserves that timeout
outcome while setting the invocation exit to 130/143. A signal during delivery also selects 130/143;
bytes already written cannot be revised. An interrupted or closed consumer may receive incomplete
JSON, so consumers must check transport completion and exit status. Never emit a second receipt or
repeat execution to repair interrupted delivery.

## Acceptance and progression

| Evidence | Required observable result | Related cases |
|---|---|---|
| Real command mission in AGENTS.md | Correct literal output, cwd and schema-valid receipt; text/JSON agreement | AC-01, AC-02 |
| Quotes, Unicode, spaces, shell metacharacters | Unchanged argv; no shell marker created | AC-02 |
| Missing executable, invalid cwd, malformed request, extra grant/role fields | Nonzero documented exit, safe error, no spawned marker | AC-04, AC-06 subset |
| Synthetic environment canary | Child receives only documented environment; canary absent | AC-09 subset |
| Noise on both pipes; control bytes and invalid UTF-8 | Bounded memory/output/encoded receipt; replacement and truncation disclosed | AC-05 |
| Slow/blocked process, descendants, parent exit and held-open pipes | Whole-operation deadline and bounded cleanup; no false completion | AC-05 |
| SIGINT/SIGTERM and closed result consumer | Cleanup and correct normalized exit, no traceback | AC-01, AC-05 |
| Discovery, inspect and doctor | Offline; available/unsupported distinctions; inspection causes no process effects | AC-01, AC-34 subset |
| Frozen candidate review and verification | Independent findings resolved; commands and exact source identity retained | Slice definition of done |

These cases do not close the full AC-03/04/06/09/34 platform, policy, secret-provider or install
requirements. Track remaining evidence explicitly. No production release is implied by PoC pass.

Next: native Windows supervisor and packaging checks; restrictive agent command forms and named
script bindings; native Grafana fixture adapter; stdio MCP parity; then an explicitly configured
live pilot. Task and Grafana adapters may proceed independently once core results/limits stabilize.
Evidence capture is introduced when the pilot needs it. Later investigations, jobs, extensions,
remote execution, schedules, controlled changes, UI and connectors retain their dependencies in
[the delivery plan](sre-workbench/delivery.md).
