"""Host-side servers and clients for testing rs2's TCP/IP applications
(docs/NETWORK.md, "rs2's TCP/IP applications"): run it next to

  cargo xtask run --net --init "RS2\\NETCLI.EXE" \\
      --hostfwd tcp::5521-:21 --hostfwd tcp::5523-:23 --hostfwd tcp::5525-:515 \\
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
10.0.2.2), sends a job to its LPD through 127.0.0.1:5525 and drives its
TELNETD through 127.0.0.1:5523, and prints PASS or FAIL lines.

With --pty, only the TELNETD checks (a VT100 screen emulated here: CMD's
prompt, colours and output, TEDIT full screen, cursor keys, exit), then
it exits; --shots DIR saves the emulated screens as PNGs. --pipes checks
a TELNETD started with -n (pipes) instead.

usage: python3 -I nethost.py ROOT [--client | --pty] [--pipes] [--shots DIR]
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


class Screen:
    """A small ANSI terminal (what rs2's TELNETD sends: CUP, ED, EL, SGR,
    cursor show/hide, CR, LF with scrolling, UTF-8): cells of (character,
    PC attribute)."""
    ANSI_TO_PC = [0, 4, 2, 6, 1, 5, 3, 7]

    def __init__(self, cols=80, rows=25):
        self.cols, self.rows = cols, rows
        self.cells = [[(' ', 7)] * cols for _ in range(rows)]
        self.r = self.c = 0
        self.attr = 7
        self.cursor = True
        self.buf = b''
        self.pending_wrap = False

    def text(self):
        return '\n'.join(''.join(ch for ch, _ in row).rstrip() for row in self.cells)

    def row(self, r):
        return ''.join(ch for ch, _ in self.cells[r])

    def lf(self):
        if self.r == self.rows - 1:
            self.cells.pop(0)
            self.cells.append([(' ', 7)] * self.cols)
        else:
            self.r += 1

    def sgr(self, ps):
        for p in ps or [0]:
            if p == 0:
                self.attr = 7
            elif p == 1:
                self.attr |= 8
            elif p == 5:
                self.attr |= 0x80
            elif 30 <= p <= 37:
                self.attr = (self.attr & 0xf8) | self.ANSI_TO_PC[p - 30]
            elif 40 <= p <= 47:
                self.attr = (self.attr & 0x8f) | self.ANSI_TO_PC[p - 40] << 4
            elif 100 <= p <= 107:
                self.attr = (self.attr & 0x0f) | (self.ANSI_TO_PC[p - 100] | 8) << 4

    def feed(self, data):
        s = (self.buf + data).decode('utf-8', 'replace') if True else ''
        # Keep an incomplete trailing escape sequence or UTF-8 character.
        self.buf = b''
        i = 0
        while i < len(s):
            ch = s[i]
            if ch == '\x1b':
                j = i + 1
                if j >= len(s):
                    self.buf = s[i:].encode()
                    break
                if s[j] != '[':
                    i = j + 1
                    continue
                k = j + 1
                while k < len(s) and not ('@' <= s[k] <= '~'):
                    k += 1
                if k >= len(s):
                    self.buf = s[i:].encode()
                    break
                params, fin = s[j + 1:k], s[k]
                priv = params.startswith('?')
                ps = [int(x) if x.isdigit() else 0 for x in params.lstrip('?').split(';')] if params.lstrip('?') else []
                self.pending_wrap = False
                if priv and fin in 'hl' and 25 in ps:
                    self.cursor = fin == 'h'
                elif fin == 'H':
                    self.r = min(max((ps[0] if ps else 1) - 1, 0), self.rows - 1)
                    self.c = min(max((ps[1] if len(ps) > 1 else 1) - 1, 0), self.cols - 1)
                elif fin == 'J':
                    self.cells = [[(' ', 7)] * self.cols for _ in range(self.rows)]
                elif fin == 'K':
                    for x in range(self.c, self.cols):
                        self.cells[self.r][x] = (' ', self.attr)
                elif fin == 'm':
                    self.sgr(ps)
                i = k + 1
                continue
            if ch == '\r':
                self.c = 0
                self.pending_wrap = False
            elif ch == '\n':
                self.lf()
                self.pending_wrap = False
            elif ch == '\b':
                self.c = max(self.c - 1, 0)
            elif ch >= ' ':
                if self.pending_wrap:
                    self.c = 0
                    self.lf()
                    self.pending_wrap = False
                self.cells[self.r][self.c] = (ch, self.attr)
                if self.c == self.cols - 1:
                    self.pending_wrap = True
                else:
                    self.c += 1
            i += 1

    def png(self, path, font_path):
        """Draw the screen with the kernel's unscii font (16 rows a glyph,
        keyed by code point) into a PNG."""
        import struct, zlib
        font = open(font_path, 'rb').read()
        glyphs = {}
        for o in range(0, len(font) - 19, 20):
            glyphs[struct.unpack_from('<I', font, o)[0]] = font[o + 4:o + 20]
        pal = [(0, 0, 0), (0, 0, 170), (0, 170, 0), (0, 170, 170), (170, 0, 0), (170, 0, 170), (170, 85, 0), (170, 170, 170),
               (85, 85, 85), (85, 85, 255), (85, 255, 85), (85, 255, 255), (255, 85, 85), (255, 85, 255), (255, 255, 85), (255, 255, 255)]
        w, h = self.cols * 8, self.rows * 16
        raw = bytearray()
        for y in range(h):
            raw.append(0)
            r, gy = divmod(y, 16)
            for x in range(self.cols):
                ch, a = self.cells[r][x]
                g = glyphs.get(ord(ch), glyphs.get(ord('?')))
                fg, bg = pal[a & 15], pal[(a >> 4) & 7]
                bits = g[gy]
                if self.cursor and (r, x) == (self.r, self.c) and gy >= 14:
                    bits = 0xff
                for b in range(8):
                    raw.extend(fg if bits & (0x80 >> b) else bg)

        def chunk(t, d):
            return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)
        png = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 2, 0, 0, 0))
        png += chunk(b'IDAT', zlib.compress(bytes(raw), 9)) + chunk(b'IEND', b'')
        open(path, 'wb').write(png)


class TelnetTerm:
    """A telnet client with a Screen: answers TERMINAL-TYPE (VT100) and
    NAWS (80x25)."""
    IAC, DONT, DO, WONT, WILL, SB, SE = 255, 254, 253, 252, 251, 250, 240

    def __init__(self, port, cols=80, rows=25):
        for _ in range(60):
            try:
                self.c = socket.create_connection(('127.0.0.1', port), timeout=10)
                break
            except OSError:
                time.sleep(2)
        else:
            raise OSError('no connection')
        self.c.settimeout(0.3)
        self.scr = Screen(cols, rows)
        self.size = (cols, rows)
        self.state = 0
        self.sb = b''
        self.raw = b''

    def opt(self, cmd, o):
        I = self.IAC
        if cmd == self.DO and o == 24:
            self.c.sendall(bytes([I, self.WILL, 24]))
        elif cmd == self.DO and o == 31:
            w, h = self.size
            self.c.sendall(bytes([I, self.WILL, 31, I, self.SB, 31, w >> 8, w & 255, h >> 8, h & 255, I, self.SE]))
        elif cmd == self.DO:
            self.c.sendall(bytes([I, self.WONT, o]))
        elif cmd == self.WILL:
            self.c.sendall(bytes([I, self.DO, o]))

    def pump(self, secs=1.0):
        end = time.time() + secs
        while time.time() < end:
            try:
                b = self.c.recv(65536)
            except socket.timeout:
                continue
            except OSError:
                return False
            if not b:
                return False
            self.raw += b
            out = bytearray()
            for x in b:
                if self.state == 0:
                    if x == self.IAC:
                        self.state = 1
                    else:
                        out.append(x)
                elif self.state == 1:
                    if x in (self.DO, self.DONT, self.WILL, self.WONT):
                        self.state = x
                    elif x == self.SB:
                        self.state, self.sb = 'sb', b''
                    elif x == self.IAC:
                        out.append(x)
                        self.state = 0
                    else:
                        self.state = 0
                elif self.state == 'sb':
                    if x == self.IAC:
                        self.state = 'sbiac'
                    else:
                        self.sb += bytes([x])
                elif self.state == 'sbiac':
                    self.state = 0 if x == self.SE else 'sb'
                    if x == self.SE and self.sb[:2] == bytes([24, 1]):
                        I = self.IAC
                        self.c.sendall(bytes([I, self.SB, 24, 0]) + b'VT100' + bytes([I, self.SE]))
                else:
                    self.opt(self.state, x)
                    self.state = 0
            self.scr.feed(bytes(out))
        return True

    def send(self, s, secs=1.5):
        self.c.sendall(s.encode('latin1') if isinstance(s, str) else s)
        return self.pump(secs)

    def wait_for(self, text, secs=15):
        end = time.time() + secs
        while time.time() < end:
            if text in self.scr.text():
                return True
            if not self.pump(0.5):
                return text in self.scr.text()
        return False


def client_lpd():
    """The guest's LPD (through 127.0.0.1:5525): a job for queue LPDQ, then
    the queue's state shows it."""
    def connect():
        for _ in range(30):
            try:
                return socket.create_connection(('127.0.0.1', 5525), timeout=10)
            except OSError:
                time.sleep(2)
        return None
    c = connect()
    if not c:
        result(False, 'host -> LPD: no connection')
        return
    data = b'printed from the host through rs2 LPD\n' * 20
    ctl = b'Hhosttest\nPhostuser\nJhostjob\nfdfA001hosttest\nUdfA001hosttest\nNhost.txt\n'
    acks = b''
    c.sendall(b'\x02lpdq\n')
    acks += c.recv(1)
    c.sendall(b'\x02%d cfA001hosttest\n' % len(ctl))
    acks += c.recv(1)
    c.sendall(ctl + b'\0')
    acks += c.recv(1)
    c.sendall(b'\x03%d dfA001hosttest\n' % len(data))
    acks += c.recv(1)
    c.sendall(data + b'\0')
    acks += c.recv(1)
    c.close()
    result(acks == b'\0' * 5, 'host -> LPD: job accepted (%r)' % acks)
    time.sleep(1)
    c = connect()
    c.sendall(b'\x03lpdq\n')
    st = b''
    while True:
        b = c.recv(4096)
        if not b:
            break
        st += b
    log('lpd state:\n' + st.decode('latin1'))
    result(b'hostjob' in st and b'hostuser' in st, 'host -> LPD: the job is in queue LPDQ')
    c.close()


