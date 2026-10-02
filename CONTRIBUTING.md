# Contributing

Read the [planning package](docs/sre-workbench/README.md) and [product roadmap](docs/roadmap.md)
before selecting work. Match each implementation slice to its requirement and acceptance IDs.
The roadmap is the sole live status record; delivery tables describe dependencies rather than
creating another backlog.

## Work in a bounded branch

Inspect Git status and preserve unrelated work. Use a separate branch/worktree when other work
shares the checkout. Keep product implementation, host integration, credential setup and live
operation authority distinct. An example or planned feature does not authorize production access.
Preserve the source/license provenance of imported material.

## Verify documentation and contracts

Use a local Python 3.14 environment. Install requirements-docs.txt with pip --require-hashes.
Run python -B docs/sre-workbench/verify_specs.py using that interpreter, then git diff --check.
The verifier checks draft schemas, positive/negative fixtures, local links and planning IDs.
It must refuse to run without the date-time format checker so format checks cannot silently disappear.

When changing a schema, include an independently meaningful accepted or rejected fixture in
docs/sre-workbench/examples/cases.json. A schema match does not establish permission, path safety,
artifact provenance, target identity or runtime behavior. Keep those limits in the specification.

requirements-docs.in lists selected dependencies; requirements-docs.txt records resolved versions
and hashes. Update the input intentionally, regenerate the lock with uv pip compile --python 3.14
--generate-hashes requirements-docs.in -o requirements-docs.txt, inspect the dependency change and
verify in a fresh environment. Do not update pins as an incidental documentation edit.

## Implementation and publication

Before Rust implementation, resolve the decisions needed for that slice and select a reviewed
toolchain/dependency set. Add behavior through the shared core, then keep CLI and MCP contracts
consistent. Test failure and denial paths with disposable targets and synthetic credentials.

Before pushing, fetch the target base, inspect the exact diff, run affected checks and the
specification verifier, and report what remains unverified. Open a pull request with scope,
requirement/acceptance IDs, verification and risks. Do not merge, release or apply production
changes solely because structural checks pass.
