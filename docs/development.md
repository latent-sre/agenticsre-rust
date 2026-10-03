# Build and exercise the PoC

The PoC includes an operator-local Rust command runner and a local browser interface. Evidence and remaining gaps are in
[the roadmap](roadmap.md); the [PoC contract](poc.md) defines its behavior. The draft product catalog
is broader than the available commands. No daemon, credentials or LLM are needed.

## Local build

Use your existing Rust toolchain management. `rust-toolchain.toml` pins 1.98.0 with rustfmt and
clippy. From the repository root:

```bash
cargo fetch --locked
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo build --locked -p workbench-cli
target/debug/save --help
target/debug/save --json exec --cwd . -- /usr/bin/printf '%s\n' 'hello workbench'
target/debug/save --json exec --cwd . --timeout 100ms -- /usr/bin/sleep 5
target/debug/save --json command inspect --cwd . -- /usr/bin/printf '%s\n' hello
target/debug/save --json capabilities list
target/debug/save --json doctor
```

The timeout example intentionally exits 1; the others should exit 0 on the exercised Linux
platform. `--json` emits one result with captured child stdout/stderr. Exit 0 establishes command
completion, not infrastructure health. Child effects are unclassified.

Bare names resolve in `/usr/bin` and `/bin`; use an absolute path for another trusted operator
binary. Cwd is canonicalized; stdin is closed. Child environment is exactly `PATH=/usr/bin:/bin`,
`LANG=C.UTF-8`, `LC_ALL=C.UTF-8`, `TZ=UTC`. Parent secrets/proxies/shell/Git overrides are not
inherited. A child still has its OS account's filesystem and network authority. Treat output as
untrusted and possibly sensitive.

The profile supports `record=never` and local target `workstation`. No run database is created;
a run ID correlates this invocation. `call --request FILE` uses the existing request schema and
requires an absolute cwd. Unsupported fields/modes fail before dispatch.

## Local browser interface

The built executable embeds the frontend; Node is unnecessary at runtime. Start a session with
the numerical error-budget check:

```bash
target/debug/save ui serve --allow task.run
```

Open the printed `http://127.0.0.1:PORT/#token=…` launch link in your browser. The page immediately
removes the fragment and keeps the session token in memory. Reopen the original launch link after
a reload. The session offers checks, exact JSON result downloads and history, with light/dark and
mobile layouts. Closing the server discards history; closing a browser tab does not stop work.
SIGINT/SIGTERM stops the server with bounded cleanup and exit 130/143 respectively.

To grant a checkout's restricted Git/ripgrep checks, first create the explicit trusted
[read policy](read-profile.md), then select its root ID:

```bash
target/debug/save --read-policy /absolute/path/read-policy.json ui serve \
  --allow process.exec --allow command.inspect --allow task.run --root checkout
```

Only granted checks/roots appear. The browser cannot select an arbitrary executable, directory,
policy or shell command. Required Linux isolation must be available; there is no unrestricted
fallback. This PoC permits one active check, 50 completed receipts and 16 MiB of result history
per server session. It has no AI chat, durable history or multi-user access. See the complete
[GUI contract](ui.md) for the HTTP/authentication, lifecycle and size bounds.

The listener binds only `127.0.0.1`; `--port` can select a fixed loopback port when using an existing
SSH tunnel. No service, firewall rule or browser launch is installed automatically. Keep the
printed launch token private. Native Git/ripgrep browser acceptance is recorded separately from
the numerical flow in the roadmap.

Frontend development uses the pinned Node 22.23.1 toolchain and `web/package-lock.json`:

```bash
cd web
npm ci
npm run check-generated
npm run lint
npm test
npm run build
```

`npm run build` regenerates types from the canonical OpenAPI contract and rebuilds `web/dist`.
The Rust asset build rejects missing/oversized assets; rebuild `save` after frontend changes.
Keep development debug symbols disabled for the restricted profile's 64 MiB executable ceiling.
The generated bundle included in the source snapshot supports offline Rust builds; source checks
detect stale API types.

On the current development host, `tools/test-ui-sandbox.sh` reuses the cached Node/Chromium/Playwright
runtime with a private network, home and scratch, exposing only declared build output as writable:

```bash
tools/test-ui-sandbox.sh bash -c 'cd web && npm run lint && npm test && npm run typecheck'
tools/test-ui-sandbox.sh bash -c '"$WORKBENCH_BROWSER_PYTHON" -B web/tests/browser.py --binary target/debug/save'
tools/test-ui-sandbox.sh .venv/bin/python -B crates/workbench-ui/tests/http_contract.py target/debug/save
```

