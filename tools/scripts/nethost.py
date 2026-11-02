"""Host-side servers and clients for testing rs2's TCP/IP applications
(docs/NETWORK.md, "rs2's TCP/IP applications"): run it next to

  cargo xtask run --net --init "RS2\\NETCLI.EXE" \\
      --hostfwd tcp::5521-:21 --hostfwd tcp::5523-:23 \\
      --guestfwd tcp:10.0.2.100:515-tcp:127.0.0.1:5515 \\
      --guestfwd tcp:10.0.2.100:79-tcp:127.0.0.1:7979 \\
      --guestfwd tcp:10.0.2.100:512-tcp:127.0.0.1:5512 \\
      --guestfwd tcp:10.0.2.100:514-tcp:127.0.0.1:5514 \\
      --guestfwd tcp:10.0.2.100:23-tcp:127.0.0.1:2324

Servers (on 127.0.0.1; the guest reaches them as 10.0.2.100 through the
guestfwd rules, on their standard ports):
  5515  LPD (RFC 1179): keeps the jobs it receives (in ROOT/lpd), lists
        and removes them
  7979  finger: answers "rs2 finger test: <query>"
  5512  rexec: checks user "rs2"/password "secret", runs nothing: answers
        "rexec ran: <command>"
  5514  rsh: answers "rsh ran: <command> as <ruser>"
  2324  a VT100 screen (colours, boxes, a scrolling region) for TELNET
FTP on 2121 is tools/scripts/sockhost.py's (run it too).

Clients (--client): after the guest's servers are up, logs in to the
guest's FTPD through 127.0.0.1:5521 (active mode: the PORT goes to
10.0.2.2) and to its TELNETD through 127.0.0.1:5523, and prints PASS or
FAIL lines.

usage: python3 -I nethost.py ROOT [--client]
"""
import os, socket, sys, threading, time

ROOT = os.path.abspath(sys.argv[1])
os.makedirs(os.path.join(ROOT, 'lpd'), exist_ok=True)


def log(*a):
    print(time.strftime('%H:%M:%S'), *a, flush=True)


def readline(c):
    line = b''
    while not line.endswith(b'\n'):
        ch = c.recv(1)
        if not ch:
            return None
        line += ch
    return line


def zstr(c):
    s = b''
    while True:
        ch = c.recv(1)
        if not ch or ch == b'\0':
            return s.decode('latin1')
        s += ch


jobs = []  # (number, queue, user, files)


def lpd_session(c, a):
    log('lpd: from', a)
    first = readline(c)
    if not first:
        return
    cmd, rest = first[0], first[1:].decode('latin1').strip()
    log('lpd <', cmd, rest)
    if cmd == 2:
        queue = rest
        c.sendall(b'\0')
        files, ctl = {}, ''
        while True:
            sub = readline(c)
            if not sub:
                break
            kind, size_name = sub[0], sub[1:].decode('latin1').strip()
            size, name = size_name.split(' ', 1)
            c.sendall(b'\0')
            data = b''
            while len(data) < int(size) + 1:
                b = c.recv(65536)
                if not b:
                    break
                data += b
            c.sendall(b'\0')
            data = data[:int(size)]
            if kind == 2:
                ctl = data.decode('latin1')
            else:
                files[name] = data
                open(os.path.join(ROOT, 'lpd', name), 'wb').write(data)
        user = next((l[1:] for l in ctl.splitlines() if l.startswith('P')), '?')
        n = len(jobs) + 1
        jobs.append((n, queue, user, files))
        log('lpd: job %d for %s from %s: %s' % (n, queue, user, ', '.join('%s %d bytes' % (k, len(v)) for k, v in files.items())))
        log('lpd: control file:', ctl.replace('\n', ' | '))
    elif cmd in (3, 4):
        queue = rest.split(' ')[0]
        out = 'Printer: %s@rs2test\r\n' % queue
        q = [j for j in jobs if j[1] == queue]
        if not q:
            out += 'no entries\r\n'
        for j in q:
            size = sum(len(v) for v in j[3].values())
            out += 'Rank  Owner   Job  Files     Total Size\r\n' if cmd == 3 else ''
            out += '%-5s %-7s %-4d %-9s %d bytes\r\n' % ('1st', j[2], j[0], ','.join(j[3]), size)
        c.sendall(out.encode())
    elif cmd == 5:
        w = rest.split(' ')
        queue, agent, which = w[0], w[1], w[2:]
        removed = []
        for j in list(jobs):
            if j[1] == queue and (str(j[0]) in which or agent in which):
                jobs.remove(j)
                removed.append(j[0])
        c.sendall(''.join('dfA%03d dequeued\r\n' % n for n in removed).encode() or b'no jobs removed\r\n')
    c.close()


