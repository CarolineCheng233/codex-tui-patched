#!/bin/sh
# Build an isolated local Codex package with the companion Code Mode host.
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
target_dir="$repo_root/codex-rs/target"
package_dir="$target_dir/codex-tui-package"
python_bin=${PYTHON:-}

# shellcheck source=ensure-rust-toolchain.sh
. "$script_dir/ensure-rust-toolchain.sh"

if [ -z "$python_bin" ]; then
    for candidate in python3.13 python3.12 python3.11 python3.10 python3; do
        if command -v "$candidate" >/dev/null 2>&1 && "$candidate" -c 'import sys; raise SystemExit(sys.version_info < (3, 10))'; then
            python_bin=$(command -v "$candidate")
            break
        fi
    done
fi

if [ -z "$python_bin" ] || ! "$python_bin" -c 'import sys; raise SystemExit(sys.version_info < (3, 10))'; then
    printf '%s\n' "Python 3.10 or newer is required to build the local Codex package." >&2
    exit 1
fi

case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) target="aarch64-apple-darwin" ;;
    Darwin-x86_64) target="x86_64-apple-darwin" ;;
    *)
        printf '%s\n' "This launcher currently packages only macOS targets." >&2
        exit 1
        ;;
esac

rg_bin=$(command -v rg) || {
    printf '%s\n' "rg is required to build the local Codex package." >&2
    exit 1
}
zsh_bin=$(command -v zsh) || {
    printf '%s\n' "zsh is required to build the local Codex package." >&2
    exit 1
}

if [ -n "${CODEX_CODE_MODE_HOST_BIN:-}" ]; then
    code_mode_host_bin=$CODEX_CODE_MODE_HOST_BIN
else
    installed_codex_bin=$(command -v codex) || {
        printf '%s\n' "Set CODEX_CODE_MODE_HOST_BIN to an official codex-code-mode-host binary." >&2
        exit 1
    }
    installed_codex_bin=$("$python_bin" -c 'import os, sys; print(os.path.realpath(sys.argv[1]))' "$installed_codex_bin")
    code_mode_host_bin=$(dirname -- "$installed_codex_bin")/codex-code-mode-host
fi

if [ ! -x "$code_mode_host_bin" ]; then
    printf '%s\n' "An executable official codex-code-mode-host is required: $code_mode_host_bin" >&2
    exit 1
fi

"$code_mode_host_bin" --help >/dev/null

export CARGO_TARGET_DIR="$target_dir"
cargo build \
    --manifest-path "$repo_root/codex-rs/Cargo.toml" \
    --release \
    --bin codex

CODEX_REPO_ROOT="$repo_root" "$python_bin" "$repo_root/scripts/build_codex_package.py" \
    --target "$target" \
    --package-dir "$package_dir" \
    --force \
    --entrypoint-bin "$target_dir/release/codex" \
    --code-mode-host-bin "$code_mode_host_bin" \
    --rg-bin "$rg_bin" \
    --zsh-bin "$zsh_bin"

test -x "$package_dir/bin/codex"
test -x "$package_dir/bin/codex-code-mode-host"
printf '%s\n' "$code_mode_host_bin" > "$package_dir/CODE_MODE_HOST_SOURCE"
printf '%s\n' "Built local Codex TUI package at $package_dir"
