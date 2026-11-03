/* IBMAPPS.CMD: IBM's TCP/IP 4.x clients (C:\TCPIP\BIN) against rs2's
   servers. RS2\NETCLI.EXE 0 runs first: it starts rs2's FTPD, TFTPD,
   TELNETD, LPD (queue LPDQ), REXECD, RSHD and TALKD (which keep running)
   and makes the test files in C:\NETT. Run it with nethost.py and
   sockhost.py on the host, as netcli.sh does. */
call RxFuncAdd 'SysLoadFuncs', 'RexxUtil', 'SysLoadFuncs'
call SysLoadFuncs
call SysSleep 8
'@C:\RS2\NETCLI.EXE 0 > C:\NETCLI.OUT 2>&1'
say 'IBMAPPS: NETCLI rc' rc

call text 'C:\IBMFTP.TXT', 'user rs2 secret', 'binary', 'cd C:/NETT', 'get DATA.BIN C:\IBMDL.BIN', 'put C:\NETT\TEXT.TXT ibmup.txt', 'dir', 'quit'
call run 'FTP -> FTPD', 'C:\TCPIP\BIN\FTP.EXE -n 127.0.0.1 < C:\IBMFTP.TXT'
say 'IBMAPPS: FTP got' stream('C:\IBMDL.BIN', 'c', 'query size') 'of' stream('C:\NETT\DATA.BIN', 'c', 'query size') 'bytes; put' stream('C:\NETT\ibmup.txt', 'c', 'query size')

call text 'C:\IBMTFTP.TXT', 'binary', 'get t.bin C:\IBMT.BIN', 'quit'
call run 'TFTP -> TFTPD', 'C:\TCPIP\BIN\TFTP.EXE 127.0.0.1 < C:\IBMTFTP.TXT'
say 'IBMAPPS: TFTP got' stream('C:\IBMT.BIN', 'c', 'query size') 'bytes'

call run 'REXEC -> REXECD', 'C:\TCPIP\BIN\REXEC.EXE 127.0.0.1 -l rs2 -p secret -n echo ibm rexec ran'
call run 'RSH -> RSHD', 'C:\TCPIP\BIN\RSH.EXE 127.0.0.1 -n echo ibm rsh ran'
call run 'LPR -> LPD', 'C:\TCPIP\BIN\LPR.EXE -p lpdq -s 127.0.0.1 C:\NETT\TEXT.TXT'
call run 'LPQ -> LPD', 'C:\TCPIP\BIN\LPQ.EXE -p lpdq -s 127.0.0.1'
call run 'LPRM -> LPD', 'C:\TCPIP\BIN\LPRM.EXE -p lpdq -s 127.0.0.1 -'
call run 'LPQ after LPRM', 'C:\TCPIP\BIN\LPQ.EXE -p lpdq -s 127.0.0.1'
call run 'FINGER (no server)', 'C:\TCPIP\BIN\FINGER.EXE @127.0.0.1'
call run 'PING', 'C:\TCPIP\BIN\PING.EXE 127.0.0.1 56 3'
call run 'RPCINFO', 'C:\TCPIP\BIN\RPCINFO.EXE -p 127.0.0.1'
call run 'TIMEDC', 'C:\TCPIP\BIN\TIMEDC.EXE ?'
call run 'TELNETD -?', 'C:\TCPIP\BIN\TELNETD.EXE -?'
call run 'LPD -?', 'C:\TCPIP\BIN\LPD.EXE -?'
call run 'REXECD -?', 'C:\TCPIP\BIN\REXECD.EXE -?'
call run 'RSHD -?', 'C:\TCPIP\BIN\RSHD.EXE -?'
call run 'TFTPD -?', 'C:\TCPIP\BIN\TFTPD.EXE -?'
call run 'FTPD -?', 'C:\TCPIP\BIN\FTPD.EXE -?'
call run 'BOOTP -?', 'C:\TCPIP\BIN\BOOTP.EXE -?'
call run 'SENDMAIL -?', 'C:\TCPIP\BIN\SENDMAIL.EXE -?'
say 'IBMAPPS: done'
exit

/* A file of lines. */
text: procedure
  parse arg f
  call SysFileDelete f
  do i = 2 to arg()
    call lineout f, arg(i)
  end
  call lineout f
  return

run: procedure
  parse arg name, cmd
  say '==== IBMAPPS' name':' cmd
  call SysFileDelete 'C:\IBMOUT.TXT'
  '@'cmd '> C:\IBMOUT.TXT 2>&1'
  say '---- rc' rc
  do while lines('C:\IBMOUT.TXT')
    say linein('C:\IBMOUT.TXT')
  end
  call stream 'C:\IBMOUT.TXT', 'c', 'close'
  return
