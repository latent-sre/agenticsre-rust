# Restricted Linux commands — implementation contract

This is the next CAP-01/AC-06 increment. Current execution status belongs in
[the roadmap](roadmap.md). Build on the accepted supervisor and keep the existing operator-local
command behavior available. This contract defines a narrower command profile for a trusted launcher
to attach to requests; it does not establish a separate OS identity for an unrestricted agent.

## Mission and scope

Run an allowed Git or ripgrep observation in an explicitly granted checkout, return its bounded
output and actual exit status, and prevent the command from writing the checkout, reading an
outside canary through a symlink, or reaching another network namespace. Denied forms must never
fall back to generic execution. Inspection is advisory and starts no child; execution revalidates
the actual policy, roots and executable bindings.

The initial Linux profile covers ordinary Git working trees and a small documented set of
`git status`, working-tree/cached `git diff`, bounded `git log`, `rg --files` and search forms.
Require explicit cwd. Use literal relative path arguments and an exact option grammar; reject
unknown options, shell source, config/global overrides, external commands, preprocessors, arbitrary
formats and following paths outside the selected root. Document normalized fixed flags and any
omitted coverage, such as untracked files. Preserve nonzero tool exits without health inference.

Linked worktrees, bare repositories, partial/sparse clones, alternates and unsupported repository
formats require explicit unsupported results until their separate contracts are implemented.
Windows support and additional host/service/network commands remain in the broader plan.

The selected public forms are:

| Tool | Allowed arguments |
|---|---|
| Git status | `status [--short\|--porcelain=v1] [--branch] [-- PATH...]` |
| Git diff | `diff [--cached] [--stat\|--name-only\|--name-status] [--exit-code] [-- PATH...]` |
| Git log | `log [--oneline] [-n N\|--max-count=N] [-- PATH...]` |
| ripgrep listing | `--files [--hidden] [-- PATH...]` |
| ripgrep search | `[-n\|--line-number] [-i\|--ignore-case] [-F\|--fixed-strings] [--hidden] -e PATTERN [-- PATH...]` |

`N` is 1–100, default 20. At most 32 literal relative paths are accepted; `..`, absolute paths,
stdin `-` and symlink paths are refused. Git status is normalized to porcelain v1 and omits untracked
files and submodules. Git diff suppresses external diff, text conversion, color and rename
detection. Git log prints fixed short-hash/subject lines without patches, signatures or decoration.
ripgrep uses one thread, ignores ambient configuration, excludes `.git`, and limits input files
to 8 MiB and matches to 1,000 per file. These are explicit coverage restrictions.
Each path is at most 1,024 bytes and the search pattern is at most 4,096 bytes. Duplicate flags
or aliases are refused; listing does not accept search-only flags. Cwd must be the exact granted
root in this version, rather than a subdirectory of that root.

For an explicitly prepared trusted policy, the intended mission is:

```bash
save --json --read-policy /absolute/read-policy.json exec --cwd /absolute/granted-checkout -- git status
```

The same context applies to `command inspect` and `call --request`. Inspection does not start the
wrapper or establish that the required namespaces can be created. Product execution acceptance
is still tracked separately in the roadmap.

## Policy and trusted inputs

Expose a narrowing CLI option such as `--read-policy /absolute/policy.json`; the later MCP launcher
supplies the same core context outside request JSON. Requests cannot select another policy, role,
runtime, binary, mount or grant. The option restricts command execution; it is not a claim that
other operations have acquired protected-agent authorization. MCP operation and task grants remain
a subsequent contract.

Use a strict, bounded policy file: version, fixed `linux-read-v1` identifier, at most eight named
absolute roots, and explicit Git/ripgrep/Bubblewrap executable bindings. A binding includes its
absolute path and content digest. It must be outside granted workspaces and must not be writable
by other users. Policy and bindings are trusted launcher inputs, never discovered from a workspace.
Malformed, duplicate, unknown or oversized fields fail before execution; no executable config hooks.
The policy limit is 64 KiB with a maximum JSON depth of eight. Roots must not overlap. Bindings
must be regular ELF files no larger than 64 MiB, owned by root or the invoking user, without
group/other write permission or set-ID bits. Their declared digests are checked before execution.

