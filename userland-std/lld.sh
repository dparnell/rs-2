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
out= shared= m64= rs2= libs=
prev=
for a in "$@"; do
    [ "$prev" = "-o" ] && out="$a"
    case "$a" in
        -shared) shared=1 ;;
        --rs2-64) m64=1 ;;
        -*) ;;
        */librs2libc-*.rlib) rs2=1 ;;
        *.so) [ "$prev" != "-o" ] && [ -s "$a" ] && { rs2=1; libs="$libs $a"; } ;;
    esac
    prev="$a"
done
# Shared libraries are cdylibs that crates from crates.io ask for besides
# their rlib, never used on rs2: write an empty file. 64-bit ones with
# rs2libc in them, or in a DLL they use, are DLLs.
if [ -n "$shared" ] && [ -n "$out" ] && { [ -z "$m64" ] || [ -z "$rs2" ]; }; then
    : > "$out"
    exit 0
fi
# 64-bit programs and DLLs (.cargo/config.toml's marker --rs2-64):
# link64.ld, the start-up code (crt64.rs), and every undefined symbol must
# be an import, MODULE.name or MODULE.ordinal, or be in a DLL on the line.
if [ -n "$m64" ]; then
    tmp=$(mktemp -d "${TMPDIR:-/tmp}/lld64.XXXXXX")
    trap 'rm -rf "$tmp"' EXIT
    env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS rustc --edition 2024 --target x86_64-unknown-linux-gnu -O -C panic=abort -C relocation-model=pic \
        ${shared:+--cfg dll} --check-cfg 'cfg(dll)' --emit=obj -o "$tmp/crt.o" "$dir/crt64.rs" || exit
    # A DLL's name is its file's: libfoo.so or libfoo-HASH.so is FOO.
    name=$(basename "$out" | sed 's/^lib//; s/\.so$//; s/-[0-9a-f]\{8,\}$//' | tr '[:lower:]' '[:upper:]')
    n=$#
    while [ "$n" -gt 0 ]; do
        a=$1
        shift
        case "$a" in
            --rs2-64) ;;
            -soname=*) [ -n "$shared" ] && set -- "$@" "-soname=$name" ;;
            --version-script=*)
                if [ -n "$shared" ]; then
                    # What rustc exports, and what the loader looks for.
                    sed 's/^\( *\)global:/\1global: _DLL_InitTerm; RS2_MODULE; RS2_RESOURCES;/' "${a#*=}" > "$tmp/exports"
                    set -- "$@" "--version-script=$tmp/exports" --undefined-version
                else
                    set -- "$@" "$a"
                fi
                ;;
            --no-undefined-version) ;;
            *) set -- "$@" "$a" ;;
        esac
        n=$((n - 1))
    done
    "$lld" -flavor "$flavor" -L "$dir/syslib" -T "$dir/link64.ld" ${shared:+-soname=$name -Bsymbolic -u _DLL_InitTerm} "$tmp/crt.o" "$@" || exit
    nm=$(dirname "$lld")/llvm-nm
    [ -x "$nm" ] || nm=llvm-nm
    bad=$( { for l in $libs; do "$nm" -D --defined-only "$l" | awk '{ print "+", $NF }'; done
             "$nm" -D --undefined-only "$out" | awk '$1 == "U" { print "-", $2 }'; } |
        awk '$1 == "+" { has[$2] = 1; next } !($2 in has) && ($2 !~ /[.]/ || $2 ~ /^_/) { print "    " $2 }')
    if [ -n "$bad" ]; then
        echo "lld.sh: $out: not in rs2libc:" >&2
        echo "$bad" >&2
        rm -f "$out"
        exit 1
    fi
    exit 0
fi
exec "$lld" -flavor "$flavor" -L "$dir/syslib" -T "$dir/link.ld" "$@"
