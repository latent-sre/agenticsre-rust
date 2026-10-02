#!/usr/bin/env bash
# Run local checks without network access, host credentials or writable source.
set -euo pipefail
IFS=$'\n\t'

repo_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
compiler_path=$(rustup which rustc)
toolchain_dir=$(dirname -- "$(dirname -- "$compiler_path")")
cargo_registry=${CARGO_HOME:-${HOME}/.cargo}/registry
if [[ ! -d "$cargo_registry" ]]; then
    printf '%s\n' 'Run cargo fetch --locked before offline sandbox checks.' >&2
    exit 2
fi
command -v bwrap >/dev/null
scratch_dir=$(mktemp -d /tmp/agenticsre-poc-sandbox.XXXXXXXX)
trap 'rm -rf -- "${scratch_dir:?}"' EXIT
mkdir -p -- "$scratch_dir/cargo" "$repo_dir/target"
mounts=(--ro-bind /usr /usr --ro-bind /lib /lib --ro-bind /lib64 /lib64
    --ro-bind /etc/alternatives /etc/alternatives
    --symlink usr/bin /bin --symlink usr/sbin /sbin
    --proc /proc --dev /dev --tmpfs /tmp
    --ro-bind "$toolchain_dir" /toolchain
    --bind "$scratch_dir/cargo" /tmp/cargo
    --ro-bind "$cargo_registry" /tmp/cargo/registry
    --ro-bind "$repo_dir" "$repo_dir"
    --bind "$repo_dir/target" "$repo_dir/target")
if [[ -x "$repo_dir/.venv/bin/python" ]]; then
    python_path=$(readlink -f -- "$repo_dir/.venv/bin/python")
    python_dir=$(dirname -- "$(dirname -- "$python_path")")
    mounts+=(--ro-bind "$python_dir" "$python_dir")
    python_link=$(readlink -- "$repo_dir/.venv/bin/python")
    if [[ "$python_link" = /* ]]; then
        python_alias_dir=$(dirname -- "$(dirname -- "$python_link")")
        if [[ "$python_alias_dir" != "$python_dir" ]]; then
            mounts+=(--ro-bind "$python_dir" "$python_alias_dir")
        fi
    fi
fi
if [[ $# -eq 0 ]]; then
    set -- cargo test --workspace --all-features --locked --offline
fi
bwrap --unshare-user --unshare-net --unshare-pid --unshare-ipc --unshare-uts \
    --die-with-parent --new-session --cap-drop ALL --clearenv \
    "${mounts[@]}" --chdir "$repo_dir" \
    --setenv PATH /toolchain/bin:/usr/bin:/bin \
    --setenv CARGO_HOME /tmp/cargo --setenv CARGO_TARGET_DIR "$repo_dir/target" \
    --setenv CARGO_BUILD_JOBS 2 --setenv RUST_TEST_THREADS 2 --setenv CARGO_INCREMENTAL 0 \
    --setenv CARGO_PROFILE_DEV_DEBUG 0 --setenv CARGO_PROFILE_TEST_DEBUG 0 \
    --setenv LANG C.UTF-8 --setenv LC_ALL C.UTF-8 --setenv TZ UTC \
    --setenv CARGO_NET_OFFLINE true -- "$@"
