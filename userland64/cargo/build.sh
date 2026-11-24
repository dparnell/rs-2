#!/bin/sh
# cargo for rs2 (docs/SELF-HOSTING.md), from the rustc source that
# userland64/rustc/build.sh has unpacked (run that first).
#
#   build.sh      (results in target/cargo64: CARGO.EXE, RSCARGO.DLL)
set -e
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
std=$root/userland-std
export RUSTC_DIR=${RUSTC_DIR:-$root/work/rustc}
dir=$RUSTC_DIR
src=$dir/rustc-1.100.0-src
t=x86_64-unknown-linux-gnu
[ -f "$src/.rs2-patched" ] || { echo "build.sh: no $src: run userland64/rustc/build.sh first" >&2; exit 1; }
if command -v sha256sum > /dev/null; then sha=sha256sum; else sha="shasum -a 256"; fi
patch=$($sha < "$here/rs2.patch")
# cargo and the crates for its C libraries, patched.
c=$dir/cargo
if [ "$(cat "$c/.rs2-patched" 2> /dev/null)" != "$patch" ]; then
    rm -rf "${dir:?}/cargo"
    mkdir -p "$c"
    cp -R "$src/src/tools/cargo" "$c/cargo"
    for n in curl-0.4.50 curl-sys-0.4.90+curl-8.21.0 libgit2-sys-0.18.7+1.9.6 libsqlite3-sys-0.38.1 libz-sys-1.1.29; do
        cp -R "$src/vendor/$n" "$c/${n%%-[0-9]*}"
    done
    # The C libraries are in RSCARGO.DLL: nothing to build for them.
    for n in curl-sys libgit2-sys libz-sys; do echo 'fn main() {}' > "$c/$n/build.rs"; done
    cat > "$c/libsqlite3-sys/build.rs" <<'EOF'
fn main() {
    let out = std::env::var_os("OUT_DIR").unwrap();
    std::fs::copy("sqlite3/bindgen_bundled_version.rs", std::path::Path::new(&out).join("bindgen.rs")).unwrap();
}
EOF
    (cd "$c" && patch -p1 < "$here/rs2.patch" && echo "$patch" > .rs2-patched)
fi
mkdir -p "$c/.cargo"
printf '[source.crates-io]\nreplace-with = "v"\n[source.v]\ndirectory = "../rustc-1.100.0-src/vendor"\n' > "$c/.cargo/config.toml"
ln -sfn "$std/rs2libc" "$dir/rs2libc"
export CC_x86_64_unknown_linux_gnu="$std/cc64.sh" AR_x86_64_unknown_linux_gnu="$std/ar.sh" CARGO_TARGET_DIR="$dir/cargo-build"
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="$std/linker64.sh"
CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS=$(sed -n '/^\[target.x86_64/,/^\]/p' "$std/.cargo/config.toml" |
    sed -n 's/^ *"\(.*\)",$/\1/p' | sed 's/", "/ /g' | tr '\n' ' ')
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS
(cd "$c/cargo" && cargo build --release --offline --target $t --lib)
# What its crates import from the C libraries.
bin=$(ls -d "$(rustc --print sysroot)"/lib/rustlib/*/bin | head -1)
find "$dir/cargo-build/$t/release" -name 'lib*.rlib' | xargs "$bin/llvm-nm" --undefined-only 2> /dev/null |
    awk '$1 == "U" && $2 ~ /^(git_|curl_|sqlite3_)/ { print $2 }' | sort -u > "$dir/cargo-imports"
mkdir -p "$root/target/cargo64"
"$here/rscargo.sh" "$dir/cargo-imports" "$root/target/cargo64/RSCARGO.DLL"
cp "$root/target/cargo64/RSCARGO.DLL" "$dir/librscargo.so"
(cd "$c/cargo" && cargo rustc --release --offline --target $t --bin cargo -- -C link-arg="$dir/librscargo.so")
cp "$dir/cargo-build/$t/release/cargo" "$root/target/cargo64/CARGO.EXE"
ls -l "$root/target/cargo64"
