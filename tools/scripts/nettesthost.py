"""Host side of rs2's NETTEST (docs/NETWORK.md, "Testing"): run it next to

  cargo xtask run --net [MODEL] --init "RS2\\NETTEST.EXE -port 5586 -listen 2323" \\
      --hostfwd tcp::5595-:2323

Servers on 0.0.0.0 (the guest reaches them as 10.0.2.2, QEMU's gateway):
  PORT     a line server: reads a line, answers "host got: <line>"
  PORT+1   a sink: reads to the end, checks the bulk pattern, closes
  PORT+2   a source: 1 MB of the bulk pattern (i % 251), then closes
  PORT+3   urgent data out: "ab", "!" as out-of-band data, "cd", close
  PORT+4   urgent data in: reads to the end, answers what came in band
           and out of band ("inband=... oob=...")
  PORT+5   silent: accepts and waits (the guest's keepalives keep the
           connection; QEMU's network stack answers the probes), then
           answers a line after it

Client (--client HOSTPORT, default 5595): connects to the guest's
listener through QEMU's host forward (retrying until it listens), reads
its greeting, sends a line and prints the answer.

Prints PASS/FAIL lines for what it can check itself.

usage: python3 -I nettesthost.py [--port PORT] [--client HOSTPORT] [--for SECONDS]
"""
import select, socket, sys, threading, time

PORT = 5586
CLIENT = 5595
FOR = 600
args = sys.argv[1:]
while args:
    a = args.pop(0)
    if a == '--port':
        PORT = int(args.pop(0))
    elif a == '--client':
        CLIENT = int(args.pop(0))
    elif a == '--for':
        FOR = int(args.pop(0))


def log(*a):
    print(time.strftime('%H:%M:%S'), *a, flush=True)


def result(ok, what):
    log('PASS' if ok else 'FAIL', what)


def listener(port):
    s = socket.socket()
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.bind(('0.0.0.0', port))
    s.listen(5)
    return s


def serve(port, handler):
    s = listener(port)

    def loop():
        while True:
            c, a = s.accept()
            threading.Thread(target=handler, args=(c,), daemon=True).start()

    threading.Thread(target=loop, daemon=True).start()


def line_server(c):
    data = b''
    while b'\n' not in data:
        d = c.recv(512)
        if not d:
            break
        data += d
    c.sendall(b'host got: ' + data.strip() + b'\r\n')
    c.close()


def sink(c):
    n, good, t0 = 0, True, time.time()
    while True:
        d = c.recv(65536)
        if not d:
            break
        good = good and d == bytes((n + i) % 251 for i in range(len(d)))
        n += len(d)
    dt = max(time.time() - t0, 0.001)
    result(good and n == 1 << 20, 'sink: %d bytes from the guest (%.0f KB/s)' % (n, n / 1024 / dt))
    c.close()


def source(c):
    data = bytes(i % 251 for i in range(1 << 20))
    c.sendall(data)
    c.close()


def oob_out(c):
    c.sendall(b'ab')
    time.sleep(0.2)
    c.send(b'!', socket.MSG_OOB)
    time.sleep(0.2)
    c.sendall(b'cd')
    time.sleep(0.5)
    c.close()


def oob_in(c):
    inband, oob = b'', b''
    while True:
        r, _, x = select.select([c], [], [c], 10)
        if x:
            try:
                oob += c.recv(1, socket.MSG_OOB)
            except OSError:
                pass
        if r:
            try:
                d = c.recv(512)
            except OSError:
                d = b''
            if not d:
                break
            inband += d
        if not r and not x:
            break
    msg = b'inband=' + inband + b' oob=' + oob
    log('urgent in:', msg)
    c.sendall(msg + b'\r\n')
    c.close()


def silent(c):
    data = b''
    c.settimeout(120)
    try:
        while b'\n' not in data:
            d = c.recv(512)
            if not d:
                break
            data += d
        c.sendall(b'still here: ' + data.strip() + b'\r\n')
    except OSError:
        pass
    c.close()


def client():
    deadline = time.time() + FOR
    while time.time() < deadline:
        try:
            s = socket.create_connection(('127.0.0.1', CLIENT), timeout=5)
        except OSError:
            time.sleep(1)
            continue
        try:
            s.settimeout(10)
            greeting = s.recv(512)
            if not greeting:
                s.close()
                time.sleep(1)
                continue
            s.sendall(b'hello from the host\r\n')
            answer = b''
            while b'\n' not in answer:
                d = s.recv(512)
                if not d:
                    break
                answer += d
            result(b'hello from the host' in answer, 'client: the guest echoed %r' % answer.strip())
            return
        except OSError as e:
            log('client:', e)
        finally:
            s.close()
        time.sleep(1)


serve(PORT, line_server)
serve(PORT + 1, sink)
serve(PORT + 2, source)
serve(PORT + 3, oob_out)
serve(PORT + 4, oob_in)
serve(PORT + 5, silent)
log('serving on %d-%d' % (PORT, PORT + 5))
if CLIENT:
    threading.Thread(target=client, daemon=True).start()
time.sleep(FOR)
