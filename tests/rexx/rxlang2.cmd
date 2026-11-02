/* RXLANG2.CMD: an external routine RXLANG.CMD calls */
parse arg a
parse source . how .
if a = 'src' then return 'OS/2' how
if a = 'sub' then return how
return a * 2