The browser driver starts actual embedded servers inside the sandbox. The local lane explicitly
marks native restricted-command checks as not run; it cannot replace Cally's required native lane.
Do not run product listeners or browser checks on the host to bypass the declared test boundary.

## Stdio MCP

An MCP client launches the ordinary binary with pipes for stdin/stdout:

```bash
target/debug/save mcp serve --transport stdio --allow task.run
```

This grant exposes only the numerical error-budget task. Direct terminal/regular-file transport
is refused. The [MCP contract](mcp.md) specifies request metadata for `2026-07-28` and the separate
`2025-11-25` handshake, plus command/root and Grafana/target grants. Wire arguments cannot widen
startup authority. The [acceptance record](mcp-verification.md) covers controlled raw-client and
native fixtures; installed-host registration remains a separate step.

## Local verification

The fixed dashboard task uses system Python 3.11+ and the embedded checker. From the repository
root, exercise the supplied model fixtures (the input-file model path resolves from this directory):

```bash
target/debug/save --json task list
target/debug/save --json task describe dashboard-hygiene
target/debug/save --json task run dashboard-hygiene --input tests/fixtures/dashboard-hygiene/clean-input.json
target/debug/save --json task run dashboard-hygiene --input tests/fixtures/dashboard-hygiene/findings-input.json --fail-on-findings
```

The last command intentionally exits 1 while reporting a successfully completed check with one
`panel-description` finding. Without `--fail-on-findings`, a completed check with findings exits 0.
These findings concern offline model hygiene, not a running service. See [the task contract](tasks.md)
for bounds, model support, runtime and provenance behavior.

The [error-budget task](error-budget.md) accepts typed numerical inputs through the same interface:

```bash
target/debug/save task run error-budget --input tests/fixtures/error-budget/time-input.json
target/debug/save --json task run error-budget --input tests/fixtures/error-budget/burn-input.json
```

Completed calculations exit 0 even for exhausted budgets or a page verdict. Results distinguish
time/request units, fixed example alert policy and full-budget projection; no alert is sent.
Use the roadmap's current status when checking out an intermediate implementation.

Use the pinned Python 3.14 documentation environment from the root README. Bubblewrap provides
tests without credentials or network access, with a private PID namespace:

```bash
cargo fmt --all -- --check
tools/test-sandbox.sh cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
tools/test-sandbox.sh
tools/test-sandbox.sh .venv/bin/python -B tools/verify-poc.py target/debug/save
tools/test-sandbox.sh .venv/bin/python -B tools/verify-tasks.py target/debug/save
tools/test-sandbox.sh .venv/bin/python -B tools/verify-error-budget.py target/debug/save
tools/test-sandbox.sh .venv/bin/python -B tools/verify-grafana.py target/debug/save
.venv/bin/python -B docs/sre-workbench/verify_specs.py
git diff --check
```

The `test-fixtures` feature enables the real-child helper and process acceptance tests; ordinary
`cargo test` without it does not constitute full PoC verification. Test tools are excluded from
the default CLI build. The sandbox mounts source/toolchain read-only, writes build output under
`target/`, and provides private scratch. Fetch dependencies beforehand. No host home directory or
credentials are mounted. Build scripts execute inside the boundary during sandbox builds too.
The helper uses two build jobs and test threads, disables incremental compilation and omits
development/test debug symbols to bound the local cache. Those settings affect development
artifacts, not runtime limits. Use one shared `target/` and clean redundant build output before
local free space falls below 8 GiB; retain source, logs and accepted binary/source identities.

Process supervision covers ordinary owned groups. Deliberately escaping groups are outside its
containment guarantee; hostile programs need an independent sandbox. This test boundary is not
silently applied to operator commands.

## Cally verification

`tools/cally-check.sh` uses Cally's existing rootless engine over its supported SSH path. It freezes
source with SHA-256, vendors locked crates locally, and streams the snapshot to a disposable
container. The default task lane uses amd64 Rust 1.98.0 on Debian 12 with system Python 3.11.2:

```text
docker.io/library/rust@sha256:4e4a7e7939c17991ab35f2b8c2e67593980f771d28f6b1254b1850f860fd0c7f
```

```bash
tools/cally-check.sh
tools/cally-check.sh minimal
```

