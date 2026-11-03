/* TFTPLOSS.CMD: IBM's 16-bit TFTP against tools/scripts/tftplossy.py (host
   10.0.2.2:6969), which drops a DATA block and an ACK: the transfers
   finish only if TFTP retransmits (TCPIPDLL _alarm/_xsignal). */
"@echo off"
call RxFuncAdd "SysSleep", "RexxUtil", "SysSleep"
/* DHCP first. */
call SysSleep 8
call lineout "C:\TFTPIN.TXT", "binary"
call lineout "C:\TFTPIN.TXT", "get big.bin C:\BIG.BIN"
call lineout "C:\TFTPIN.TXT", "put C:\BIG.BIN back.bin"
call lineout "C:\TFTPIN.TXT", "quit"
call lineout "C:\TFTPIN.TXT"
"C:\TCPIP\BIN\TFTP.EXE 10.0.2.2 6969 < C:\TFTPIN.TXT"
say "TFTPLOSS: done, rc" rc
