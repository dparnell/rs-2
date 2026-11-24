# Sourced: a disk with the toolchain and the committed sources, and a run of
# rs2 that builds on it. Needs $here, $root (the current directory) and $t
# (the test's own directory). Then `say COMMAND` (a line of build.cmd, run
# in \rs2) and `build`. Changes work/disk.txt and puts it back.
rm -rf "$t"
mkdir -p "$t/rs2"
userland64/rustc/sdk.sh "$t/sdk" > /dev/null
git archive HEAD Cargo.toml Cargo.lock .cargo boot kernel crates tools xtask userland userland-std userland64 dist | tar xf - -C "$t/rs2"
(cd "$t/rs2" && cargo vendor --locked -s userland/Cargo.toml -s userland-std/Cargo.toml vendor > /dev/null 2>&1)
printf '\n[source.crates-io]\nreplace-with = "vendored"\n[source.vendored]\ndirectory = "vendor"\n' >> "$t/rs2/.cargo/config.toml"
sed -e 's|^CC_i686.*|CC_i686_unknown_linux_gnu = "E:/sdk/bin/CLANG.EXE --target=i686-unknown-linux-gnu -ffreestanding -fno-stack-protector -isystem E:/sdk/lib/rs2-32/include"|' \
    -e 's|^AR_i686.*|AR_i686_unknown_linux_gnu = "E:/sdk/bin/LLVM-AR.EXE"|' userland-std/.cargo/config.toml > "$t/rs2/userland-std/.cargo/config.toml"
cmd=$t/build.cmd
printf '@echo off\r\nE:\r\ncd \\rs2\r\nset PATH=E:\\sdk\\bin;%%PATH%%\r\nset BEGINLIBPATH=E:\\sdk\\bin\r\nset TMPDIR=E:\\tmp\r\nset CARGO_HOME=E:\\cargo\r\nset CARGO_INCREMENTAL=0\r\nset CARGO_NET_OFFLINE=true\r\n' > "$cmd"
say() { printf '%s\r\n' "$1" >> "$cmd"; }
# userland-std's config names shell scripts for these (and for the C compiler
# above, as CMD makes variables' names upper case).
say 'set CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=rs2-ld.exe'
say 'set CARGO_TARGET_I686_UNKNOWN_LINUX_GNU_LINKER=rs2-ld32.exe'
say 'echo begin'
# QEMU is stopped when the build has been over for a while: the CPU time it
# uses in two minutes is then a few seconds.
cputime() { ps -o time= -p "$1" | tr ':' ' ' | awk '{ for (i = 1; i <= NF; i++) t = t * 60 + $i; print int(t) }'; }
stop_idle() {
    sleep 300
    while p=$(pgrep -f "qemu-system-x86_64 .*$t/d.img"); do
        a=$(cputime "$p")
        sleep 120
        b=$(cputime "$p")
        [ -z "$b" ] || [ $((b - a)) -ge 15 ] || kill "$p"
    done
}
build() {
    say 'echo end'
    [ ! -f work/disk.txt ] || cp work/disk.txt "$t/disk.save"
    trap '[ ! -f $t/disk.save ] || cp $t/disk.save work/disk.txt' EXIT
    {
        cat "$t/sdk.txt"
        echo "$here/on-rs2.sh -> tmp\\keep"
        echo "$cmd -> build.cmd"
        (cd "$t" && find rs2 -type f) | while read -r f; do printf '%s -> %s\n' "$t/$f" "$(echo "$f" | tr / '\\')"; done
    } > work/disk.txt
    cargo xtask disk --hpfs --size 3500 -o "$t/d.img" > /dev/null
    stop_idle > /dev/null 2>&1 &
    cargo xtask run --headless --timeout "${TIMEOUT:-14000}" --mem 8G --smp "${SMP:-8}" --hdd "$t/d.img" --init 'OS2\CMD.EXE /C E:\build.cmd' > "$t/log" 2>&1
    sed -n '/^begin/,$p' "$t/log" | tr '\r' '\n' | grep -av Building
    grep -aq '^end' "$t/log"
    if grep -aqE '^error|^build failed' "$t/log"; then exit 1; fi
}
