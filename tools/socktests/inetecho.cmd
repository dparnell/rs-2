/* INETD server: echoes lines on the connection whose socket number INETD passes */
parse arg s .
call RxFuncAdd 'SockLoadFuncs', 'RXSOCK', 'SockLoadFuncs'
call SockLoadFuncs 'quiet'
call SockSend s, 'rs2 inetd echo on socket' s || '0d0a'x
do forever
  n = SockRecv(s, 'data', 1000)
  if n <= 0 then leave
  call SockSend s, 'echo: ' || data
end
call SockSoClose s
