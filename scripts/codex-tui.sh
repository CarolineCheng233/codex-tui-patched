#!/bin/sh
# Start only the isolated local package; never replace the system `codex` command.
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
package_dir="$script_dir/../codex-rs/target/codex-tui-package"
codex_bin="$package_dir/bin/codex"
host_bin="$package_dir/bin/codex-code-mode-host"

if [ ! -x "$codex_bin" ] || [ ! -x "$host_bin" ]; then
    printf '%s\n' "Local Codex TUI package is missing. Run: $script_dir/build-patched-tui.sh" >&2
    exit 1
fi

exec "$codex_bin" "$@"
