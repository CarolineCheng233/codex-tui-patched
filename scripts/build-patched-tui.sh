#!/bin/sh
# Build an isolated local Codex package with the companion Code Mode host.
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
target_dir="$repo_root/codex-rs/target"
package_dir="$target_dir/codex-tui-package"

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

export CARGO_TARGET_DIR="$target_dir"
cargo build \
    --manifest-path "$repo_root/codex-rs/Cargo.toml" \
    --bin codex \
    --bin codex-code-mode-host

python3 "$repo_root/scripts/build_codex_package.py" \
    --target "$target" \
    --package-dir "$package_dir" \
    --force \
    --entrypoint-bin "$target_dir/debug/codex" \
    --code-mode-host-bin "$target_dir/debug/codex-code-mode-host" \
    --rg-bin "$rg_bin" \
    --zsh-bin "$zsh_bin"

test -x "$package_dir/bin/codex"
test -x "$package_dir/bin/codex-code-mode-host"
printf '%s\n' "Built local Codex TUI package at $package_dir"