Pin root and executable identities through open descriptors and report the effective binding.
Check cwd and input paths against the selected root. A successful check followed by reopening a
caller-replaceable pathname does not establish containment. The builder owns the smallest necessary
descriptor-passing change; retain file lifetimes and close unused descriptors in descendants.

## Execution boundary

Require Bubblewrap and the required namespace/mount support. Use a fixed template with private
user, mount, network, PID, IPC and UTS namespaces, no new privileges, dropped capabilities, a new
session and parent-death cleanup. No optional namespace flags or unsandboxed fallback.

Bind only the selected root read-only at a fixed namespace path, plus explicitly reviewed runtime
binaries/libraries. Do not mount the host root, home, credentials, resolver settings, sockets or
procfs. Supply minimal devices and bounded private temporary storage. The runtime read scope is
part of the profile and receipt, not an implicit grant to arbitrary host files.
The initial runtime is x86_64 GNU/Linux with six fixed library bindings:
`/lib64/ld-linux-x86-64.so.2`, `/lib/x86_64-linux-gnu/libc.so.6`,
`/lib/x86_64-linux-gnu/libm.so.6`, `/lib/x86_64-linux-gnu/libpcre2-8.so.0`,
`/lib/x86_64-linux-gnu/libz.so.1` and `/lib/x86_64-linux-gnu/libgcc_s.so.1`.
The launcher pins and reports each file; no library directory is mounted wholesale.

A private network namespace does not isolate filesystem Unix sockets inside a granted root.
Require a fixed syscall filter that rejects socket communication, opaque open interfaces,
namespace reconfiguration and unsupported syscall ABIs. Require a Landlock filesystem policy
that denies write/create/truncate actions except access to the namespace's fixed `/dev/null`
device. Workspace files, FIFOs and other devices receive no write exception. Both controls are
mandatory; an unavailable required kernel ABI or failed setup refuses tool execution.
The selected policy requires Landlock ABI 5 or newer, including device-ioctl control;
the launcher checks the private null device identity before granting its exception.
Namespace isolation and read-only mounts remain independently required. Read-only FIFO opens can
block and must remain bounded by the operation deadline.

