# PR #3 review finding corrections — 2026-10-17

All seven unresolved P2 reports from PR #3 reproduced against the shared implementation in
`codex/offline-evidence`. The fixes are in `1fae8fd`; the backport to PR #3's
`codex/service-diagnostics` branch is `f12e4c9`. The original review was against `9016dcb`.
No live service, credential, or retained operator store was changed. Checks use private fixtures
inside the repository's existing Bubblewrap boundary.

| Review finding | Correction and regression evidence |
|---|---|
| [Pre-intent directories](https://github.com/latent-sre/agenticsre-rust/pull/3#discussion_r4289094993) | Explicit recovery removes only recognized initial-construction leftovers, reports deleted IDs, and remains restartable after sync failure or cancellation. Unknown formats and unsafe/noninitial layouts are preserved. `pre_intent_failures_can_be_explicitly_recovered_after_restart`, `pre_intent_cleanup_sync_failures_remain_restartable`, and public recovery/cancellation tests cover the boundary. |
| [Absolute evidence-store path](https://github.com/latent-sre/agenticsre-rust/pull/3#discussion_r4289095003) | Store admission rejects nonabsolute directories before filesystem access. `relative_store_options_are_rejected_before_access` verifies refusal without effects. |
| [Unknown journal format](https://github.com/latent-sre/agenticsre-rust/pull/3#discussion_r4289095013) | Every committed journal's identity, revision, and format are validated during scans. `foreign_unknown_format_blocks_open_create_and_replace_without_effects` proves a newer unrelated journal blocks open/create/replace while preserving all bytes. |
| [Completed current step](https://github.com/latent-sre/agenticsre-rust/pull/3#discussion_r4289095021) | The current cursor requires pending state except its explicit waiting checkpoint. `current_step_must_be_pending_unless_waiting_or_terminal` and the CLI `rewound_cursor_cannot_replay_a_completed_observation` reject corrupted cursors without replacing retained observations. |
| [Invocation deadline](https://github.com/latent-sre/agenticsre-rust/pull/3#discussion_r4303845172) | Opening, scanning, status, and new-state admission use the work deadline. Only persistence of observed work can use the bounded cleanup allowance; cancellation narrows a ceiling that later publications cannot renew. `status_scan_uses_work_deadline_without_cleanup_allowance` reproduced a 1 ms request returning success after 20 ms before the fix, and now requires timeout. |
| [Pinned receipt identity](https://github.com/latent-sre/agenticsre-rust/pull/3#discussion_r4303845178) | Retained show receipts bind bundle ID/digest, while comparison receipts bind both references and normalization. Legitimate early failure receipts may omit observations. Unit regressions and CLI `foreign_valid_receipts_cannot_drive_checkpoint_resume` reject valid receipts substituted from another workflow. |
| [Unreferenced sealed artifacts](https://github.com/latent-sre/agenticsre-rust/pull/3#discussion_r4303845184) | Sealed readers and bulk management reject extra non-temporary artifacts. Explicit deletion requires validated manifest/domain/receipt content, digest pins, and protection checks. `sealed_unreferenced_artifacts_are_refused_by_reads_and_bulk_management`, `explicit_deletion_still_refuses_corrupt_or_protected_extra_artifacts`, and public pinned-deletion tests cover this. |

## Evidence and integration

The [retained packet](evidence/pr3-findings-2026-10-17/) contains the failing baseline checks,
focused verification, and integrated logs. `runbooks-evidence.txt` is explicitly labeled as
excerpts transcribed from tool output, not a complete captured shell log. Other `.log` files
are captured command output. The baseline evidence-store suite had four new failing regressions;
the journal-format regression also failed before its fix. All three new cursor/binding tests and
the deadline regression failed before the corresponding runtime fixes.

Independent cross-review found two related corrections: invalid temporary intent metadata must
produce a per-run corrupt listing instead of hiding valid neighbors, and early cancellation must
not acquire a fresh cleanup allowance for each persistence step. Both were corrected and tested.
The final focused evidence-store suite passed 38 tests; the runbook suite passed 18.

On the PR #4 branch, `cargo test --workspace --all-features --locked --offline` passed 595 tests,
with zero failures and 33 existing scoped ignores. Strict workspace Clippy, formatting, ordinary
CLI build, and specification checks passed. Both new CLI runbook regressions ran successfully.
The first integrated attempt stopped at a Linux `ETXTBSY` launch race in an existing MCP fixture;
the fixture now uses the already established staged-copy/rename pattern (`c4cc453`). The complete
rerun passed. That test-only change is not part of the PR #3 backport because its fixture did not
exist on the older branch.

On PR #3's backport `f12e4c9`, the separate full workspace suite passed 335 tests, with zero
failures and 19 existing scoped ignores. Strict workspace Clippy, formatting, ordinary CLI build,
and the older branch's 44-schema specification checks passed. The ordinary binary SHA-256 is
`aeae08ed92072e108c57963d1d84f459e42ae73355f1df9ea3e83cdff89197d2`.
The corresponding PR #4 binary SHA-256 is
`9e7ecbe4b81dab61f6419e931d756b23622c05c212364da80536e1cec2ab8f3f`.
Subsequent evidence/roadmap commits change documentation only; these counts and hashes remain
bound to their named code revisions, not to an unperformed additional test run.

The changes preserve existing formats and do not silently migrate journals. Older corrupt or
foreign-format state is refused. Explicit recovery is the supported public cleanup path for
pre-intent leftovers; it does not synthesize a bundle or replay collection. Native Windows and
live-target acceptance remain outside these Linux fixture results.
