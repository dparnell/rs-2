#!/bin/sh
# Network printers in QEMU against smbhost.py (docs/LAN.md, PRINTING.md):
# a network printer made with NETID=, a raw job on its local alias that
# must arrive in the server's LASER spool directory, the printer's
# settings, then the server's folder (DATA, PUBLIC, the LASER queue as a
# network printer) and its settings. Screenshots work/lanprint1.png ...
# Needs tools/lantest/IBMLAN.INI and LMHOSTS in work/modules.txt.
R=$(cd "$(dirname "$0")/../.." && pwd)
D=$R/work/lanprint
rm -rf $D; mkdir -p $D/data/Documents $D/spool
echo "A text file on the server" > $D/data/Documents/readme.txt
python3 -I $R/tools/scripts/smbhost.py --share "DATA=$D/data" --share "PUBLIC=$D/data/Documents" --printer "LASER=$D/spool" \
  --user rs2:secret --guest --ip 10.0.2.2 --comment "rs2 test server" > $D/smbhost.log 2>&1 &
srv=$!
cd $R
cargo xtask run --timeout 220 --net --init 'RS2\WPSTEST.EXE SHELL RUN C:\OS2\NET.EXE USE Y: \\SMBHOST\DATA secret /USER:rs2; CREATE WPRPrinter NetLaser <WP_DESKTOP> NETID=WORKGROUP\\SMBHOST\LASER|OBJECTID=<RS2_NP1>; JOB LASER TestDoc; SLEEP 5; OPEN <RS2_NP1> 2; SLEEP 20; OPEN <WP_NETWORK> 1; SLEEP 8; OPEN \\WORKGROUP 1; SLEEP 8; OPEN \\SMBHOST 1; SLEEP 10; OPEN \\SMBHOST 2; SLEEP 60' \
  --type-at 40 --type '{sleep 30}{shot lanprint1}{sleep 25}{shot lanprint2}{sleep 20}{shot lanprint3}{sleep 20}{shot lanprint4}' > $D/guest.log 2>&1
kill $srv
grep -a "wpstest\|requester" $D/guest.log
echo "jobs on the server:"
ls -l $D/spool
