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
    prog=$1; want=$2; keys=$3
    out=$(cd "$here/bin" && "$dosrun" --timeout 30 --keys "$keys" "$prog" 2>/dev/null || true)
    if printf '%s' "$out" | grep -q "$want" && ! printf '%s' "$out" | grep -q FAILED; then
        echo "PASS $prog"
    else
        echo "FAIL $prog"; fail=$((fail + 1))
    fi
}
set +e
check HELLO.COM "File I/O OK" ""
check GFX.COM "GFX OK" "{wait}x{wait}y"
check DPMITEST.COM "DPMI test done" ""
check VESA.COM "VESA OK" "{wait}x"
check SBTEST.COM "SB OK" ""
check BEEP.COM "beep done" ""
check COMMAND.COM "rs2 DOS session, version" 'ver\nexit\n'
exit $fail
