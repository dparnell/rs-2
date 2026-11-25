#!/bin/sh
# The developer disk (docs/DEVDISK.md): a CD image with the compilers that
# run on rs2 (userland64/{llvm,rustc,cargo}/build.sh have to have run; their
# results in target/ are enough), samples, documentation, rs2's sources with
# their dependencies, and the installers.
#
#   build.sh [ISO]            (work/rs2-dev.iso by default)
#
# Needs python3 (for the archives), git and, for cargo vendor, the network.
# The sources are the last commit's.
set -e
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
cd "$root"
iso=${1:-$root/work/rs2-dev.iso}
t=$root/target/devdisk
for f in llvm64/CLANG.EXE llvm64/LLD.EXE llvm64/LLVM-AR.EXE llvm64/clang/version rustc64/RUSTC.EXE rustc64/RSLLVM.DLL rustc64/LICENSE-MIT \
    cargo64/CARGO.EXE cargo64/RSCARGO.DLL; do
    [ -f "target/$f" ] || { echo "build.sh: no target/$f: run userland64/{llvm,rustc,cargo}/build.sh first" >&2; exit 1; }
done
rm -rf "$t"
mkdir -p "$t/sdk" "$t/source/src" "$t/disk"
commit=$(git rev-parse --short HEAD)
t64=x86_64-unknown-linux-gnu

# The compilers, and the resource compiler.
userland64/rustc/sdk.sh "$t/sdk/sdk" > /dev/null
(cd userland-std && cargo build -q --release --target $t64 -p rc64)
bin=$(ls -d "$(rustc --print sysroot)"/lib/rustlib/*/bin | head -1)
"$bin/llvm-objcopy" --strip-all "userland-std/target/$t64/release/rc64" "$t/sdk/sdk/bin/RC64.EXE"
for o in x86_64-unknown-none x86_64-unknown-uefi i686-unknown-linux-gnu; do
    [ -d "$t/sdk/sdk/lib/rustlib/$o" ] || { echo "build.sh: rustup target add $o" >&2; exit 1; }
done
# The OS/2 API for Rust programs.
c=$t/sdk/sdk/crates
mkdir -p "$c"
git archive HEAD userland64/os2 | tar xf - -C "$c" --strip-components=1
git archive HEAD crates/os2-rc crates/os2-exe | tar xf - -C "$c" --strip-components=1
printf '[workspace]\nresolver = "3"\nmembers = ["os2", "os2-rc", "os2-exe"]\n\n[workspace.package]\nedition = "2024"\nversion = "0.1.0"\nlicense = "MIT OR Apache-2.0"\n' > "$c/Cargo.toml"
# Samples, documentation, licences.
cp -R "$here/files/." "$t/sdk/"
cp userland64/c/samples/pmhello.c userland64/c/samples/pmhello.rc "$t/sdk/samples/c/"
mkdir -p "$t/sdk/samples/rust/pmhello/src"
grep -v 'use rs2libc as _;' userland-std/pmstd64/src/main.rs > "$t/sdk/samples/rust/pmhello/src/main.rs"
mkdir -p "$t/sdk/docs" "$t/sdk/licenses/llvm" "$t/sdk/licenses/rust"
for f in DEVDISK API64 LIBC64 CXX64 STD64 SELF-HOSTING; do cp "docs/$f.md" "$t/sdk/docs/"; done
cp target/llvm64/LICENSE.TXT "$t/sdk/licenses/llvm/"
cp target/rustc64/COPYRIGHT target/rustc64/LICENSE-* "$t/sdk/licenses/rust/"
echo "rs2 $commit, $(rustc -V)" > "$t/sdk/VERSION.TXT"

# rs2's sources, with what the build would download.
s=$t/source/src
git archive HEAD Cargo.toml Cargo.lock README.md .cargo boot kernel crates tools xtask userland userland-std userland64 dist docs | tar xf - -C "$s"
echo "$commit" > "$s/.rs2-commit"
(cd "$s" && cargo vendor -q --locked -s userland/Cargo.toml -s userland-std/Cargo.toml vendor > /dev/null)
printf '\n[source.crates-io]\nreplace-with = "vendored"\n[source.vendored]\ndirectory = "vendor"\n' >> "$s/.cargo/config.toml"
cargo xtask fetch
for d in cache firmware; do
    [ ! -d "work/$d" ] || { mkdir -p "$s/work" && cp -RL "work/$d" "$s/work/"; }
done
cp "$here/files-src/"* "$s/"
# userland-std's configuration names shell scripts: the tools by name.
cc=$s/userland-std/.cargo/config.toml
sed -e 's|"\./lld\.sh"|"rs2-ld32.exe"|; s|"\./linker64\.sh"|"rs2-ld.exe"|' \
    -e 's|^CC_i686.*|CC_i686_unknown_linux_gnu = "@UDIR@/sdk/bin/CLANG.EXE --target=i686-unknown-linux-gnu -ffreestanding -fno-stack-protector -isystem @UDIR@/sdk/lib/rs2-32/include"|' \
    -e 's|^CC_x86_64.*|CC_x86_64_unknown_linux_gnu = "@UDIR@/sdk/bin/CLANG.EXE --target=x86_64-unknown-linux-gnu -ffreestanding -fno-stack-protector -isystem @UDIR@/sdk/lib/rs2-32/include"|' \
    -e 's|^AR_\([a-z0-9_]*\) = .*|AR_\1 = "@UDIR@/sdk/bin/LLVM-AR.EXE"|' "$cc" > "$cc.rs2in"
rm "$cc"
[ "$(grep -c '\.sh"' "$cc.rs2in")" = 0 ] || { echo "build.sh: shell scripts left in $cc" >&2; exit 1; }

python3 -I "$here/pack.py" "$t/sdk" "$t/disk/SDK.ZIP" "$(git log -1 --format=%ct)"
python3 -I "$here/pack.py" "$t/source" "$t/disk/SOURCE.ZIP" "$(git log -1 --format=%ct)"
sed 's/$/\r/' "$here/README.TXT" > "$t/disk/README.TXT"
cargo xtask devdisk "$t/disk" -o "$iso"
ls -l "$t/disk"
