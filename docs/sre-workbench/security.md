# Security and authority specification

The product executes programs and contacts operational systems. Its authority model must be
explicit before live acceptance. This is a design threat model, not a security audit verdict.
Existing [Save Toolkit consumer boundaries](https://github.com/latent-sre/save-toolkit/blob/7d900fb999abee3b3712e1da881dcc7e80b1f6e7/AGENTS.md) and agent grants continue to apply.

## Assets and trust boundaries

Protected assets include credentials, private telemetry, source/configuration, operational targets,
execution identities, approval records, evidence integrity and the availability of checked systems.
Untrusted inputs include command args, repository files, scripts, task/extension manifests, service
records, API output, logs, dashboards, reports and agent-generated requests.

Trust boundaries are interface to core, core to child process, core to credential provider,
adapter to remote API, core to artifact store, and later controller to remote runner.
Crossing a boundary requires validation of the relevant identity, target, data and budget.

## Authority model

| Profile | Intended use | Enforced boundary required |
|---|---|---|
| Operator local | Human runs authorized local tasks | OS identity, explicit target/config and local policy; no sandbox claim |
| Agent observation | Agent invokes a limited read/check surface | Host-controlled tool grants, trusted operation policy, scoped target account and output controls |
| Protected remote | Agent/human requests work under a separate runner identity | Authenticated caller, server-side authorization, target-scoped credentials and execution isolation |
| Controlled change | Exact live effect requested under an accepted workflow | Bound approval, replay control, readback and recovery contract |

A mode string, CLI flag, MCP annotation, extension manifest or environment variable controlled by
the caller is not identity. A human and an agent under the same unrestricted OS account cannot be
separated by a cooperative CLI alone. Document this limitation and withhold protected-access claims
until a host/runner boundary is demonstrated.

Read effects are defined semantically. A Grafana query can use HTTP POST; a GET-like endpoint can
still be expensive or expose sensitive material. A command's flags, configuration, hooks and
environment can execute other code. The effective decision binds the resolved operation, inputs,
target and identity, not a friendly label such as read_only.

## Threats and required controls

| Threat | Required design control | Acceptance |
|---|---|---|
| Shell injection or argument confusion | Direct argv; special handling/rejection for shells/batch; tested PowerShell adapters | AC-02, AC-03 |
| PATH or installed-script substitution | Trusted resolved paths, package/digest binding, protected installation and race analysis | AC-04, AC-26 |
| Credential exposure in outputs | Scoped credential provider; minimal child environment; controlled result/error/event/artifact paths | AC-09, AC-28 |
| Wrong environment, org or data source | Explicit selectors, binding/readback and ambiguity rejection | AC-07, AC-12 |
| Request to arbitrary network target | Configured destination policy, validated redirects/proxies/TLS and bounded queries | AC-08, AC-14 |
| Runaway process, output or fan-out | Whole-operation deadline, process-tree control, byte/work/concurrency limits | AC-05, AC-06, AC-30 |
| Malicious plugin or workflow | Reviewed immutable installation, schema validation, restricted grants, no self-approval | AC-26, AC-16 |
| Prompt injection in telemetry | Data remains untrusted; no embedded instruction changes policy or triggers a next action | AC-28 |
| Evidence tampering or path escape | Normalized contained paths, links/reparse checks, size/digest verification and provenance | AC-11, AC-18 |
| Double execution after lost response | No automatic replay of uncertain effects; reconcile target state or use proven idempotency | AC-24, AC-25 |
| Misleading health/result claims | Separate execution, findings, coverage and effect outcome | AC-10, AC-13 |
| Unauthorized remote caller | Authenticated transport and independent target-side checks; no token pass-through by assumption | AC-23 |

## Credentials and sensitive data

Configuration stores references and approved destinations, never credential values. The human or
protected runtime configures providers. Agent prompts, command arguments, saved requests and
repository files must not carry secrets. Reuse supported authenticated paths; browser SSO does
not imply an API token and the product must not extract browser cookies.

Only the adapter needing a credential receives it. Generic commands and offline scripts inherit
an allowlisted environment, not the parent process's complete environment. Output masking covers
known configured secrets and tested encodings; it cannot guarantee removal of arbitrary sensitive
telemetry or all derived secret forms. Streaming boundaries must handle a secret split across
chunks or suppress that unsafe stream. Raw diagnostic output is never a debug escape hatch.

File reads, environment access and network access outside the tool remain the host's responsibility.
Downstream accounts should be limited to the selected organization/resources. Enterprise-specific
Grafana permission APIs cannot be assumed available on every edition.

## Local files and external extensions

Policy/config loading never executes project code. Working directories and artifact paths are
canonicalized and checked against configured roots; symlink/reparse-point changes require
platform-aware handling. Digest verification alone does not prevent replacement between checking
and executing; installation permissions, open-handle strategies where possible, and residual
race limits must be reviewed before calling a boundary protected.

External extensions start with the least needed environment and file/network access. A subprocess
boundary is not sufficient isolation for an untrusted extension. Untrusted packages are inspected
without execution and require a separately established sandbox before testing. The distribution
phase defines signing, provenance, revocation and offline installation behavior.

## Controlled changes

CAP-22 is a later capability. Plan creation performs read-only preparation and produces a
versioned immutable plan with target identities, exact operations/inputs, implementation digest,
observed preconditions, expected effects, expiry, verification and recovery steps.

Approval is obtained through a trusted authority and binds that plan's digest, requester/executor,
expiry and unique execution token. The apply operation rechecks preconditions and rejects drift,
expired approval or replay. It records dispatch before the effect and result/readback afterward.
A crash in between leaves unknown. Reconciliation must occur before another attempt.

Rollback is operation-specific and may be unavailable; the plan states recovery options honestly.
Approval to apply does not automatically authorize a compensating change outside its bound scope.
The existing invoked observability-engineer Grafana exception remains its own complete rule;
this new product does not broaden it.

Save Toolkit's [EFFECT-001](https://github.com/latent-sre/save-toolkit/blob/7d900fb999abee3b3712e1da881dcc7e80b1f6e7/docs/fleet-roadmap.md#effect-001--effect-bound-execution-broker) remains
deferred until a named authorized consumer and separate execution identity exist. Including the
future capability in this plan does not satisfy those conditions.

## Privacy and distribution

Default evidence is private to its configured owner. Exports use explicit selection, classification,
redaction review and a named destination. Reports preserve omitted-data notices. No automatic Slack,
email, issue creation or external upload follows from collecting data.

Releases publish checksums, provenance and a software bill of materials; signing mechanism and
license are selected before distribution. Updates never silently change policy, enable an
extension, migrate secrets or widen a remote grant. A kill switch disables the product's own
dispatch; it cannot revoke actions already sent to an external system.
