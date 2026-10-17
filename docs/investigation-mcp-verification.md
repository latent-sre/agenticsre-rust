# MCP investigation verification — 2026-10-15

WORKBENCH-028 is accepted for the local Linux stdio scope in the
[MCP investigation contract](investigation-mcp.md). Five independently granted tools expose context,
saved evidence, inspection, comparison and reports through the existing bounded adapter and the
accepted core investigation grant.

The [retained packet](evidence/investigation-mcp-2026-10-15/summary.json) contains the discovered
schemas, 19 MCP result envelopes, ten CLI parity receipts, unchanged-store snapshots, test logs and
[independent review](evidence/investigation-mcp-2026-10-15/independent-review.json).
All **11 installed acceptance groups** passed with the ordinary binary copied into a disposable
installation directory. Both supported protocol revisions were exercised.

| Artifact | SHA-256 |
|---|---|
| Source manifest, 289 files | `aaf970270c0f2a3017b5c6e105e140d47cc05d2a68a11f9b975796207a22692a` |
| Ordinary CLI, 51,428,344 bytes | `0141935ea3105c2dd5591af968dfec283fb5852e45a49184edf642b5e2765c80` |

The manifest covers crates, canonical schemas, dependency manifests, context fixture, new contract
and acceptance tools. Their hashes and the installed binary remained unchanged throughout the final
mission. This acceptance prose, retained packet and roadmap updates were added afterward.

## Established behavior

| Boundary | Evidence |
|---|---|
| Discovery | Only five explicitly selected investigation tools appear. Paths are absent; input/output schemas resolve locally and fit the existing transport bound. Empty grants expose no tools. |
| Both revisions | Modern `2026-07-28` and legacy `2025-11-25` return all five canonical results. Independent raw-client checks compare each with installed CLI calls, normalizing only generated IDs and clocks. |
| Authority | Unknown/null path, root, target, record and role fields are refused, as are unsupported limits and direct calls to hidden tools. A mixed numerical-task/evidence-list session preserves each independent grant. |
| Historical outcomes | Wrong manifest pins and missing/corrupt bundles fail. Item limits produce partial results; differing windows remain incompatible. `isError` agrees with execution state. |
| Reports | Markdown and JSON stay inside structured results; JSON rendering matches the report object. Source-bound proposals remain explicitly pending. No report file is published or written by the operation. |
| Identity and storage | Context byte changes and a same-content store-root replacement are refused. Real-pipe integration also covers same-byte context inode replacement. All observations preserve retained store bytes/modes; corrupt evidence is not repaired. |
| Lifecycle | Cancellation suppresses the queued investigation result and leaves ping responsive. Existing MCP tests retain protocol, malformed-input, stream, signal and cleanup checks. Stdout contains only protocol frames. |
| Startup | Real-pipe tests cover paired flags, missing/invalid resources, duplicate/unknown grants, unsupported global controls and missing-store refusal without creation. Errors retain exit 2 and empty stdout. |

## Verification and review

Focused Rust checks passed **30 MCP unit tests** and **23 CLI tests**, with **one existing native
Cally test ignored**. The CLI run includes the new five investigation tests, ten context regressions
and eight existing MCP tests. Strict CLI all-target/all-feature clippy, workspace formatting,
ordinary build and specification checks passed. Installed receipts validate against both canonical
result schemas and the exact discovered output schema; successful/partial domain data also validates
against its canonical schema. Ten CLI parity receipts validate independently.

The existing WORKBENCH-027 manifest confirms **265 core, UI, frontend and canonical-schema files
unchanged**. Their accepted workspace/browser evidence was reused. No dependency, protocol-version,
core executor or browser change was needed.

The new context startup and tool-normalization tests failed before implementation, then passed.
Independent static review found no actionable issue in the new startup/grant, schema/admission and
dispatch paths. It records exact reviewed hashes and excludes the accepted backlog. Because this
worktree contains earlier uncommitted work, scope attribution used the contract and identified hunks
rather than a clean milestone-to-milestone Git diff. Executable evidence came from separate tests and
the root's installed raw-client mission.

Two verifier corrections preceded final acceptance: its JSON Pointer helper needed to support root
references and array indexes in the existing schema closure, and one legacy discovery assertion
needed its legacy flag. The failed attempt logs are retained and are not counted as passing runs.
Neither failure required a product change.

## Reproduction and limits

```bash
tools/test-sandbox.sh cargo build -p workbench-cli --locked --offline
tools/test-sandbox.sh .venv/bin/python -B tools/verify-investigation-mcp.py \
  target/debug/save --output target/investigation-mcp/fresh-acceptance
```

The output directory must be new. The verifier copies the binary and historical store, generates a
fresh synthetic context source, runs real subprocess pipes and checks source/binary identities.
All execution uses the existing private Bubblewrap namespaces with no host credentials or external
network. It does not register an agent host or configure a live source. Same-account processes are
not separate protected identities, and access-time changes are outside the retained-byte guarantee.
Actual installed-agent compatibility, MCP diagnostics/saved-query grants, native Windows, live
sources and publication retain their own acceptance work. Changes remain local and uncommitted.
