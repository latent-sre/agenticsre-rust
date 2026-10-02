#!/usr/bin/env bash
# Exercise required nested-isolation proofs in Cally's bounded rootless engine.
set -euo pipefail
IFS=$'\n\t'

repo_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
mode=${1:-profile}
if (( $# > 1 )) || [[ "$mode" != profile && "$mode" != gui && "$mode" != gui-focused && "$mode" != mcp ]]; then
    printf '%s\n' 'Usage: tools/cally-profile-check.sh [profile|gui|gui-focused|mcp]' >&2
    exit 2
fi
image_ref=docker.io/library/rust@sha256:620dbcd124499c59e2406d3741574b5c5838cf9eb9656f0c3a03948f79b02959
evidence_dir=$(mktemp -d /tmp/agenticsre-profile-cally.XXXXXXXX)
remote_started=0
cleanup_staging() {
    local run_status=$?
    local cleanup_status=0
    trap - EXIT
    if (( remote_started )); then
        # Both names belong to this mktemp-scoped run. Remove only its container
        # and image; --no-prune preserves the shared pinned base and image cache.
        # shellcheck disable=SC2029
        timeout 20s ssh "${ssh_args[@]}" "sudo -n /usr/local/libexec/forgejo-runner-engine rm --force --ignore $run_name" \
            >> "$evidence_dir/remote-cleanup.log" 2>&1 || cleanup_status=1
        # shellcheck disable=SC2029
        timeout 20s ssh "${ssh_args[@]}" "sudo -n /usr/local/libexec/forgejo-runner-engine image rm --ignore --no-prune $test_image" \
            >> "$evidence_dir/remote-cleanup.log" 2>&1 || cleanup_status=1
    fi
    rm -rf -- "${evidence_dir:?}/source" "${evidence_dir:?}/build.tar" \
        "${evidence_dir:?}/context-hash.pipe" || cleanup_status=1
    if (( cleanup_status == 0 )); then
        printf '%s\n' 'Removed disposable extraction and build archive; source archive, hashes and logs retained.' \
            > "$evidence_dir/staging-cleanup.txt" || cleanup_status=1
    fi
    if (( cleanup_status != 0 )); then
        printf 'Staging cleanup or its receipt failed in %s\n' "$evidence_dir" >&2 || :
        if (( run_status == 0 )); then run_status=1; fi
    fi
    exit "$run_status"
}
trap cleanup_staging EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
available_bytes=$(df -B1 --output=avail -- "$repo_dir" | tail -n 1)
if (( available_bytes < 8589934592 )); then
    printf '%s\n' 'Cally staging requires at least 8 GiB free on Adama; clean disposable build output first.' >&2
    exit 2
fi
run_name=agenticsre-profile-${evidence_dir##*.}
test_image=localhost/agenticsre-profile:${evidence_dir##*.}
ssh_args=(-T -i /home/hawkfire/.ssh/husker -o BatchMode=yes
    -o StrictHostKeyChecking=yes -o ConnectTimeout=10 ansible@10.0.0.72)
printf 'Retained evidence: %s\n' "$evidence_dir"
mkdir -p -- "$evidence_dir/source"
git -C "$repo_dir" ls-files --cached --others --exclude-standard -z |
    tar --directory "$repo_dir" --null --no-recursion --verbatim-files-from \
        --mtime=@0 --owner=0 --group=0 --numeric-owner --format=gnu \
        --files-from=- -cf "$evidence_dir/source.tar"
sha256sum "$evidence_dir/source.tar" > "$evidence_dir/source.sha256"
tar -xf "$evidence_dir/source.tar" --directory "$evidence_dir/source"
if [[ -e "$evidence_dir/source/.cargo/config.toml" || -e "$evidence_dir/source/.cargo/config" ||
      -e "$evidence_dir/source/.profile-runtime" || -e "$evidence_dir/source/.ui-runtime" ]]; then
    printf '%s\n' 'Refusing to replace existing snapshot configuration or runtime files.' >&2
    exit 2
fi
mkdir -p -- "$evidence_dir/source/.cargo" "$evidence_dir/source/.profile-runtime"
python3 - "$evidence_dir/source/.profile-runtime" <<'PY'
import hashlib
import json
from pathlib import Path
import stat
import sys

destination = Path(sys.argv[1])
manifest = []
for name in ("bwrap", "rg"):
    original = Path("/usr/bin") / name
    source = original.resolve(strict=True)
    metadata = source.stat()
    if (not stat.S_ISREG(metadata.st_mode) or metadata.st_uid != 0
            or metadata.st_mode & 0o022 or not metadata.st_mode & 0o111
            or not 0 < metadata.st_size <= 64 * 1024 * 1024):
        raise SystemExit(f"Refusing untrusted native runtime binding: {name}")
    content = source.read_bytes()
    if len(content) != metadata.st_size or content[:4] != b"\x7fELF":
        raise SystemExit(f"Invalid native runtime binding: {name}")
    target = destination / name
    target.write_bytes(content)
    target.chmod(0o555)
    manifest.append({"name": name, "source": str(source), "bytes": len(content),
                     "sha256": hashlib.sha256(content).hexdigest()})
(destination / "provenance.json").write_text(json.dumps(manifest, indent=2) + "\n")
PY
(
    cd -- "$evidence_dir/source"
    cargo vendor --locked --offline vendor > .cargo/config.toml 2> "$evidence_dir/vendor.log"
)
cp -- "$evidence_dir/source/.profile-runtime/provenance.json" "$evidence_dir/runtime-provenance.json"
if [[ "$mode" == gui || "$mode" == gui-focused ]]; then
    python3 "$repo_dir/tools/stage-ui-runtime.py" "$evidence_dir/source/.ui-runtime"
    cp -- "$evidence_dir/source/.ui-runtime/provenance.json" "$evidence_dir/ui-runtime-provenance.json"
fi
source_digest=$(cut -d ' ' -f 1 "$evidence_dir/source.sha256")
cat > "$evidence_dir/Containerfile" <<EOF
FROM $image_ref
COPY --chown=0:0 source/ /opt/workbench-source/
LABEL io.agenticsre.source-sha256="$source_digest"
WORKDIR /opt/workbench-source
EOF
# Static digest, local identity and mktemp-generated name expand on the client.
# shellcheck disable=SC2029
ssh "${ssh_args[@]}" "sudo -n /usr/local/libexec/forgejo-runner-engine pull $image_ref"
remote_started=1
# The image build copies files only; product code runs later under the declared
# unprivileged, offline boundary with an enforced read-only root filesystem.
# shellcheck disable=SC2029
# Stream the COPY-only context; retain its digest without a second large disk copy.
mkfifo "$evidence_dir/context-hash.pipe"
sha256sum < "$evidence_dir/context-hash.pipe" > "$evidence_dir/build.sha256" &
hash_pid=$!
# shellcheck disable=SC2029
if tar --mtime=@0 --owner=0 --group=0 --numeric-owner --directory "$evidence_dir" \
    -cf - Containerfile source | tee "$evidence_dir/context-hash.pipe" | \
    ssh "${ssh_args[@]}" "sudo -n /usr/local/libexec/forgejo-runner-engine build --pull=never --network=none --http-proxy=false --layers=false --force-rm --tag $test_image -" \
    > "$evidence_dir/image-build.log" 2>&1; then
    wait "$hash_pid"
else
    build_status=$?
    wait "$hash_pid" || :
    exit "$build_status"
fi
rm -- "$evidence_dir/context-hash.pipe"
# shellcheck disable=SC2029
ssh "${ssh_args[@]}" "sudo -n /usr/local/libexec/forgejo-runner-engine image inspect $test_image" \
    > "$evidence_dir/image.json"
frozen_image=$(python3 - "$evidence_dir/image.json" <<'PY'
import json
import re
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    image = json.load(handle)[0]["Id"]
if not re.fullmatch(r"sha256:[0-9a-f]{64}", image):
    raise SystemExit("Invalid disposable image identity")
print(image)
PY
)
# shellcheck disable=SC2029
run_status=0
# shellcheck disable=SC2029
ssh "${ssh_args[@]}" "sudo -n /usr/local/libexec/forgejo-runner-engine run --rm -i \
    --name $run_name --timeout=900 --user=1000:1000 --pull=never --read-only \
    --tmpfs /tmp:rw,nosuid,nodev,size=512m,mode=1777 \
    --tmpfs /opt/workbench-source/target:rw,nosuid,nodev,size=2g,mode=1777 \
    --env RUSTUP_TOOLCHAIN=1.98.0 --env CARGO_HOME=/tmp/cargo \
    --env CARGO_BUILD_JOBS=2 --env RUST_TEST_THREADS=2 \
    --env CARGO_INCREMENTAL=0 --env CARGO_PROFILE_DEV_DEBUG=0 --env CARGO_PROFILE_TEST_DEBUG=0 \
    --env WORKBENCH_PROFILE_REQUIRED=1 \
    --env WORKBENCH_CALLY_MODE=$mode \
    --env WORKBENCH_PROFILE_BWRAP=/opt/workbench-source/.profile-runtime/bwrap \
    --env WORKBENCH_PROFILE_RG=/opt/workbench-source/.profile-runtime/rg \
    --env WORKBENCH_PROFILE_GIT=/usr/bin/git \
    --network=none --http-proxy=false --cpus=2.5 --memory=4g --pids-limit=512 \
    --security-opt=no-new-privileges --cap-drop=ALL --workdir=/opt/workbench-source \
    $frozen_image sh -ec 'mkdir /tmp/cargo
/usr/bin/python3 -c \"import os; from pathlib import Path; p=Path.cwd(); assert os.getuid()==1000; assert p.stat().st_uid==0; assert os.statvfs(p).f_flag & os.ST_RDONLY; assert not os.access(p / \\\"Cargo.toml\\\", os.W_OK); assert not (os.statvfs(p / \\\"target\\\").f_flag & os.ST_RDONLY); print(\\\"source_rootfs=read-only source_owner=0 runtime_uid=1000 build_target=writable_tmpfs\\\")\"
rustc --version
cargo --version
cat /etc/os-release
cat /sys/fs/cgroup/memory.max /sys/fs/cgroup/cpu.max /sys/fs/cgroup/pids.max
cat .profile-runtime/provenance.json
.profile-runtime/bwrap --version
.profile-runtime/rg --version
/usr/bin/git --version
# Formatting and Clippy are separate local sandbox gates; this pinned runtime image omits them.
if [ \"\$WORKBENCH_CALLY_MODE\" != gui-focused ]; then
cargo test --workspace --all-targets --all-features --locked --offline
cargo test -p workbench-cli --test read_profile --all-features --locked --offline -- --include-ignored
cargo test -p workbench-core --lib --locked --offline read_profile -- --include-ignored
else
    cargo test -p workbench-ui --lib --locked --offline
fi
if [ \"\$WORKBENCH_CALLY_MODE\" = gui ] || [ \"\$WORKBENCH_CALLY_MODE\" = gui-focused ]; then
    cargo test -p workbench-cli --test ui --locked --offline -- --include-ignored
fi
if [ \"\$WORKBENCH_CALLY_MODE\" = mcp ]; then
    cargo test -p workbench-cli --test mcp --all-features --locked --offline -- --include-ignored
fi
# Restore the ordinary operator build after all feature-enabled Rust tests.
cargo build -p workbench-cli --locked --offline
if [ \"\$WORKBENCH_CALLY_MODE\" != gui-focused ]; then
/usr/bin/python3 -B tools/verify-read-profile.py target/debug/save --git /usr/bin/git --rg /opt/workbench-source/.profile-runtime/rg --bwrap /opt/workbench-source/.profile-runtime/bwrap --no-schema --json-results
/usr/bin/python3 -B tools/verify-grafana.py target/debug/save --no-schema
target/debug/save --json exec --cwd /opt/workbench-source -- /usr/bin/printf \"%s\\n\" \"hello workbench\"
fi
sha256sum target/debug/save
if [ \"\$WORKBENCH_CALLY_MODE\" = mcp ]; then
    /usr/bin/python3 -B tools/verify-mcp.py target/debug/save --no-schema
    /usr/bin/python3 -B tools/verify-mcp-commands.py target/debug/save --git /usr/bin/git --rg /opt/workbench-source/.profile-runtime/rg --bwrap /opt/workbench-source/.profile-runtime/bwrap --no-schema --json-results
    /usr/bin/python3 -B tools/verify-mcp-grafana.py target/debug/save --no-schema --json-results
fi
if [ \"\$WORKBENCH_CALLY_MODE\" = gui ] || [ \"\$WORKBENCH_CALLY_MODE\" = gui-focused ]; then
    export PYTHONPATH=/opt/workbench-source/.ui-runtime/site-packages
    export WORKBENCH_BROWSER_EXECUTABLE=/opt/workbench-source/.ui-runtime/browser
    export WORKBENCH_UI_OUTPUT=/tmp/ui-evidence
    export HOME=/tmp/ui-home
    mkdir -p /tmp/ui-home
    browser_status=0
    .ui-runtime/python/bin/python3.14 -S -B web/tests/browser.py --binary target/debug/save || browser_status=\$?
    /usr/bin/python3 -B tools/ui-check-artifacts.py export /tmp/ui-evidence
    exit \"\$browser_status\"
fi'" < /dev/null 2>&1 | tee "$evidence_dir/cally.log" || run_status=$?
if [[ "$mode" == gui || "$mode" == gui-focused ]]; then
    if python3 "$repo_dir/tools/ui-check-artifacts.py" collect "$evidence_dir/cally.log" "$evidence_dir/ui"; then
        :
    elif (( run_status == 0 )); then
        run_status=1
    fi
fi
if (( run_status != 0 )); then exit "$run_status"; fi
printf 'Cally %s checks passed; source identity: ' "$mode"
cat "$evidence_dir/source.sha256"
