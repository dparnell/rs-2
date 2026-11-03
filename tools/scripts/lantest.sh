#!/bin/sh
# Run the LAN requester tests in QEMU against tools/scripts/smbhost.py:
# RS2\LANTEST.EXE (API checks) and RS2\LANTEST.CMD (NET, DIR, COPY, XCOPY,
# MD/RD, REN, ATTRIB, DEL on a network drive and by UNC name, printing to
# a redirected LPT3), then compare the files the guest left on the share.
#
# work/modules.txt needs (see docs/LAN.md):
#   tools/lantest/IBMLAN.INI -> IBMLAN\IBMLAN.INI
#   tools/lantest/LMHOSTS -> MPTN\ETC\LMHOSTS
#   tools/lantest/LANTEST.CMD -> RS2\LANTEST.CMD
#
# The guest reaches the host as 10.0.2.2 (QEMU user networking); the
# server listens on 4445 (SMB), 4139 (NetBIOS session) and UDP 4137 (NBNS),
# which IBMLAN.INI points the requester at.
# usage: tools/scripts/lantest.sh [seconds] [extra xtask options]
secs=${1:-300}
[ $# -gt 0 ] && shift
root=$(cd "$(dirname "$0")/../.." && pwd)
dir=${LANTEST_DIR:-$root/work/lantest}
rm -rf "$dir"
mkdir -p "$dir/data" "$dir/spool"
python3 -I "$root/tools/scripts/lantest_check.py" prepare "$dir/data"
python3 -I "$root/tools/scripts/smbhost.py" --share "DATA=$dir/data" --printer "LPT=$dir/spool" \
  --user rs2:secret --ip 10.0.2.2 --bind 127.0.0.1 > "$dir/smbhost.log" 2>&1 &
srv=$!
trap 'kill $srv 2>/dev/null' EXIT INT TERM
cd "$root" || exit 1
cargo xtask run --headless --timeout "$secs" --net --init "OS2\\CMD.EXE /C C:\\RS2\\LANTEST.CMD" "$@" > "$dir/guest.log" 2>&1
grep -a -E "PASS|FAIL|LANTEST|===|SYS[0-9]|NET[0-9]" "$dir/guest.log"
python3 -I "$root/tools/scripts/lantest_check.py" check "$dir/data" "$dir/spool"
