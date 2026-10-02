# Operating and developer guide

This guide specifies the documentation and operating behavior the future product must deliver.
Commands are proposed examples, not instructions to install or run an existing product. No live
endpoint, credential provider or target is configured by this planning package.

## Installation and first use

The release page must list supported OS/architecture, artifact digest, provenance/signature,
minimum OS/runtime requirements, license and compatibility notes. Install into a trusted location
separate from a repository under investigation. A per-user install must document its weaker
same-account protection; a managed install must document who can replace its files.

Proposed first-use sequence:

1. Obtain the selected release from the trusted distribution location and verify its integrity.
2. Run save --version and save doctor. These local checks require no production access.
3. Create/select a configuration file containing target aliases and credential references.
4. Have the human/protected runtime configure the credential provider and its exact origin/org scope.
5. Validate the configuration, then explicitly select one target for an online doctor/read check.
6. Run the documented read-only pilot and inspect its result/coverage.
7. Register the MCP server only in the intended host profile, enable the exact tool set, and run
   installed-host success and denial cases.

The installer must not silently modify shell profiles, agent grants, credentials, Git settings or
another tool's PATH. A portable archive is the first distribution candidate; managed packages and
automatic updates need their own provenance and rollback design.

## Configuration contract

The [draft configuration schema](schemas/config.schema.json) stores non-secret settings and
connection aliases. Target references bind protocol/origin, organization, API profile and credential
reference. Service/resource aliases come from approved context records. A connection with missing
credential binding is unconfigured, not anonymous by accident.

Explicitly selected user configuration can choose ordinary defaults. It cannot increase
administrator/launcher policy. Credential providers must bind references to approved origins,
organizations and operations independently of caller-editable configuration; changing a URL must
not redirect a credential to a new host. Agent tools have no arbitrary --config override.

State/cache/config directory conventions are selected per platform in DEC-08 and shown by doctor.
Config validation reports unsupported fields and versions; migration writes a recoverable backup
with restricted permissions. Logging of effective configuration includes only non-secret values
and reference names where those names are safe to disclose.

## Daily operator workflows

| Task | Proposed sequence | What the operator checks |
|---|---|---|
| Ordinary command | inspect if useful, then exec with target/cwd/limits | Resolved executable, original exit code, truncation |
| Grafana lookup | Select alias/org, retrieve dashboard or query | Actual identity, interval, query errors and coverage |
| Script | Describe task, check dependency, run typed inputs | Installed version, side effects, structured result |
| Service investigation | Resolve context, collect pack, compare/report | Stale mappings, missing checks, compatible units/windows |
| Handoff | Select bundle, render report, review before sharing | Private data, observations versus hypotheses, source references |
| Offline work | Open bundle, verify it, run analysis offline | Original observation time and no live-state inference |

Examples use explicit targets. The default configuration must not silently select production.
Missing access is reported with a supported next step rather than installing a replacement tool
or changing credentials during an incident.

## Troubleshooting

| Symptom | Diagnostic path | Required product behavior |
|---|---|---|
| Command unavailable | doctor and capability describe | Name missing binary/runtime and supported installation owner |
| Command denied | command.inspect and policy reason | Show applicable rule without exposing protected policy values |
| Wrong service/environment | context.resolve and source revision | Refuse ambiguity; show conflicting records |
| Grafana unauthorized | Named target online doctor | Safe category; no token, headers or raw error body |
| Wrong org/data source | Bound identity output | Stop before query; never switch active org implicitly |
| Query returns no data | Inspect window, response and coverage | Distinguish empty data, no traffic, failure and unknown coverage |
| Output missing | Result limits and truncation metadata | Identify omitted bytes/items and whether a bounded artifact exists |
| Task hangs | Job/run status and timeout | Stop owned work; report incomplete termination or unknown effects |
| Disk full | Store health and retained artifacts | Preserve known state, report partial capture and safe recovery |
| MCP tools absent | Exact host registration/version/tool inventory | Separate registration failure from adapter/API failure |
| Extension unavailable | Inspect version/dependencies/grants | No auto-install, auto-enable or arbitrary fallback execution |
| Upgrade failed | Version/store compatibility and previous artifact | Restore documented compatible state without deleting evidence |

Support bundles require explicit export and contain sanitized versions/configuration categories,
selected run metadata and diagnostics. They exclude credential stores, raw environments and
unselected telemetry. Preview and classification precede any external upload.

## Retention and backup

Users can list retained runs, protect a bundle and request deletion of named owned data. Deletion
never expands beyond the configured state root; path containment and active-reference checks are
mandatory. Local audit/evidence retention may differ, and unresolved effects cannot be silently
garbage-collected. Artifact deletion is distinct from revoking copies already exported.

Backups cover config references, metadata and explicitly retained artifacts according to policy.
Restoration validates schema/digests/ownership and does not replay runs. Credentials are restored
through their provider's procedure, not from an evidence backup.

## Release and rollback procedure

Before publishing: run phase acceptance on exact artifacts, review dependencies/licenses, publish
checksums/provenance/SBOM, execute documentation examples on a clean host, and rehearse upgrade plus
rollback with representative state. Document version/edition/host limits and unresolved cases.

Before adoption: select release, confirm configuration/store compatibility, retain a recoverable
previous artifact/state snapshot, and run the named pilot checks. On regression, stop new dispatch,
preserve run/effect state, then restore only through the tested rollback path. Unknown external
effects require reconciliation independently of the binary rollback.

A release owner decides distribution and adoption. Green CI, generated docs or this plan do not
promote a candidate automatically.

## Developer workflow

For a new capability, define the user task and ID, typed input/output, effect class, targets,
credential needs, bounds, error categories, compatibility and acceptance cases before implementation.
Use the smallest adapter seam; share normalization/policy/result code through the core.

The initial repository uses a locked Rust toolchain and Cargo lockfile with reviewed dependencies.
CI checks formatting, linting, unit/property/contract/platform tests, dependency/license policy and
reproducible release artifacts. Exact commands and versions are recorded after WP-02 chooses them.
No floating latest-version dependency is a reviewed pin.

Extensions include manifest/schema/examples, a fixed executable protocol, compatibility range,
provenance and tests. Provide deterministic fixtures and a local no-credential test path. A fourth
different extension must pass AC-29 before marketing the extension boundary as mature.

Source changes update user help, schemas, MCP descriptions and examples together. Breaking changes
include a migration plan and overlapping support policy. A contribution that grants new execution
or credential paths requires authority review, not just formatting and passing happy-path tests.

## Documentation delivered with each feature

Every implemented capability ships: purpose/limits, supported environments, installation/config,
human example, agent schema/example, output meanings, error recovery, permission/effect statement,
version compatibility and acceptance evidence. Later scheduler/runner/UI features also ship
deployment, monitoring, backup and incident procedures for their own services.

Maintain one API/schema source and generate reference material where practical. Human task guides
remain authored explanations. Generated documentation is checked against the implementation.
The documentation set includes onboarding, common commands, Grafana, script authoring, service
context, diagnostics, evidence/replay, reports, extensions, jobs, remote operations, scheduling,
controlled changes, UI, troubleshooting and migration/release notes.
