#!/bin/sh
# rustc for rs2 (docs/SELF-HOSTING.md): cross-built with the installed
# rustc of the same version, as a 64-bit program with std (userland-std),
# and its LLVM as a DLL.
#
#   build.sh      (results in target/rustc64: RUSTC.EXE, RSLLVM.DLL)
#
# Needs CMake, Ninja, clang and about 20 GB. The source comes from
# static.rust-lang.org into $RUSTC_DIR (work/rustc), where the builds go
# too; it has to be the installed rustc's, whose std it uses.
set -e
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
std=$root/userland-std
export RUSTC_DIR=${RUSTC_DIR:-$root/work/rustc}
dir=$RUSTC_DIR
ver=1.100.0
tarsha=90dab90b8084eb57c35ae2b1d0b70c8e5138ace6023b22c60a5a400f586f63b2
src=$dir/rustc-$ver-src
t=x86_64-unknown-linux-gnu
v=$(rustc -V)
hash=$(rustc -vV | sed -n 's/^commit-hash: //p')
[ "$(echo "$v" | cut -d' ' -f2)" = $ver ] || { echo "build.sh: needs rustc $ver, not $v" >&2; exit 1; }
mkdir -p "$dir"
if command -v sha256sum > /dev/null; then sha=sha256sum; else sha="shasum -a 256"; fi
patch=$($sha < "$here/rs2.patch")
if [ "$(cat "$src/.rs2-patched" 2> /dev/null)" != "$patch" ]; then
    tar=$dir/rustc-src.tar.xz
    [ -f "$tar" ] || curl -fL -o "$tar" "https://static.rust-lang.org/dist/rustc-$ver-src.tar.xz"
    sum=$($sha < "$tar")
    [ "${sum%% *}" = $tarsha ] || { echo "build.sh: $tar does not have SHA-256 $tarsha" >&2; exit 1; }
    rm -rf "$src"
    (cd "$dir" && tar xf rustc-src.tar.xz)
    (cd "$src" && patch -p1 < "$here/rs2.patch" && echo "$patch" > .rs2-patched)
fi
# rustc-main depends on it as ../../../rs2libc.
ln -sfn "$std/rs2libc" "$dir/rs2libc"
"$here/llvm.sh"

# The compiler's crates. What bootstrap sets:
export RUSTC_BOOTSTRAP=1 CFG_RELEASE=$ver CFG_RELEASE_NUM=$ver CFG_RELEASE_CHANNEL=stable CFG_VERSION="${v#rustc }" \
    CFG_VER_HASH="$hash" CFG_VER_DATE="$(rustc -vV | sed -n 's/^commit-date: //p')" CFG_COMPILER_HOST_TRIPLE=$t \
    RUSTC_INSTALL_BINDIR=bin CFG_LIBDIR_RELATIVE=lib CFG_DISABLE_UNSTABLE_FEATURES=1 CFG_DEFAULT_CODEGEN_BACKEND=llvm RS2_LLVM=1
export CC_x86_64_unknown_linux_gnu="$std/cc64.sh" AR_x86_64_unknown_linux_gnu="$std/ar.sh" CARGO_TARGET_DIR="$dir/build"
# How to link: as userland-std/.cargo/config.toml.
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="$std/linker64.sh"
CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS=$(sed -n '/^\[target.x86_64/,/^\]/p' "$std/.cargo/config.toml" |
    sed -n 's/^ *"\(.*\)",$/\1/p' | sed 's/", "/ /g' | tr '\n' ' ')
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS
cargo_rustc() {
    (cd "$src" && cargo rustc --release --offline --target $t "$@")
}
features="--features llvm,max_level_info"
(cd "$src" && cargo build --release --offline --target $t -p rustc_driver_impl $features)
# What they import from LLVM.
bin=$(ls -d "$(rustc --print sysroot)"/lib/rustlib/*/bin | head -1)
find "$dir/build/$t/release" -name 'librustc_codegen_llvm-*.rlib' -o -name 'librustc_llvm-*.rlib' -o -name 'librustc_codegen_ssa-*.rlib' |
    xargs "$bin/llvm-nm" --undefined-only 2> /dev/null | awk '$1 == "U" && $2 ~ /^LLVM/ { print $2 }' | sort -u > "$dir/llvm-imports"
mkdir -p "$root/target/rustc64"
"$here/rsllvm.sh" "$dir/llvm-imports" "$root/target/rustc64/RSLLVM.DLL"
cp "$root/target/rustc64/RSLLVM.DLL" "$dir/librsllvm.so"
cargo_rustc -p rustc-main --bin rustc-main $features -- -C link-arg="$dir/librsllvm.so"
cp "$dir/build/$t/release/rustc-main" "$root/target/rustc64/RUSTC.EXE"
cp "$src/COPYRIGHT" "$src/LICENSE-APACHE" "$src/LICENSE-MIT" "$root/target/rustc64/"
ls -l "$root/target/rustc64"
