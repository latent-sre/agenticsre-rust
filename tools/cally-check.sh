#!/usr/bin/env bash
# Use Cally's existing rootless engine; no runner registration or host packages.
set -euo pipefail
IFS=$'\n\t'

repo_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
lane=${1:-tasks}
case "$lane" in
    tasks)
        image_ref=docker.io/library/rust@sha256:4e4a7e7939c17991ab35f2b8c2e67593980f771d28f6b1254b1850f860fd0c7f
        ;;
    minimal)
        image_ref=docker.io/library/rust@sha256:af0579d28b9a7ec5251aaafcb0c0a23dcde5c97065112aae0cc3abeda42d5394
        ;;
    *) printf '%s\n' 'Usage: tools/cally-check.sh [tasks|minimal]' >&2; exit 2 ;;
esac
evidence_dir=$(mktemp -d /tmp/agenticsre-poc-cally.XXXXXXXX)
cleanup_staging() {
    local run_status=$?
    local cleanup_status=0
    trap - EXIT
    rm -rf -- "${evidence_dir:?}/source" "${evidence_dir:?}/build.tar" || cleanup_status=1
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
run_name=agenticsre-poc-${evidence_dir##*.}
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
if [[ -e "$evidence_dir/source/.cargo/config.toml" ]]; then
    printf '%s\n' 'Refusing to replace existing Cargo configuration in the snapshot.' >&2
    exit 2
fi
mkdir -p -- "$evidence_dir/source/.cargo"
(
    cd -- "$evidence_dir/source"
    cargo vendor --locked --offline vendor > .cargo/config.toml 2> "$evidence_dir/vendor.log"
)
tar --mtime=@0 --owner=0 --group=0 --numeric-owner --directory "$evidence_dir/source" -cf "$evidence_dir/build.tar" .
sha256sum "$evidence_dir/build.tar" > "$evidence_dir/build.sha256"
# Static digest and mktemp-generated name intentionally expand on the client.
# shellcheck disable=SC2029
ssh "${ssh_args[@]}" "sudo -n /usr/local/libexec/forgejo-runner-engine pull $image_ref"
# shellcheck disable=SC2029
ssh "${ssh_args[@]}" "sudo -n /usr/local/libexec/forgejo-runner-engine run --rm -i \
    --name $run_name --timeout=900 --env RUSTUP_TOOLCHAIN=1.98.0 --env WORKBENCH_TEST_LANE=$lane \
    --env CARGO_BUILD_JOBS=2 --env RUST_TEST_THREADS=2 --env CARGO_INCREMENTAL=0 \
    --env CARGO_PROFILE_DEV_DEBUG=0 --env CARGO_PROFILE_TEST_DEBUG=0 \
    --network=none --http-proxy=false --cpus=2.5 --memory=4g --pids-limit=512 \
    --security-opt=no-new-privileges --cap-drop=ALL --workdir=/tmp \
    $image_ref sh -ec 'mkdir /tmp/work
cd /tmp/work
tar --no-same-owner -xf -
rustc --version
cargo --version
cat /etc/os-release
cat /sys/fs/cgroup/memory.max /sys/fs/cgroup/cpu.max /sys/fs/cgroup/pids.max
if [ \"\$WORKBENCH_TEST_LANE\" = tasks ]; then
    /usr/bin/python3 --version
    cargo test --workspace --all-targets --all-features --locked --offline
else
    cargo test -p workbench-cli --all-features --test grafana --locked --offline
fi
cargo build --locked --offline -p workbench-cli
target/debug/save --json exec --cwd /tmp/work -- /usr/bin/printf \"%s\\n\" \"hello workbench\"
target/debug/save --json doctor
printf \"%s\\n\" \"{\\\"file\\\":\\\"/tmp/work/tests/fixtures/dashboard-hygiene/clean.json\\\"}\" > /tmp/task-input.json
if [ \"\$WORKBENCH_TEST_LANE\" = tasks ]; then
    target/debug/save --json task run dashboard-hygiene --input /tmp/task-input.json
    target/debug/save --json task run error-budget --input tests/fixtures/error-budget/time-input.json
    /usr/bin/python3 -B tools/verify-grafana.py target/debug/save --no-schema
else
    target/debug/save --json task list
    task_status=0
    target/debug/save --json task run dashboard-hygiene --input /tmp/task-input.json > /tmp/task-missing.json || task_status=\$?
    cat /tmp/task-missing.json
    test \"\$task_status\" -eq 2
    grep -q \"missing_dependency\" /tmp/task-missing.json
    task_status=0
    target/debug/save --json task run error-budget --input tests/fixtures/error-budget/time-input.json > /tmp/budget-missing.json || task_status=\$?
    cat /tmp/budget-missing.json
    test \"\$task_status\" -eq 2
    grep -q \"missing_dependency\" /tmp/budget-missing.json
fi
sha256sum target/debug/save'" < "$evidence_dir/build.tar" 2>&1 | tee "$evidence_dir/cally.log"
printf 'Cally passed; source identity: '
cat "$evidence_dir/source.sha256"