def client_telnetd_pty(shots=None, font=None):
    """The guest's TELNETD with pseudo terminals: CMD's prompt and output,
    colours, a full-screen VIO program (TEDIT) and its keys, and the end
    of the session."""
    try:
        t = TelnetTerm(5523)
    except OSError:
        result(False, 'host -> TELNETD: no connection')
        return
    ok = t.wait_for(':\\', 60)
    result(ok, 'TELNETD pty: the command prompt shows')

    def wait_screen(cond, secs=60):
        end = time.time() + secs
        while time.time() < end:
            if cond(t.scr.text()):
                return True
            if not t.pump(0.5):
                return cond(t.scr.text())
        return False

    t.send('echo hello from telnetd\r', 0.5)
    wait_screen(lambda x: any(r.strip() == 'hello from telnetd' for r in x.split('\n')))
    rows = t.scr.text().split('\n')
    result(any(r.strip() == 'hello from telnetd' for r in rows), 'TELNETD pty: command output')
    t.send('prompt $e[1;33;44m[pty $p]$e[0m\r', 0.5)
    wait_screen(lambda x: '[pty C:\\]' in x)
    t.send('cls\r', 0.5)
    # The prompt is near the top after CLS.
    wait_screen(lambda x: any(t.scr.row(r).startswith('[pty') for r in range(4)) and 'prompt' not in x, 30)
    pr = next((r for r in range(4) if t.scr.row(r).startswith('[pty')), 0)
    r0 = t.scr.cells[pr]
    txt = t.scr.row(pr)
    result(txt.startswith('[pty C:\\') and r0[1][1] == 0x1e, 'TELNETD pty: ANSI colours in the prompt (%r attr %x)' % (txt[:12], r0[1][1]))
    t.send('dir C:\\OS2\\*.EXE\r', 0.5)
    wait_screen(lambda x: 'BYTES FREE' in x.upper() and x.rstrip().endswith(']'))
    result('FILE(S)' in t.scr.text().upper() and 'BYTES FREE' in t.scr.text().upper(), 'TELNETD pty: dir scrolls the screen')
    if shots:
        t.scr.png(os.path.join(shots, 'telnetd-cmd.png'), font)
    t.send('cls\r', 0.5)
    wait_screen(lambda x: 'BYTES FREE' not in x.upper(), 30)
    t.send('tedit C:\\CONFIG.SYS\r', 0.5)
    wait_screen(lambda x: 'TEDIT V' in x.upper() and 'END OF FILE' in x.upper())
    t.pump(1)
    log('TEDIT over telnet:\n' + t.scr.text())
    scr = t.scr.text().upper()
    result('CONFIG.SYS' in scr and ('PROTSHELL' in scr or 'SET ' in scr or 'LIBPATH' in scr), 'TELNETD pty: TEDIT shows the file full screen')
    if shots:
        t.scr.png(os.path.join(shots, 'telnetd-tedit.png'), font)
    # Cursor keys move TEDIT's cursor.
    before = (t.scr.r, t.scr.c)
    t.send('\x1b[B\x1b[B\x1b[C', 0.5)
    wait_screen(lambda x: (t.scr.r, t.scr.c) != before, 20)
    result((t.scr.r, t.scr.c) != before, 'TELNETD pty: cursor keys reach the program (%s -> %s)' % (before, (t.scr.r, t.scr.c)))
    # Leave TEDIT: F3 (quit), then answer a "save?" question no.
    t.send('\x1bOR', 0.5)
    if not wait_screen(lambda x: '[pty' in x, 10):
        t.send('n', 0.5)
        wait_screen(lambda x: '[pty' in x, 20)
    if shots:
        t.scr.png(os.path.join(shots, 'telnetd-after.png'), font)
    t.send('exit\r', 0.5)
    end = time.time() + 30
    closed = False
    while time.time() < end:
        if not t.pump(1):
            closed = True
            break
    result(closed, 'TELNETD pty: exit ends the connection')
    t.c.close()


for port, fn in ((5515, lpd_session), (7979, finger_session), (5512, rexec_session), (5514, rsh_session), (2324, vt_session)):
    threading.Thread(target=serve, args=(port, fn), daemon=True).start()
log('serving', ROOT)
SHOTS = sys.argv[sys.argv.index('--shots') + 1] if '--shots' in sys.argv else None
FONT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', 'kernel', 'assets', 'unscii-16.bin')
if '--pty' in sys.argv:
    # Only TELNETD's pseudo terminal checks, then exit.
    client_telnetd_pty(SHOTS, FONT)
    sys.exit(0)
if '--client' in sys.argv:
    time.sleep(5)
    client_ftpd()
    client_lpd()
    if '--pipes' in sys.argv:
        client_telnetd()
    else:
        client_telnetd_pty(SHOTS, FONT)
while True:
    time.sleep(3600)
