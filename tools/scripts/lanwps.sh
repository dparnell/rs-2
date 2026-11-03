#!/bin/sh
# The Network folder in QEMU against smbhost.py (docs/LAN.md): NET USE Y:,
# then open Network, WORKGROUP, \\SMBHOST, \\SMBHOST\DATA and Drives;
# screenshots work/lanwps1.png ... lanwps14.png, ten seconds apart.
# Needs tools/lantest/IBMLAN.INI and LMHOSTS in work/modules.txt.
R=$(cd "$(dirname "$0")/../.." && pwd)
D=$R/work/lanwps
rm -rf $D; mkdir -p $D/data/Documents $D/spool
python3 -I $R/tools/scripts/lantest_check.py prepare $D/data
echo "A text file on the server" > $D/data/Documents/readme.txt
python3 -I $R/tools/scripts/smbhost.py --share "DATA=$D/data" --share "PUBLIC=$D/data/Documents" --printer "LASER=$D/spool" \
  --user rs2:secret --ip 10.0.2.2 --comment "rs2 test server" > $D/smbhost.log 2>&1 &
srv=$!
cd $R
cargo xtask run --timeout 300 --net --init 'RS2\WPSTEST.EXE SHELL RUN C:\OS2\NET.EXE USE Y: \\SMBHOST\DATA secret /USER:rs2; OPEN <WP_NETWORK> 1; SLEEP 30; OPEN \\WORKGROUP 1; SLEEP 30; OPEN \\SMBHOST 1; SLEEP 30; OPEN \\SMBHOST\DATA 1; SLEEP 30; OPEN <WP_DRIVES> 1; SLEEP 60' \
  --type-at 40 --type '{sleep 20}{shot lanwps1}{sleep 10}{shot lanwps2}{sleep 10}{shot lanwps3}{sleep 10}{shot lanwps4}{sleep 10}{shot lanwps5}{sleep 10}{shot lanwps6}{sleep 10}{shot lanwps7}{sleep 10}{shot lanwps8}{sleep 10}{shot lanwps9}{sleep 10}{shot lanwps10}{sleep 10}{shot lanwps11}{sleep 10}{shot lanwps12}{sleep 10}{shot lanwps13}{sleep 10}{shot lanwps14}' > $D/guest.log 2>&1
kill $srv
grep -a "wpstest\|requester" $D/guest.log