The container has no network or host mounts, drops capabilities, prohibits privilege gain, and
is limited to 2.5 CPUs, 4 GiB, 512 processes and 15 minutes. It runs Rust tests and the actual
command/doctor demonstration and records the binary digest. It is removed on exit. The source
archive, hashes and logs remain under the printed `/tmp/agenticsre-poc-cally.*` path. Disposable
extracted sources and the vendored build archive are removed on success or failure. The helper
requires at least 8 GiB local free space before staging. Logs attest
that snapshot, not subsequent edits. The helper uses `/home/hawkfire/.ssh/husker`; no credential
value is read or copied into tests.

Cally selects its installed 1.98.0 toolchain explicitly; clippy/rustfmt checks run locally. The
container cannot download missing components. This also applies to `tools/cally-profile-check.sh mcp`:
retain a passing local sandbox Clippy result for the same source candidate alongside the Cally
runtime results. A Cally pass alone does not establish the separate lint gate. The `minimal` lane
retains the original PoC image
`docker.io/library/rust@sha256:af0579d28b9a7ec5251aaafcb0c0a23dcde5c97065112aae0cc3abeda42d5394`
without Python. It builds the CLI and checks that task discovery/invocation explicitly reports the
missing dependency for both tasks. It also runs the Grafana Rust HTTPS test target without Python.
The default lane runs the full Rust suite, both Python tasks and the independent public HTTPS
fixture checks. Cally's Python lane lacks the documentation-validator packages, so that public
fixture run explicitly skips schemas; local and independent verification validate them.

The restricted-profile lane is `tools/cally-profile-check.sh`, under the
[read-profile contract](read-profile.md). It uses the pinned compatible Debian 13 image, UID/GID
1000 and the same outer resource/network limits. A disposable image adds only root-owned frozen
source and vendored dependencies to the pinned base. Execution uses a read-only root filesystem,
a 512 MiB private `/tmp` and a separate 2 GiB writable `target` tmpfs; the helper checks these
boundaries before running product code. The source archive and derived image identities are
retained, while the run's container, derived image and local staging are removed on exit. Shared
base images remain cached. It explicitly includes the native CLI and core tests that require
nested isolation; an unavailable required boundary fails this lane. The
separate local `--admission-only` public check cannot establish native containment. This lane's
full product acceptance remains pending until recorded in the roadmap.

`tools/cally-profile-check.sh gui` extends that same bounded lane with the GUI HTTP/native parity
test and actual browser checks. It stages only the existing cached browser/Python dependencies,
records their identities and streams the COPY-only image context without a duplicate build archive.
No browser download or host package installation occurs. The browser and app communicate only
inside the container's private network. Bounded screenshots/JSON evidence return through the test
log before temporary storage disappears, into the run's `ui/` directory. The source, runtime and
image identities remain separate, and the same exact-name cleanup removes the run's image/container.
For a correction confined to the GUI, `tools/cally-profile-check.sh gui-focused` runs the UI
library and native HTTP tests, rebuilds the CLI and executes the browser driver under that identical
boundary. Its success is scoped GUI evidence; retain the separately identified core/profile/Grafana
results instead of calling this focused lane a full-suite pass.

This creates no Forgejo repository/runner registration and invokes no paid PR review. The included
GitHub workflow needs an actual hosted run before it can be called verified; local linting and
Cally execution do not prove GitHub integration.
The hosted Linux job explicitly installs Git, ripgrep and Bubblewrap before compiling the
read-profile fixtures; the generic Ubuntu runner image is not assumed to provide them. It uses
the same two-job/no-debug-symbol build settings as the local and Cally lanes. Tests requiring the
explicit native containment environment still run on Cally rather than being silently enabled on
the hosted runner.

## Offline service context

The source CLI includes the [offline context resolver](context.md), with
[independent native evidence](context-verification.md). Build from this source using the normal
locked CLI build. Select a regular export file and review-age policy at startup; requests supply
only explicit selectors. From the repository root:

```bash
target/debug/save --json \
  --context-source "$(pwd)/tests/fixtures/context/unique.json" \
  --context-max-age-days 30 \
  context resolve --service checkout --env dev --team platform --deployment blue
```

The sample retains its declared review date. An expired record returns `context_stale`; changing
the policy is an explicit operator startup decision. The public Cally checks used generated recent
synthetic exports for successful observations and separate deliberately stale/future exports for
refusals. This sample command uses the same verified normalization, with repository paths substituted.

