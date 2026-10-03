# Linux development bundle

This implements the Linux development portion of CAP-25/WP-05. Current execution status belongs
in [the roadmap](roadmap.md). Publication, managed installation, Windows support and the five-task
human/agent pilot remain separate requirements. The first artifact is a portable development
archive, tested in Cally's existing Debian 13 x86-64 GNU/Linux environment.

## Mission and compatibility

Create a bundle from exact source and binary bytes, verify it against a separately supplied archive
digest, install it into a fresh explicit version directory and execute the literal-command mission
away from the source checkout. Install a distinguishable second artifact, select it explicitly,
then select the retained previous artifact and repeat the mission. External configuration and
synthetic evidence must remain byte-identical throughout. Corruption must be refused before
installation or selection.

The bundle contains the ordinary `save` binary, which embeds the browser assets and reviewed task
bindings. Feature-gated test executables are excluded. Runtime support is limited to the exact
Linux/architecture/libc environment actually tested; an archive is not a static-linking or broad
distribution claim. Python 3.11+ is optional for the two embedded offline tasks. Node is unnecessary
at runtime. Restricted command execution still needs the documented Git/ripgrep/Bubblewrap,
kernel and fixed library prerequisites in [the profile contract](read-profile.md). Offline help,
discovery and doctor must remain useful when an optional runtime is absent.

Context-containing source builds also provide the [offline context CLI](context.md), configured
with an explicit export path and review-age policy. Its regular-file descriptor checks require
Linux `openat2` and the trusted proc descriptor route; unavailable prerequisites are refused.
The installed mission must exercise both direct selectors and structured requests, validate the
observed export digest, and preserve stale/ambiguous/missing refusals without choosing a fallback.
Context configuration is not exposed by the current UI/MCP servers. Earlier artifacts retain
their original feature sets and identities.

## Artifact identity and contents

Each artifact has a source-qualified ID rather than an invented product release version. Its
manifest includes the product version, exact source capture identity, binary digest and size,
platform/build metadata, a bounded list of file sizes/digests/modes and the current stateless
compatibility contract. Preserve the source-file manifest and recorded toolchain/build command.
The build evidence binds those claims to the actual Cally build and tested archive.

Include the project/upstream notices, dependency inventory with license metadata and available
license texts, runtime/compatibility notes and operation/rollback instructions. Distinguish the
locked dependency inventory from a claim about which packages are linked into the binary. Missing
license evidence remains an explicit distribution gap; do not label the development bundle as a
fully cleared release or silently invent license text.

Repacking identical input bytes and metadata must give the same archive digest. This is archive
determinism, not a claim that separate Rust builds produce identical executable bytes. The expected
archive digest comes from the operator's trusted build record; checksums alone provide no signature
or protection against replacing both an artifact and its advertised digest.

## Bounded installation and selection

Use an explicitly supplied install root; do not edit global PATH, shell profiles, system services,
host agent grants, credentials or package-manager state. All rehearsals use disposable directories
inside the existing test boundaries. Per-user installation retains ordinary same-account authority.

Verify the expected archive SHA-256 before writing installation data. Accept only the bounded,
documented archive layout and manifest revision. Reject absolute/traversing/duplicate paths,
symlinks, hard links, special files, mismatched digests/sizes/modes, unknown required metadata and
oversized files/archives. Do not run arbitrary post-install code or anything from the archive during
validation. Extract into a run-owned staging directory and publish the complete version only after
validation succeeds. Do not overwrite an existing version with different bytes.

Versions are separate immutable directories under the explicit root. Activation is a separate
explicit operation and uses an atomic relative pointer only to a verified installed version.
Refuse an unrelated existing activation file or escaping pointer. Revalidate the selected version
before switching, preserve the prior version, and disclose the selected path/identity. Same-account
modification remains possible; this is not a protected administrator installation.

Rollback selects a retained verified version; no execution is replayed. Current product storage is
`record=never` with session-only UI history. There is no config/store migration in this increment.
The installer/selector never rewrites external config or evidence. Durable storage and migrations
will need their own compatibility and backup contract when implemented.

## Required evidence

- Exact frozen source, build environment, archive and installed binary identities; executable modes.
- Deterministic repacking; rejected corruption and hostile archive paths/types without activation.
- Fresh install, repeated install, two distinguishable artifacts, explicit upgrade and rollback.
- Offline help/version/doctor; the literal-command mission and both embedded tasks away from source.
- Installed stdio numerical MCP transaction and embedded UI response inside private test networking.
- Honest missing-Python behavior; no global/path/service/grant effects and unchanged external canaries.
- Independent review and executable acceptance bound to the actual archive, plus confirmed cleanup.

Actual commands and their verified outcomes are added to [development](development.md) when the
tooling exists. AC-34/AC-35 acceptance is scoped to the tested Linux/stateless slice. AC-36 still
requires the separate human/agent usefulness pilot.