def finger_session(c, a):
    q = readline(c) or b''
    log('finger <', q)
    c.sendall(b'rs2 finger test: ' + q.strip() + b'\r\nLogin: rs2   Name: rs2 test user\r\n')
    c.close()


def rexec_session(c, a):
    port, user, pw, cmd = zstr(c), zstr(c), zstr(c), zstr(c)
    log('rexec <', port, user, cmd)
    if user != 'rs2' or pw != 'secret':
        c.sendall(b'\1Login incorrect.\n')
    else:
        c.sendall(b'\0rexec ran: ' + cmd.encode() + b'\r\n')
    c.close()


def rsh_session(c, a):
    port, luser, ruser, cmd = zstr(c), zstr(c), zstr(c), zstr(c)
    log('rsh <', a, luser, ruser, cmd)
    c.sendall(b'\0rsh ran: ' + cmd.encode() + b' as ' + ruser.encode() + b'\r\n')
    # Input until the client shuts its side.
    c.settimeout(5)
    try:
        while c.recv(4096):
            pass
    except OSError:
        pass
    c.close()


def vt_session(c, a):
    """A screen for TELNET: negotiation, then VT100 drawing; echoes keys."""
    IAC, DO, WILL, SB, SE = 255, 253, 251, 250, 240
    c.sendall(bytes([IAC, WILL, 1, IAC, WILL, 3, IAC, DO, 24, IAC, DO, 31]))
    time.sleep(0.5)
    c.settimeout(0.5)
    neg = b''
    try:
        neg = c.recv(4096)
    except OSError:
        pass
    log('vt: negotiation from client', neg.hex())
    if bytes([IAC, SB, 24]) not in neg:
        c.sendall(bytes([IAC, SB, 24, 1, IAC, SE]))
        try:
            neg += c.recv(4096)
        except OSError:
            pass
    tt = neg.split(bytes([IAC, SB, 24, 0]))[1].split(bytes([IAC]))[0] if bytes([IAC, SB, 24, 0]) in neg else b'?'
    naws = neg.split(bytes([IAC, SB, 31]))[1][:4] if bytes([IAC, SB, 31]) in neg else b''
    size = '%dx%d' % (naws[0] * 256 + naws[1], naws[2] * 256 + naws[3]) if len(naws) == 4 else '?'
    log('vt: terminal type', tt, 'size', size)
    E = '\x1b['
    s = E + '2J' + E + 'H'
    s += E + '1;37;44m' + ' rs2 TELNET test screen'.ljust(80) + E + '0m'
    s += E + '3;3H' + '\x1b(0lqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqk\x1b(B'
    for r in range(4, 8):
        s += E + '%d;3H\x1b(0x\x1b(B%s\x1b(0x\x1b(B' % (r, ' ' * 30)
    s += E + '8;3H' + '\x1b(0mqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqj\x1b(B'
    s += E + '4;5H' + E + '31mred ' + E + '32mgreen ' + E + '33myellow ' + E + '34mblue' + E + '0m'
    s += E + '5;5H' + E + '1mbold ' + E + '0m' + E + '4munderline' + E + '0m ' + E + '7m reverse ' + E + '0m'
    s += E + '6;5Hterminal type: ' + tt.decode('latin1')
    s += E + '7;5Hwindow size: ' + size
    s += E + '10;1H' + 'Scrolling region (rows 10-14):'
    s += E + '11;14r' + E + '11;1H'
    for i in range(8):
        s += 'scroll line %d\r\n' % i
    s += E + 'r' + E + '16;1H' + 'Type keys (they are echoed back as hex); "q" closes.\r\n> '
    c.sendall(s.encode('latin1'))
    c.settimeout(None)
    while True:
        b = c.recv(1024)
        if not b:
            break
        log('vt: keys', b.hex())
        if b.startswith(b'q'):
            c.sendall(b'\r\nbye\r\n')
            break
        c.sendall(('[' + b.hex() + '] ').encode())
    c.close()


