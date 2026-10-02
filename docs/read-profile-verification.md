# Restricted Linux command verification

This record covers the final R1b correction to the [restricted command contract](read-profile.md),
before GUI integration. Live work remains in [the roadmap](roadmap.md). Full acceptance remains
open: the final independent executable verdict and actual namespace-setup failure evidence are
not established by the results below.

## Frozen candidate and review

- Disposable commit: `a33679a1d8739e785f64923a98766551c5730d7c`.
- Tree: `821582f25094215564d7b7885c8aaff066fe404f`.
- 213-file archive SHA-256: `4b81b8c1b68ca10f3b57244c8952d65a5003bffca613540c86dd8f8d03741fb6`.
- Snapshot: `/tmp/agenticsre-read-profile-r1b-final-hs2hg5cr`.
- Independent static correctness and separate manual security review: **APPROVE**; no unresolved
  source findings. Dedicated-auditor capacity was unavailable; no scanner execution is claimed.

One formal correction batch resolved explicit/nested ripgrep `.git` access and replaced a weak
hostile-Git assertion with an observable existing Git `stripspace` filter. Grammar admission refuses
`.git` path components; traversal excludes nested metadata while preserving names such as `.github`.
The filter changes known whitespace, so the test detects execution without relying on prohibited
filesystem writes or an absent helper executable. Existing isolation controls remain required.

## Coordinator native evidence

`tools/cally-profile-check.sh` executed from that frozen snapshot exited 0:

- 81 ordinary Rust tests and all 17 explicit native proofs passed.
- 51 public profile results passed, then validated against that archive's exact schemas.
- 96 Grafana cases and the original literal-printf mission passed.
- Read-only root-owned source, runtime UID 1000 and writable build tmpfs were checked.
- Native binary SHA-256: `2ad13032f35c7739463147b524e476c438decfabd036754de49b729149fa2343`.

Evidence is retained under `/tmp/agenticsre-profile-cally.0Atw0Yi1`, including `cally.log`,
`source.sha256`, runtime provenance and `receipt-schema-verification.json`. This execution was
performed by the coordinator, not the independent verifier.

The nonshipping sensitivity control removes only the five-line Git config masking argument block:
commit `6707b4abc1cc5c8a0464868c00b819852313c140`, tree
`9efe3b509a7f5366c4b3b7f5597856af53f93c46`, archive SHA-256
`28d9a39730ba236b727172214203e643de3834b5b2709445197925ba4198112f`.
The existing hostile-Git test failed specifically at `clean-filter execution sentinel observed`,
with tool exit 0 and the transformed sentinel in stdout. This is a successful regression-sensitivity
check, not a product candidate. Its source change was never applied to the product checkout.

Control evidence is `/tmp/agenticsre-profile-cally.KGXkqrqg`; the independently reviewed external
controller SHA-256 is `a9aae10a593828846d526ee68f385ecf505b557dc61acf90d4767d40107c8fc2`.
It selects only the frozen source and existing test; container boundaries are unchanged.
Both runs removed their containers, derived images and staging. Exact-name readbacks are retained
in `/tmp/agenticsre-read-profile-r1b-cleanup-readback.json`.

## Remaining acceptance limits

The earlier independent existing-suite run on commit `7a907cf3f07ef02202af879b6f3f2fe4529066cc`
passed all executed checks but returned **INCONCLUSIVE** because several required failure paths
were missing. Its packet is `/tmp/agenticsre-read-profile-verify-rjilbkgh/packet.txt`, SHA-256
`5737dd213825fb5618bdcf73848931fa3990f0971ff7a21eabac3d335ddf0aba`.
R1 added missing-runtime, unsupported-layout and blocking read-only FIFO coverage, now passed by
the coordinator. An attempted verifier restart reached the agent-thread limit. No independent
R1b execution is claimed.

Actual namespace-setup-failure acceptance remains open. The standard Bubblewrap namespace-disable
control failed before the payload because the container's `/proc/sys` is read-only. Evidence is
`/tmp/agenticsre-namespace-prerequisite-_ae_47j8/result.json`. Positive execution and unrelated local
ownership refusal do not prove this failure path. The host/container boundary was not weakened.
An earlier custom-probe attempt was rejected by automatic tool filtering and was not retried.

These results establish neither Windows support, live service access nor production readiness.
Later GUI seams, browser transport and changed verification helpers require their own evidence.

## Subsequent integrated evidence

The [GUI acceptance record](ui-verification.md) adds independent execution on integrated commit
`8c44553a2f2f3a7dce8ab5df8d162740615c53e1`: all existing native profile proofs, all 51 exact
schema-valid public profile receipts and 96 Grafana cases passed. It includes the new trusted GUI
core seams and preserves the restricted implementation. Later GUI corrections do not change core,
CLI, policy, schemas or dependencies; final GUI native checks passed on its final R2 source.
This supplements the earlier coordinator-only profile evidence without inventing an independent
run of the profile-only R1b snapshot. The actual namespace-setup-failure gap remains open.