Use `call --request FILE` for the existing structured request interface, keeping the same startup
options. The operation is `context.resolve`; its `inputs` object contains `service`, `environment`
and optional `team`/`deployment`. Source path and age policy are not request fields. Discovery reports
`requires_configuration` without opening the export. Resolution reads metadata only: bindings and
runbook references do not select a command, target, credential or approval. The source options are
rejected for the current UI/MCP servers.

The previously accepted development archives below contain the earlier MCP/browser source. They
do not include `context resolve`; source build and archive identities are kept separate.

## Development artifact and rollback

The [Linux development bundle](packaging.md) contains the ordinary `save` binary, embedded browser
assets and task bindings, exact source/build manifests, dependency inventory and available notices.
The [acceptance record](packaging-verification.md) identifies the actual current/prior archives and
their separately trusted SHA-256 values. The tested runtime is Debian 13 x86-64 with glibc 2.41;
the artifact does not establish compatibility with Adama or other distributions.

Use an operator-trusted checkout of `tools/package-linux.py` with Python 3.11+. Validation runs no
archive code or product commands. Supply an absolute installation root you control; the installer
creates version directories and selection is a separate operation. These operations passed in
disposable Cally directories. The paths below illustrate an explicit per-user installation and
have not been applied to a host installation:

```bash
python3 -I -B tools/package-linux.py verify /absolute/path/current.tar \
  --expected-sha256 76a2ca3414ae624cb51f7ff6378e78771336ec9360b8f4c9e3618e8f52aafa02
python3 -I -B tools/package-linux.py install /absolute/path/current.tar \
  --expected-sha256 76a2ca3414ae624cb51f7ff6378e78771336ec9360b8f4c9e3618e8f52aafa02 \
  --root /absolute/path/workbench-install
python3 -I -B tools/package-linux.py activate \
  save-0.1.0-src-8f4d42b935291593-bin-939e5f5b5d2bccea \
  --root /absolute/path/workbench-install
/absolute/path/workbench-install/current/bin/save --json doctor
/absolute/path/workbench-install/current/bin/save ui serve --allow task.run
```

The UI prints its local browser URL. It needs no Node server; both fixed offline tasks require
system Python 3.11+. Stdio MCP can be launched from the same selected binary using the
[explicit grant instructions](mcp.md). Installing or selecting a version does not add grants,
global PATH entries or system services.

Install the retained previous archive with its own trusted digest before rollback:

```bash
python3 -I -B tools/package-linux.py install /absolute/path/previous.tar \
  --expected-sha256 efa9fd14bb7daebe7cca729315f867aa34fbfad7e917f0f200c0346ecb179ba2 \
  --root /absolute/path/workbench-install
python3 -I -B tools/package-linux.py rollback \
  save-0.1.0-src-60395b4757c8ad83-bin-2990afe949f1279a \
  --root /absolute/path/workbench-install
```

Selection revalidates installed files and atomically switches a relative `current` pointer. Versions
remain separate; no stored data or configuration is migrated. Existing external configuration and
evidence are never rewritten. Same-account modification remains possible. A checksum supplies
integrity relative to the separately trusted digest; no signature or release authenticity is claimed.

To create another development bundle, first capture exact source-file digests and observed build
metadata in the version-1 shapes enforced by `tools/package-linux.py`. Build evidence must match
the supplied binary. Prepare notices from a checksum-verified Cargo vendor directory and the tracked
frontend notice inventory; the helper runs no dependency code:

```bash
python3 -I -B tools/prepare-package-notices.py --repo /absolute/source \
  --vendor /absolute/vendor --output /absolute/fresh-notice-inputs
python3 -I -B tools/package-linux.py create --binary /absolute/build/save \
  --source-manifest /absolute/source-manifest.json \
  --build-provenance /absolute/build-provenance.json \
  --inventory /absolute/fresh-notice-inputs/inventory.json \
  --notices-dir /absolute/fresh-notice-inputs/notices \
  --product-version 0.1.0 --output /absolute/fresh-bundle.tar
```

The response returns the artifact ID and archive digest. The independent Cally build used these
commands with run-owned absolute paths and ordinary current/prior binaries. Repacking identical
inputs passed deterministically; reproducibility of separate Rust builds is not asserted.
Shipping still requires license/distribution clearance, broader native platform evidence and the
human/agent usefulness pilot identified in the roadmap.
