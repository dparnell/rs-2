/* ORXREQ.CMD: the file ORXTEST.CMD's ::requires loads */
::routine orxreq_hello public
  return 'hello' arg(1)

::class greeter public
::method greet
  return 'Greetings,' arg(1)
