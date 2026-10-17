# Offline evidence acceptance

WORKBENCH-016 passes for the Linux x86-64 core/CLI fixture increment in the
[offline contract](offline-evidence.md). This establishes CAP-14/AC-18 for the currently supported
version-1 service-query bundles. Hosted grants, other platforms and additional evidence formats
remain in the [roadmap](roadmap.md).

## Observed behavior

The [installed verdict](evidence/offline-2026-10-11/verification.json) records 13 commands against
an ordinary CLI copied to a disposable installation and invoked from an unrelated directory.
Two real collections produced 12 authenticated fixture requests. All fixture threads were joined
and the connection/context configuration removed before offline analysis. No credential provider
was supplied to subsequent invocations. All 13 exact JSON receipts passed the canonical schema;
all populated offline results also passed the new offline-data schema.

The mission verifies original receipt contents, old observation windows, nanosecond timestamps,
hand-counted metric samples and duplicate log multiplicity, deterministic repeated and structured
replay, whole-receipt/sample detail limits, manifest-pin refusal, and tamper refusal for both
operations. A genuinely collected redacted source remains limited on open/replay, while open
still exposes its original sanitized receipt. The retained
[store hashes](evidence/offline-2026-10-11/store-sha256.json) matched after each read-only sequence;
the intentional tamper control was restored and the original bytes checked again.

The full workspace/all-feature sandbox suite passed before the final inspection correction.
Fresh affected checks on the final implementation passed:

- Six public CLI tests: direct/structured parity, deterministic projections, original bytes/times,
  detail bounds, pins/paths, tampered artifacts, failed receipts, null samples, redacted and
  incompatible frames, incomplete-store refusal and limited recovered-bundle inspection.
- Twenty-two frame tests, including the new open-inspection regression and replay finite/null/
  nonfinite values, integer precision, canonical order, duplicates, bounds and cancellation.
- Four offline core tests, including real kernel denial of socket, messaging, command/process
  creation, io_uring and alternate ABI syscalls on the worker, successful caller socket creation
  before/after, unchanged store files, original receipt preservation and cancellation metadata.
- Hosted/input refusal checks; all-target/all-feature strict Clippy; formatting; ordinary CLI
  build; schema/examples/local-link verification; whitespace checks.

Commands use `tools/test-sandbox.sh`, whose private user/network/PID namespace exposes read-only
source/toolchain and no home credentials. The outer execution restriction initially prevented
namespace creation; the approved sandbox invocation supplied that boundary. Existing ignored
native-platform/read-profile tests remain ignored and are not claimed as newly verified.

## Source identity and review

The [frozen source manifest](evidence/offline-2026-10-11/source-manifest.json) identifies 1038 files
before acceptance/status documentation was added. Its SHA-256 is
`3654318c4245cb430a1557818db9023605fd3c6b93c1ab9216a7e119017d74af`.
The ordinary installed binary SHA-256 is
`987d2417211167e05c16af6a4fe906306c7a4542874c0824b963f6596643db3f`.
Source and binary guards matched before/after installed execution. Later acceptance documentation
does not change tested product, tests, schemas or verifier bytes.

Independent static review found one P2: open could report complete inspection before checking
whether the retained frames were redacted or unsupported. The corrected implementation uses the
same bounded frame parsers during open, preserves incompatibility/partial reasons, and still shows
original receipts. Final scoped review of the frozen correction found no unresolved material
findings and zero independent P0/P1 findings. Executable results above are the coordinator's
observations; the static reviewer did not run them.

The profile restricts the fixed analyzer's network/command syscalls. It is not an arbitrary-code,
filesystem or credential sandbox. This milestone introduces no live access, release claim,
external publishing or UI/MCP grants.
