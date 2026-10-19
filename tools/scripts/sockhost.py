"""Host-side test servers for rs2's socket DLLs (docs/NETWORK.md, testing
with IBM's programs): run it next to `cargo xtask run --net`.

FTP on 2121 (root: ROOT dir, any user/password; PASV and PORT, LIST,
NLST, RETR, STOR, SIZE, CWD, PWD, TYPE, SYST, NOOP, QUIT), a telnet-ish
line server on 2323 (banner, echoes each line upper-cased, "bye" closes),
TCP echo on 2007. Logs to stdout.

Active-mode FTP data connections (PORT 10.0.2.15,...) are reached by
adding a QEMU user-network port forward through the monitor socket.

usage: python3 -I sockhost.py ROOT [work/qemu-monitor.sock]
"""
import os, socket, sys, threading, time

ROOT = os.path.abspath(sys.argv[1])
MONITOR = sys.argv[2] if len(sys.argv) > 2 else None


def qemu(cmd):
    for _ in range(20):
        try:
            m = socket.socket(socket.AF_UNIX)
            m.connect(MONITOR)
            m.settimeout(0.5)
            try:
                m.recv(4096)
            except OSError:
                pass
            m.sendall((cmd + '\n').encode())
            time.sleep(0.3)
            try:
                out = m.recv(4096)
            except OSError:
                out = b''
            m.close()
            log('qemu:', cmd, out[-80:])
            return
        except OSError as e:
            log('qemu monitor busy', e)
            time.sleep(0.2)


def connect_guest(addr, port):
    """A connection to the guest's addr:port."""
    if MONITOR and addr.startswith('10.0.2.'):
        t = socket.socket()
        t.bind(('127.0.0.1', 0))
        hp = t.getsockname()[1]
        t.close()
        qemu('hostfwd_add n0 tcp:127.0.0.1:%d-:%d' % (hp, port))
        try:
            return socket.create_connection(('127.0.0.1', hp), timeout=10)
        finally:
            threading.Timer(5, qemu, args=('hostfwd_remove n0 tcp:127.0.0.1:%d' % hp,)).start()
    return socket.create_connection((addr, port), timeout=10)
os.makedirs(ROOT, exist_ok=True)


def log(*a):
    print(time.strftime('%H:%M:%S'), *a, flush=True)