def serve(port, fn):
    s = socket.socket()
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.bind(('127.0.0.1', port))
    s.listen(5)
    while True:
        c, a = s.accept()
        threading.Thread(target=fn, args=(c, a), daemon=True).start()


def result(ok, what):
    print('PASS' if ok else 'FAIL', what, flush=True)


def client_ftpd():
    """The guest's FTPD: login, PASV is unreachable from here, so PORT to
    a host listener the guest reaches as 10.0.2.2."""
    for _ in range(120):
        try:
            c = socket.create_connection(('127.0.0.1', 5521), timeout=5)
            banner = readline(c)
            if banner and banner.startswith(b'220'):
                break
            c.close()
        except OSError:
            pass
        time.sleep(2)
    else:
        result(False, 'host -> FTPD: no connection')
        return
    f = c.makefile('rb')

    def cmd(s):
        c.sendall((s + '\r\n').encode())
        while True:
            l = f.readline().decode('latin1')
            if len(l) < 4 or l[3] != '-':
                log('ftpd >', l.strip())
                return l

    cmd('USER rs2')
    r = cmd('PASS secret')
    result(r.startswith('230'), 'host -> FTPD: login (%s)' % r.strip())
    lst = socket.socket()
    lst.bind(('127.0.0.1', 0))
    lst.listen(1)
    p = lst.getsockname()[1]
    # QEMU's NAT: the guest's 10.0.2.2 is this host's 127.0.0.1.
    cmd('PORT 10,0,2,2,%d,%d' % (p >> 8, p & 255))
    r = cmd('LIST')
    d, _ = lst.accept()
    data = b''
    while True:
        b = d.recv(65536)
        if not b:
            break
        data += b
    d.close()
    r = f.readline().decode('latin1')
    log('ftpd LIST:\n' + data.decode('latin1'))
    result(r.startswith('226') and b'DATA.BIN' in data.upper(), 'host -> FTPD: LIST in active mode')
    cmd('QUIT')
    c.close()


def client_telnetd():
    for _ in range(60):
        try:
            c = socket.create_connection(('127.0.0.1', 5523), timeout=10)
            break
        except OSError:
            time.sleep(2)
    else:
        result(False, 'host -> TELNETD: no connection')
        return
    c.settimeout(3)
    got = b''

    def drain():
        nonlocal got
        try:
            while True:
                b = c.recv(4096)
                if not b:
                    break
                got += b
        except OSError:
            pass

    drain()
    c.sendall(b'secret\r\n')
    time.sleep(1)
    drain()
    c.sendall(b'echo hello from telnetd\r\n')
    time.sleep(2)
    drain()
    c.sendall(b'ver\r\n')
    time.sleep(2)
    drain()
    text = bytes(x for x in got if x < 128 and x != 255).decode('latin1')
    log('telnetd session:\n' + text)
    result('hello from telnetd' in text.split('echo hello from telnetd')[-1], 'host -> TELNETD: command output')
    c.sendall(b'exit\r\n')
    time.sleep(1)
    c.close()


for port, fn in ((5515, lpd_session), (7979, finger_session), (5512, rexec_session), (5514, rsh_session), (2324, vt_session)):
    threading.Thread(target=serve, args=(port, fn), daemon=True).start()
log('serving', ROOT)
if '--client' in sys.argv:
    time.sleep(5)
    client_ftpd()
    client_telnetd()
while True:
    time.sleep(3600)
