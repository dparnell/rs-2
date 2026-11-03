#!/bin/sh
# Build the DOS test programs and run each on the host with dosrun:
# every one must print its success line. Exit status: the failures.
set -e
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
"$here/build.sh"
(cd "$root" && cargo build -q --release -p dosrun)
dosrun="$root/target/release/dosrun"
fail=0
check() {
    prog=$1; want=$2; keys=$3; shift 3
    out=$(cd "$here/bin" && "$dosrun" --timeout 30 "$@" --keys "$keys" "$prog" 2>/dev/null || true)
    if printf '%s' "$out" | grep -q "$want" && ! printf '%s' "$out" | grep -q FAILED; then
        echo "PASS $prog"
    else
        echo "FAIL $prog"; fail=$((fail + 1))
    fi
}
# A program's sound (dosrun --wav) must have these tones
# (tools/scripts/doswav.py: CHANNEL:HZ:SECONDS).
checksnd() {
    prog=$1; want=$2; shift 2
    wav=${TMPDIR:-/tmp}/rs2-dossnd.$$.wav
    out=$(cd "$here/bin" && "$dosrun" --timeout 30 --wav "$wav" "$prog" 2>/dev/null || true)
    if printf '%s' "$out" | grep -q "$want" && python3 -I "$root/tools/scripts/doswav.py" "$wav" "$@" >/dev/null; then
        echo "PASS $prog (sound)"
    else
        echo "FAIL $prog (sound)"; fail=$((fail + 1))
    fi
    rm -f "$wav"
}
set +e
check HELLO.COM "File I/O OK" ""
check GFX.COM "GFX OK" "{wait}x{wait}y"
check DPMITEST.COM "DPMI test done" ""
check VESA.COM "VESA OK" "{wait}x"
check VESAHI.COM "VESAHI OK" "{wait}x{wait}x"
checksnd SBTEST.COM "SB OK" B:441:1.0
checksnd SB16TEST.COM "SB16 OK" L:1002:1.0 R:501:1.0
checksnd FMTEST.COM "FM OK" L:440:0.5 R:880:0.5
checksnd BEEP.COM "beep done" B:440:0.5 B:880:0.5
check DOSSET.COM "DOSSET RM=640 VER=20.45 BREAK=0" ""
check DOSSET.COM "DOSSET RM=512 VER=5.0 BREAK=1 FREE=31877" "" --set DOS_RMSIZE=512 --set DOS_VERSION=DOSSET.COM,5,0,255 --set DOS_BREAK=1
check PRINT.COM "PRINT OK" ""
# What PRINT.COM prints by its five ways is one job.
lpt=${TMPDIR:-/tmp}/rs2-dosprint.$$
(cd "$here/bin" && "$dosrun" --timeout 30 --lpt "$lpt" PRINT.COM >/dev/null 2>&1)
if [ "$(grep -c '^[1-5] ' "$lpt.1" 2>/dev/null)" = 5 ] && [ ! -e "$lpt.2" ]; then echo "PASS PRINT.COM job"; else echo "FAIL PRINT.COM job"; fail=$((fail + 1)); fi
rm -f "$lpt".*
check COMMAND.COM "rs2 DOS session, version" 'ver\nexit\n'
exit $fail