Git 2.47.3 unconditionally opens `/dev/null` read/write in
[`sanitize_stdfds`](https://raw.githubusercontent.com/git/git/v2.47.3/setup.c).
The first native checkpoint demonstrated that denying every writable open breaks even Git status.
The fixed [Landlock policy](https://docs.kernel.org/userspace-api/landlock.html) supplies a pathname
exception that [classic seccomp](https://docs.kernel.org/userspace-api/seccomp_filter.html) cannot
express by inspecting a pointed-to pathname. A fixed internal launcher stage in the same `save`
image applies this policy inside the completed namespace before executing `/tool`. The trusted
core pins the running image; policy/request fields cannot replace or select a launcher. Its setup
and exec-error channel must be bounded, unavailable to the payload and reflected honestly in the
receipt. Launcher start alone does not establish tool execution. The internal entry is not an
alternative public command executor. This design remains subject to native tests and review.
The current-image binding also has a 64 MiB limit and must be outside the granted roots; use a
release build or disable debug symbols when necessary. The sealed internal launch metadata has
a fixed 256 KiB limit to accommodate JSON escaping of the largest admitted CLI arguments. This
does not enlarge the public request or policy limits. The receipt distinguishes wrapper exit,
launcher admission/Landlock ABI and confirmed tool completion; incomplete status records never
establish a confirmed tool exit.

Git repository configuration must not re-enable filters or hooks. Mask the applicable repository
config with an immutable empty file within the namespace, disable system/global configuration,
pagers, fsmonitor, external diff/text conversion, signature helpers, submodule recursion and lazy
fetch behavior. Detect unsupported repository layouts conservatively. For ripgrep, suppress
ambient configuration and preprocessing; validate explicit paths as well as traversal flags.

Keep the accepted literal-argv supervisor, deadlines, cancellation, output cap and process cleanup
where possible. Distinguish the wrapper's outcome from proof that the requested tool started.
Do not infer an unavailable sandbox solely from an arbitrary exit code. Any trusted status channel
must be bounded, validated and unavailable for payload spoofing. Observe the final encoded receipt
bound after adding profile, binding and isolation metadata.

The profile constrains filesystem/network effects and declares its resource limits. It must not
claim protection from arbitrary kernel defects, a hostile administrator, a caller that can replace
the trusted launcher, or sensitive files deliberately included in a granted root.
The profile itself bounds time, captured output and private temporary storage; it does not impose
a total memory or process-count quota. The Cally verification container supplies those outer limits.

## Evidence and acceptance environment

The [feasibility probe](evidence/read-profile-feasibility-2026-10-02.txt) established that Git
`--no-ext-diff --no-textconv` still permits a configured clean filter, and ripgrep `--no-follow`
still reads an explicitly named symlink. These are actual scratch-marker/canary results, so option
filtering alone is insufficient. Relevant upstream references are the
[Git environment controls](https://git-scm.com/docs/git),
[Git configuration](https://git-scm.com/docs/git-config) and
[ripgrep guide](https://github.com/BurntSushi/ripgrep/blob/master/GUIDE.md).

Local nested Bubblewrap was refused before payload execution under the existing test sandbox.
No host policy was changed. A [Cally prerequisite probe](evidence/read-profile-cally-boundary-2026-10-02.txt)
then succeeded using UID/GID 1000 in the existing pinned Rust container, with network disabled,
no new privileges, no capabilities or host mounts, bounded CPU/memory/PIDs, and no inner procfs.
It read the granted fixture, received EROFS on a write, could not read the outside symlink or
connect to the outer container's private listener, and ran as PID 2. All probe containers were removed.

The probe bundled the host's root-owned Bubblewrap 0.9.0 and runtime libraries using an explicit
loader because the existing Debian 12 image has an older glibc. Its retained manifest and harness
are under `/tmp/agenticsre-cally-profile-probe-61h4qc1i`; archive SHA-256 is
`c3639f29c41ccc3cad43b46f26a4a027c01b60fc8c42baa186c0b62652d89d16`.
The actual product acceptance lane must use a pinned compatible runtime and declared executable
bindings. This prerequisite probe is not product containment acceptance.

A subsequent [native-runtime probe](evidence/read-profile-native-runtime-2026-10-02.txt) established
the compatible image `docker.io/library/rust@sha256:620dbcd124499c59e2406d3741574b5c5838cf9eb9656f0c3a03948f79b02959`.
It ran the copied Bubblewrap 0.9.0 and ripgrep 14.1.0 binaries directly, with image Git 2.47.3 and
Rust 1.98.0, and repeated the same isolation checks. No custom loader was needed in this image.
The native probe archive is `/tmp/agenticsre-cally-profile-native-0j2z0xpy.tar`, SHA-256
`207a41181d90fb0da8af0d4f577b576fe45e62ca5dcf390fe19d9af9175a7c4a`.

A separate [socket probe](evidence/read-profile-socket-boundary-2026-10-02.txt) demonstrated that
the namespace/read-only-mount template alone could still reach an outer listener through a
filesystem Unix socket in the granted root. This is a failed containment prerequisite, not a
product pass. Product acceptance must independently prove the combined profile controls close
that channel and refuse writable FIFO opens and alternate syscall ABIs.

The [Landlock prerequisite probe](evidence/read-profile-landlock-prerequisite-2026-10-02.txt)
passed in the unchanged Cally boundary (kernel ABI 8; required ABI 5 rights). The pinned null
device accepted read/write access. Existing-file writes, truncation via a read-only open, both
FIFO write modes, another device write and file creation were denied; file/FIFO canaries stayed
unchanged. A positive FIFO writer control established the probe could detect communication.
This proves a prerequisite, not the integrated launcher or its receipt channel.

Independent acceptance must exercise allowed commands and hostile config/filter/pager cases,
explicit and raced symlinks, descriptor replacement, forbidden argv, missing runtime/namespace
support, inherited descriptors, read/write/network canaries, output/deadline limits, signals and
an intentionally detached descendant. Reuse passed supervisor cases unless changed behavior
requires another run. Freeze source for correctness/security review and independent execution;
keep the standalone operator, fixed-task and Grafana regressions intact.
