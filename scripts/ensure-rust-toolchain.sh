#!/bin/sh
# Make a standard rustup installation available to non-interactive script shells.

if command -v cargo >/dev/null 2>&1; then
    return 0
fi

for cargo_dir in "${HOME:-}/.cargo/bin" /opt/homebrew/opt/rustup/bin /usr/local/opt/rustup/bin; do
    if [ -x "$cargo_dir/cargo" ]; then
        PATH="$cargo_dir:$PATH"
        export PATH
        return 0
    fi
done

printf '%s\n' "Rust cargo was not found. Install Rust or add cargo to PATH." >&2
return 1
