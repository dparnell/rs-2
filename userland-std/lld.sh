#!/bin/sh
# The linker for userland-std: rustup's rust-lld with link.ld and the empty
# system libraries of syslib/ (std's link line names -lc, -lpthread...),
# found from this script's directory whatever directory rustc links in
# (crates from crates.io link their own cdylibs from theirs).
dir=$(cd "$(dirname "$0")" && pwd)
lld="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/^host: //p')/bin/rust-lld"
if [ "$1" = "-flavor" ]; then
    flavor="$2"
    shift 2
else
    flavor=gnu
fi
# Shared libraries (cdylibs that crates ask for besides their rlib) are
# never used on rs2, and static code cannot make one: write an empty file.
out=
prev=
for a in "$@"; do
    [ "$prev" = "-o" ] && out="$a"
    prev="$a"
done
for a in "$@"; do
    if [ "$a" = "-shared" ] && [ -n "$out" ]; then
        : > "$out"
        exit 0
    fi
done
# 64-bit programs (.cargo/config.toml's marker --rs2-64): link64.ld, and
# every undefined symbol must be an import, MODULE.name or MODULE.ordinal.
for a in "$@"; do
    [ "$a" = --rs2-64 ] || continue
    n=$#
    while [ "$n" -gt 0 ]; do
        a=$1
        shift
        [ "$a" = --rs2-64 ] || set -- "$@" "$a"
        n=$((n - 1))
    done
    "$lld" -flavor "$flavor" -L "$dir/syslib" -T "$dir/link64.ld" "$@" || exit
    nm=$(dirname "$lld")/llvm-nm
    [ -x "$nm" ] || nm=llvm-nm
    bad=$("$nm" -D --undefined-only "$out" | awk '$1 == "U" && $2 !~ /[.]/ { print "    " $2 }')
    if [ -n "$bad" ]; then
        echo "lld.sh: $out: not in rs2libc:" >&2
        echo "$bad" >&2
        rm -f "$out"
        exit 1
    fi
    exit 0
done
exec "$lld" -flavor "$flavor" -L "$dir/syslib" -T "$dir/link.ld" "$@"
