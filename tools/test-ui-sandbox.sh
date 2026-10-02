#!/usr/bin/env bash
# Run Node, Rust and browser checks with private credentials, network and scratch.
set -euo pipefail
IFS=$'\n\t'
repo_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
node_binary=$(readlink -f -- "$(command -v node)")
node_dir=$(dirname -- "$(dirname -- "$node_binary")")
compiler_path=$(rustup which rustc)
toolchain_dir=$(dirname -- "$(dirname -- "$compiler_path")")
cargo_registry=${CARGO_HOME:-${HOME}/.cargo}/registry
browser_venv=/tmp/dualla-dashboard-preview-venv
browser_dir=/tmp/dualla-playwright/chromium_headless_shell-1243/chrome-headless-shell-linux64
browser_libraries=/tmp/dualla-browser-libs/root
scratch_dir=$(mktemp -d /tmp/agenticsre-ui-sandbox.XXXXXXXX)
cleanup() {
    local run_status=$?
    trap - EXIT
    if ! rm -rf -- "${scratch_dir:?}"; then
        printf 'Could not remove UI check scratch: %s\n' "$scratch_dir" >&2 || :
        if (( run_status == 0 )); then run_status=1; fi
    fi
    exit "$run_status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
if (( $# == 0 )); then
    printf '%s\n' 'Usage: tools/test-ui-sandbox.sh COMMAND [ARGUMENT ...]' >&2
    exit 2
fi
mkdir -p -- "$scratch_dir/cargo" "$scratch_dir/npm" "$scratch_dir/output" "$repo_dir/target/ui" \
    "$repo_dir/web/dist" "$repo_dir/web/src/generated" "$repo_dir/web/node_modules/.cache" \
    "$repo_dir/web/node_modules/.vite" "$repo_dir/web/node_modules/.vite-temp"
# A synthetic account supplies native home-directory lookup; no host passwd or home is exposed.
printf 'preview:x:%s:%s:Workbench check:/tmp/check-home:/usr/sbin/nologin\n' "$(id -u)" "$(id -g)" \
    > "$scratch_dir/passwd"
mounts=(--ro-bind /usr /usr --ro-bind /lib /lib --ro-bind /lib64 /lib64
    --ro-bind /etc/alternatives /etc/alternatives --ro-bind /etc/fonts /etc/fonts
    --ro-bind /etc/ld.so.cache /etc/ld.so.cache --symlink usr/bin /bin --symlink usr/sbin /sbin
    --proc /proc --dev /dev --tmpfs /tmp --dir /tmp/check-home
    --ro-bind "$scratch_dir/passwd" /etc/passwd
    --ro-bind "$node_dir" /node --ro-bind "$toolchain_dir" /toolchain
    --bind "$scratch_dir/cargo" /tmp/cargo --ro-bind "$cargo_registry" /tmp/cargo/registry
    --bind "$scratch_dir/npm" /tmp/npm-cache
    --ro-bind "$repo_dir" "$repo_dir" --bind "$repo_dir/target" "$repo_dir/target")
for output_dir in web/dist web/src/generated web/node_modules/.cache web/node_modules/.vite web/node_modules/.vite-temp; do
    mounts+=(--bind "$repo_dir/$output_dir" "$repo_dir/$output_dir")
done
python_mount() {
    local executable=$1
    local resolved parent alias
    resolved=$(readlink -f -- "$executable")
    parent=$(dirname -- "$(dirname -- "$resolved")")
    mounts+=(--ro-bind "$parent" "$parent")
    alias=$(readlink -- "$executable")
    if [[ "$alias" = /* ]]; then
        alias=$(dirname -- "$(dirname -- "$alias")")
        if [[ "$alias" != "$parent" ]]; then mounts+=(--ro-bind "$parent" "$alias"); fi
    fi
}
if [[ -x "$repo_dir/.venv/bin/python" ]]; then python_mount "$repo_dir/.venv/bin/python"; fi
browser_env=()
if [[ -x "$browser_venv/bin/python" && -x "$browser_dir/chrome-headless-shell" && -d "$browser_libraries" ]]; then
    mounts+=(--ro-bind "$browser_venv" "$browser_venv" --ro-bind "$browser_dir" /browser
        --ro-bind "$browser_libraries" /browser-libs)
    python_mount "$browser_venv/bin/python"
    browser_env+=(--setenv WORKBENCH_BROWSER_PYTHON "$browser_venv/bin/python"
        --setenv WORKBENCH_BROWSER_EXECUTABLE /browser/chrome-headless-shell
        --setenv LD_LIBRARY_PATH /browser-libs/usr/lib/x86_64-linux-gnu)
fi
bwrap --unshare-user --unshare-net --unshare-pid --unshare-ipc --unshare-uts \
    --die-with-parent --new-session --cap-drop ALL --clearenv "${mounts[@]}" \
    --chdir "$repo_dir" --setenv PATH /node/bin:/toolchain/bin:/usr/bin:/bin \
    --setenv LANG C.UTF-8 --setenv LC_ALL C.UTF-8 --setenv TZ UTC \
    --setenv CARGO_HOME /tmp/cargo --setenv CARGO_TARGET_DIR "$repo_dir/target" \
    --setenv CARGO_BUILD_JOBS 2 --setenv RUST_TEST_THREADS 2 --setenv CARGO_INCREMENTAL 0 \
    --setenv CARGO_PROFILE_DEV_DEBUG 0 --setenv CARGO_PROFILE_TEST_DEBUG 0 --setenv CARGO_NET_OFFLINE true \
    --setenv npm_config_cache /tmp/npm-cache --setenv npm_config_offline true \
    --setenv WORKBENCH_UI_OUTPUT "$repo_dir/target/ui" "${browser_env[@]}" \
    -- /usr/bin/timeout 900 "$@"
