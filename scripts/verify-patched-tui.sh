#!/bin/sh
# Verify the isolated patched package and all deterministic workspace behavior.
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
package_dir="$repo_root/codex-rs/target/codex-tui-package"
codex_bin="$package_dir/bin/codex"
host_bin="$package_dir/bin/codex-code-mode-host"
host_source="$package_dir/CODE_MODE_HOST_SOURCE"

# shellcheck source=ensure-rust-toolchain.sh
. "$script_dir/ensure-rust-toolchain.sh"

if [ ! -x "$codex_bin" ] || [ ! -x "$host_bin" ] || [ ! -s "$host_source" ]; then
    printf '%s\n' "Local package is missing. Run: $script_dir/build-patched-tui.sh" >&2
    exit 1
fi

cd "$repo_root/codex-rs"
just test -p codex-config tui_transcript_workspace_can_be_enabled_from_config
just test -p codex-tui \
    transcript_workspace_routes_typing_and_ctrl_c_to_the_existing_composer \
    workspace_turn_keys_select_collapse_and_expand_a_whole_turn \
    workspace_wheel_scrolls_only_the_transcript \
    workspace_places_local_input_images_in_reserved_user_rows \
    local_input_preview_
just test -p codex-install-context code_mode_host_program_uses_package_resource_without_legacy_binary

"$codex_bin" --version
"$host_bin" --help >/dev/null
printf '%s\n' "Patched Codex TUI verification passed."
