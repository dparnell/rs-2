/* rs2's Object REXX: the classes and objects IBM's Object REXX 6.00 keeps
   as REXX code in its REXX.IMG and that are not in OREXX.INF (rs2's own
   code, written from their behaviour on IBM's): .RX_QUEUE (an external data
   queue as an object), .STDQUE (the session queue) and .SYSTEM. Loaded the
   first time a program names one of them. */
.environment['RX_QUEUE'] = .rx_queue
q = .rx_queue~new
q~set('SESSION')
q~objectname = 'SESSION'
.local['STDQUE'] = q
s = .directory~new
s['VERSION'] = '2.30'
s['INTNAME'] = 'OS/2 Intel'
s['NAME'] = 'OS/2'
s['SOMVERSION'] = '2'
.environment['SYSTEM'] = s
exit

::class rx_queue
/* The queue's name: a new queue object has none until SET gives it one
   (GET then returns the variable's own name, as IBM's does). */
::method get
  expose named_queue
  return named_queue
::method set
  expose named_queue
  use arg new
  old = named_queue
  named_queue = new
  return old
/* CREATE makes a queue (RXQUEUE('Create')): its name */
::method create
  if arg(1, 'E') then return rxqueue('Create', arg(1))
  return rxqueue('Create')
::method delete
  expose named_queue
  return rxqueue('Delete', named_queue)
/* The number of lines in the queue */
::method query
  expose named_queue
  old = rxqueue('Set', named_queue)
  n = queued()
  call rxqueue 'Set', old
  return n
::method push
  expose named_queue
  use arg line
  old = rxqueue('Set', named_queue)
  push line
  call rxqueue 'Set', old
  return 0
::method queue
  expose named_queue
  use arg line
  old = rxqueue('Set', named_queue)
  queue line
  call rxqueue 'Set', old
  return 0
/* PULL and LINEIN: the next line, .nil when the queue is empty */
::method pull
  expose named_queue
  old = rxqueue('Set', named_queue)
  if queued() = 0 then r = .nil
  else parse pull r
  call rxqueue 'Set', old
  return r
::method linein
  forward message 'PULL'