def ftp_session(c, addr):
    f = c.makefile('rwb', buffering=0)

    def reply(s):
        log('ftp >', s)
        c.sendall((s + '\r\n').encode())

    cwd = '/'
    pasv = None
    port = None
    binary = False
    reply('220 rs2 test FTP server ready')

    def data_conn():
        nonlocal pasv, port
        if pasv:
            d, _ = pasv.accept()
            pasv.close()
            pasv = None
            return d
        if port:
            d = connect_guest(*port)
            port = None
            return d
        return None

    def path(p):
        p = p if p.startswith('/') else os.path.join(cwd, p)
        p = os.path.normpath(p).lstrip('/')
        return os.path.join(ROOT, p)

    while True:
        line = b''
        while not line.endswith(b'\n'):
            ch = c.recv(1)
            if not ch:
                log('ftp: closed')
                return
            line += ch
        line = line.decode('latin1').strip()
        log('ftp <', line)
        cmd, _, arg = line.partition(' ')
        cmd = cmd.upper()
        if cmd == 'USER':
            reply('331 Password required')
        elif cmd == 'PASS':
            reply('230 Logged in')
        elif cmd == 'SYST':
            reply('215 UNIX Type: L8')
        elif cmd == 'TYPE':
            binary = arg.upper().startswith('I')
            reply('200 Type set to ' + arg)
        elif cmd in ('PWD', 'XPWD'):
            reply('257 "%s" is current directory' % cwd)
        elif cmd in ('CWD', 'XCWD'):
            p = path(arg)
            if os.path.isdir(p):
                cwd = '/' + os.path.relpath(p, ROOT).replace('.', '') if p != ROOT else '/'
                reply('250 CWD ok')
            else:
                reply('550 No such directory')
        elif cmd == 'PASV':
            pasv = socket.socket()
            pasv.bind(('0.0.0.0', 0))
            pasv.listen(1)
            pasv.settimeout(20)
            pn = pasv.getsockname()[1]
            reply('227 Entering Passive Mode (10,0,2,2,%d,%d)' % (pn >> 8, pn & 255))
        elif cmd == 'PORT':
            p = [int(x) for x in arg.split(',')]
            # The guest's address as seen from here goes through QEMU's NAT:
            # connect back to the address it gave (10.0.2.15 is not
            # reachable from the host), so use the forwarded loopback port
            # when one is configured, else try the given address.
            port = ('.'.join(map(str, p[:4])), p[4] * 256 + p[5])
            reply('200 PORT ok')
        elif cmd in ('LIST', 'NLST'):
            reply('150 Opening data connection')
            d = data_conn()
            names = sorted(os.listdir(path(cwd)))
            if cmd == 'LIST':
                out = ''.join('-rw-r--r-- 1 user user %8d Jan 01 00:00 %s\r\n' % (os.path.getsize(os.path.join(path(cwd), n)), n) for n in names)
            else:
                out = ''.join(n + '\r\n' for n in names)
            d.sendall(out.encode())
            d.close()
            reply('226 Transfer complete')
        elif cmd == 'SIZE':
            p = path(arg)
            reply('213 %d' % os.path.getsize(p) if os.path.isfile(p) else '550 No such file')
        elif cmd == 'RETR':
            p = path(arg)
            if not os.path.isfile(p):
                reply('550 No such file')
                continue
            reply('150 Opening data connection for %s (%d bytes)' % (arg, os.path.getsize(p)))
            d = data_conn()
            data = open(p, 'rb').read()
            if not binary:
                data = data.replace(b'\r\n', b'\n').replace(b'\n', b'\r\n')
            d.sendall(data)
            d.close()
            reply('226 Transfer complete')
        elif cmd == 'STOR':
            p = path(arg)
            reply('150 Ready to receive')
            d = data_conn()
            data = b''
            while True:
                b = d.recv(65536)
                if not b:
                    break
                data += b
            d.close()
            open(p, 'wb').write(data)
            log('ftp: stored %s %d bytes' % (p, len(data)))
            reply('226 Transfer complete')
        elif cmd == 'NOOP':
            reply('200 NOOP ok')
        elif cmd == 'QUIT':
            reply('221 Goodbye')
            c.close()
            return
        else:
            reply('502 Command not implemented')


def line_session(c, addr):
    c.sendall(b'rs2 test line server\r\nlogin: ')
    buf = b''
    while True:
        b = c.recv(4096)
        if not b:
            log('line: closed')
            return
        log('line <', repr(b))
        buf += b
        while b'\n' in buf or b'\r' in buf:
            i = min(x for x in (buf.find(b'\n'), buf.find(b'\r')) if x >= 0)
            line, buf = buf[:i], buf[i + 1:]
            # Skip telnet option negotiation bytes.
            text = bytes(x for x in line if x < 128 and x >= 32).decode()
            if not text:
                continue
            if text.lower() == 'bye':
                c.sendall(b'goodbye\r\n')
                c.close()
                return
            c.sendall(('you said: %s\r\n> ' % text.upper()).encode())


def echo_session(c, addr):
    while True:
        b = c.recv(65536)
        if not b:
            return
        c.sendall(b)


def serve(port, fn):
    s = socket.socket()
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.bind(('127.0.0.1', port))
    s.listen(5)
    while True:
        c, a = s.accept()
        log('connection on', port, 'from', a)
        threading.Thread(target=fn, args=(c, a), daemon=True).start()


for port, fn in ((2121, ftp_session), (2323, line_session), (2007, echo_session)):
    threading.Thread(target=serve, args=(port, fn), daemon=True).start()
log('serving', ROOT)
while True:
    time.sleep(3600)
