#!/bin/sh
# Run RS2\NETCLI.EXE (rs2's TCP/IP applications) in QEMU against the host
# servers: start these first, in other shells (MONITOR is QEMU's monitor
# socket, which xtask prints; sockhost.py needs it for active-mode FTP):
#   python3 -I tools/scripts/sockhost.py work/nethost/ftp MONITOR
#   python3 -I tools/scripts/nethost.py work/nethost --client
# The guest reaches nethost.py's servers as 10.0.2.100 on their standard
# ports (QEMU's guestfwd runs nc for each connection); the host reaches the
# guest's FTPD, TELNETD and LPD on 5521, 5523 and 5525.
# usage: tools/scripts/netcli.sh [seconds the servers stay up] [extra xtask options]
linger=${1:-60}
[ $# -gt 0 ] && shift
f() { echo "tcp:10.0.2.100:$1-cmd:nc 127.0.0.1 $2"; }
exec cargo xtask run --headless --timeout $((linger + 200)) --net --init "RS2\\NETCLI.EXE $linger" \
  --hostfwd tcp::5521-:21 --hostfwd tcp::5523-:23 --hostfwd tcp::5525-:515 \
  --guestfwd "$(f 515 5515)" --guestfwd "$(f 79 7979)" --guestfwd "$(f 512 5512)" \
  --guestfwd "$(f 514 5514)" --guestfwd "$(f 23 2324)" "$@"
