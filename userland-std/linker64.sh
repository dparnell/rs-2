#!/bin/sh
# The linker for x86_64-unknown-linux-gnu (.cargo/config.toml). On an x86-64
# Linux host that is the host's triple too, and cargo links build scripts
# and proc macros with the same `linker`, but without the rustflags: what
# has no --rs2-64 marker is the host's, for cc. (The name must not be lld's:
# rustc takes the host's linker flavor from it.)
for a in "$@"; do
    [ "$a" = "--rs2-64" ] && exec "$(dirname "$0")/lld.sh" "$@"
done
exec cc "$@"
