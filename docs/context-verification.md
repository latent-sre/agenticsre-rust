# Offline context fixture acceptance

Independent verdict: **PASS** for the offline core/CLI CAP-07/AC-12 slice. Static R1 review approved
the bounded fixture resolver; Cally passed the workspace, existing native grant regression and public
context matrix. All 90 public receipts and two direct checkpoint receipts are schema-valid. This
does not establish actual team inventory, live catalog freshness, automatic target dispatch or a
context tool in the browser/MCP adapters.
The [unchanged independent packet](evidence/context-cally-verification-2026-10-03.txt) and
[92 exact receipt observations](evidence/context-receipts-2026-10-03.jsonl) are retained in Git.
Receipt bytes were copied directly from the validated native transcripts.

## Exact source and corrections

The approved source is frozen in `/tmp/agenticsre-context-r1-snapshot-yvmh2dtw`, commit
`e0d223c5ae6c2192417515d749f9ce912fe1dd1a`, tree
`b16a46821256077181c5566cbd814c065a697d6c`, parent
`afd2929d80e5fa176eb39f3be942a7197726a8ee`. The 315-file capture manifest is
`/tmp/agenticsre-context-r1-snapshot.json`, SHA-256
`fab13d9470f8d6c27a580d90fd7e41bc538fd5da68183e32036e2e6d7cde51e5`.
Every original source hash remained unchanged after execution.

R0 static review requested three P2 corrections, with zero independent P0/P1s. Serde allowed
positional arrays where object shapes were declared; the pinned time parser admitted lexical forms
outside the timestamp schema; fixed-date positive tests would eventually expire. R1 guards every
declared object position before decoding and checks lexical UTC timestamp syntax before parsing.
Positive tests generate review timestamps and validate observation-derived age before excluding
only those varying fields from parity. Source tuples and `X` timestamps reproduced incorrect exit-0
acceptance on R0, then refused after correction. Valid timestamp forms retain original text.

Independent R1 re-review APPROVE: all three findings resolved, zero new or unresolved findings.
Context correction rounds: one. The final ten focused CLI tests and all-target/all-feature Clippy,
formatting and diff checks passed locally. Root reran the three new rejection/valid-time regressions.
Earlier local workspace evidence is 79 passed with six existing ignored tests. Red/green/Clippy logs
are `/tmp/agenticsre-context-r1-{red,green,clippy}.log`.

## Native checks and verifier correction

The first exact-R1 Cally run completed 151 workspace tests with 19 explicitly ignored tests and
one separately selected native MCP grant test. Its ordinary build and first 12 public context
checks passed, including CLI/structured-call parity. It then exited 1 at a verifier expectation:
JSON-mode CLI parse errors return the existing canonical `invalid_usage` receipt, while the verifier
incorrectly expected empty stdout. The exit code was correctly 2. This was a test expectation
failure; product code remained unchanged.

A focused follow-up corrected only that expectation, rebuilt the ordinary CLI and ran the remaining
80 public checks. It carried the original workspace/native/first-12 evidence instead of repeating
those suites. Both ordinary builds produced the identical binary SHA-256
`22b8a16efbfce9fc03f1e6ed7ac0d64437757a38137124ba355f8bef36321153`.
The first failed record is retained and is not relabeled as a complete passing run.

The combined public matrix has 92 distinct checks. It covers exact source/review identity and
CLI/call parity; explicit environment and optional selectors; ambiguous/missing/stale/retired/future
and incomplete bindings; partial mappings and independent dependency directions; unknown, duplicate,
null, malformed, positional-array and timestamp inputs; source/depth/record/output bounds; FIFO,
device and every-component symbolic-link refusal; source/age startup pairing; JSON usage receipts;
UI/MCP startup rejection; and discovery without opening a configured FIFO. Malicious metadata stayed
data. Source/external canaries were unchanged, no command marker was created and the private
loopback witness observed zero metadata connections.

One verifier-only ordinary Cargo integration test in the execution copy uses the exact unchanged
`context.rs` with actual public request/result/control types. It injects a preset cancellation and
an elapsed deadline at the context read checkpoint, producing two canonical refusal receipts. This
is deterministic unit evidence for those checkpoints; public CLI observations supply the separate
end-to-end evidence. No product source, behavior stub or alternative normalizer was injected.

All 90 exact public receipts and two checkpoint receipts validate against the canonical result and
applicable context-data schemas. Source identities describe bytes actually read, review age does
not claim the latest live catalog revision, assessment remains `not_assessed`, and recording is
`never`. A selected binding never replaces the canonical local/workstation target or grants authority.

## Execution boundary and cleanup

The cached Rust base is
`docker.io/library/rust@sha256:620dbcd124499c59e2406d3741574b5c5838cf9eb9656f0c3a03948f79b02959`.
Both jobs used COPY-only derived images, read-only source/root, UID/GID 1000, private PID/network,
no host mounts or credentials, no-new-privileges and all capabilities dropped, with 2.5 CPUs,
4 GiB memory and 512 PIDs. The strict 8 GiB pre-staging floor passed after only completed builder
Cargo caches were cleaned; accepted archives, binaries and evidence were preserved.

The full job started before 00:39:55 UTC, before the session's full-job cutoff. The follow-up started
before 00:45:01 UTC under the explicitly scoped late-check contract: staging at most 120 seconds,
runtime at most 420 seconds and launch before 00:48 UTC. It finished successfully by 00:45:39 UTC,
within the session's executable cutoff. No extra workspace/native or browser/Grafana run occurred.

First image: `sha256:0d43b080deaeabe7d986f6e1b501934ce44e266d7dc68112cba092fb0d248861`;
follow-up image: `sha256:47dd61e1ada44d29d803791a56746f59f6e53d98f42c3ae5388c298b5bae1382`.
Evidence directories are `/tmp/agenticsre-context-cally-8stdgxse` and
`/tmp/agenticsre-context-cally-ycwbvgb2`. The combined packet is
`/tmp/agenticsre-context-cally-ycwbvgb2/VERIFICATION.md`, SHA-256
`d5cf0ebe34db98e721c6bec4080cabeb17abb0c729c9665f999686d333d2a28e`.
Its 35-artifact digest manifest has SHA-256
`36a2b5a92a8836e7c5322ec7f9b7bada8ebc8b7d123ddcb7676013d2c6919a98`.
The original failed harness packet remains sealed separately, SHA-256
`a2d5d21c659025701329d013785797e7174ddb9008a1a2050785a8ffd5a4df6a`.
Final readback found no context containers or owned drivers; both staging trees and scratch were
absent, product residue was empty and the cached base identity remained unchanged.

## Remaining scope

The tested runtime is the existing Debian 13 x86-64 GNU/Linux Cally boundary. Other platforms and
unavailable kernel/proc prerequisites need native evidence. Source files remain ordinary same-account
operator inputs, with observed byte identities rather than signatures or authoritative catalog access.
Actual team source/adapter selection is DEC-09; credentials, approvals, live mappings, catalog writes,
source-aware browser/MCP grants and downstream diagnostic dispatch remain separate work.
Public in-flight signal timing was not established by the deterministic checkpoint test.

The accepted Linux bundles in [packaging verification](packaging-verification.md) predate this context
source and retain their own identities. They do not contain the context capability. Existing native
restricted-profile namespace-refusal, Windows, release-clearance and five-task pilot gaps remain open.
