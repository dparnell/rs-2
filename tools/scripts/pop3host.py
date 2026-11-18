"""A POP3 server (RFC 1939) and an SMTP receiver (RFC 5321) on the host, for
testing mail clients under rs2 (UltiMail Lite, docs/COMPATIBILITY.md):

  python3 -I tools/scripts/pop3host.py MAILDIR [--user U] [--password P]
      [--pop PORT] [--smtp PORT] [--messages N]

and run the guest with

  cargo xtask run --net --guestfwd tcp:10.0.2.100:110-tcp:127.0.0.1:1110 \\
      --guestfwd tcp:10.0.2.100:25-tcp:127.0.0.1:2525 ...

so that the guest reaches them as 10.0.2.100 on their standard ports.

POP3 (default 127.0.0.1:1110): USER/PASS (default "test"/"secret"), APOP is
not offered; STAT, LIST, UIDL, RETR, TOP, DELE, RSET, NOOP, CAPA, QUIT.
The mailbox is MAILDIR/inbox/*.eml, served in name order; --messages N
first writes N test letters there (plain text, one with a long body, one
MIME multipart with a text attachment) when the directory is empty.
Messages marked with DELE are removed at QUIT, as the RFC says.

SMTP (default 127.0.0.1:2525): takes any MAIL FROM/RCPT TO and keeps each
message as MAILDIR/sent/NNNN.eml.

Every command is logged on stdout ("pop: C: ...", passwords hidden).
"""
import os, socket, sys, threading, time

args = sys.argv[1:]
if not args or args[0].startswith('-'):
    sys.exit(__doc__)
ROOT = os.path.abspath(args[0])
opts = {'--user': 'test', '--password': 'secret', '--pop': '1110', '--smtp': '2525', '--messages': '0'}
i = 1
while i < len(args):
    if args[i] in opts and i + 1 < len(args):
        opts[args[i]] = args[i + 1]
        i += 2
    else:
        sys.exit(f'unknown argument {args[i]}\n' + __doc__)
INBOX = os.path.join(ROOT, 'inbox')
SENT = os.path.join(ROOT, 'sent')
os.makedirs(INBOX, exist_ok=True)
os.makedirs(SENT, exist_ok=True)
lock = threading.Lock()


def log(*a):
    with lock:
        print(*a, flush=True)


def sample_letters(n):
    date = time.strftime('%a, %d %b %Y %H:%M:%S +0000', time.gmtime())
    out = []
    for k in range(n):
        if k % 3 == 1:
            body = '\r\n'.join(f'Line {j + 1} of a longer letter, to fill more than one screen.' for j in range(60))
            subj = f'Test letter {k + 1}: a long one'
            out.append(f'From: Host Tester <tester@example.com>\r\nTo: test@example.com\r\nSubject: {subj}\r\n'
                       f'Date: {date}\r\nMessage-ID: <rs2-{k + 1}@example.com>\r\n\r\n{body}\r\n')
        elif k % 3 == 2:
            out.append('From: Host Tester <tester@example.com>\r\nTo: test@example.com\r\n'
                       f'Subject: Test letter {k + 1}: with an attachment\r\nDate: {date}\r\n'
                       f'Message-ID: <rs2-{k + 1}@example.com>\r\nMIME-Version: 1.0\r\n'
                       'Content-Type: multipart/mixed; boundary="rs2-boundary"\r\n\r\n'
                       'This is a MIME message.\r\n\r\n--rs2-boundary\r\nContent-Type: text/plain\r\n\r\n'
                       'The letter text, with a text file attached.\r\n\r\n--rs2-boundary\r\n'
                       'Content-Type: text/plain; name="notes.txt"\r\n'
                       'Content-Disposition: attachment; filename="notes.txt"\r\n\r\n'
                       'The attached notes.\r\nSecond line.\r\n\r\n--rs2-boundary--\r\n')
        else:
            out.append('From: Host Tester <tester@example.com>\r\nTo: test@example.com\r\n'
                       f'Subject: Test letter {k + 1}\r\nDate: {date}\r\nMessage-ID: <rs2-{k + 1}@example.com>\r\n\r\n'
                       f'Hello from the POP3 server on the host.\r\nThis is letter {k + 1}.\r\n')
    return out


if int(opts['--messages']) and not os.listdir(INBOX):
    for k, m in enumerate(sample_letters(int(opts['--messages']))):
        with open(os.path.join(INBOX, f'{k + 1:04}.eml'), 'w', newline='') as f:
            f.write(m)


class Conn:
    def __init__(self, s):
        self.s = s
        self.buf = b''

    def line(self):
        while b'\n' not in self.buf:
            d = self.s.recv(4096)
            if not d:
                return None
            self.buf += d
        l, self.buf = self.buf.split(b'\n', 1)
        return l.rstrip(b'\r').decode('latin-1')

    def send(self, t):
        self.s.sendall(t.encode('latin-1') + b'\r\n')


def pop_session(s, peer):
    c = Conn(s)
    c.send('+OK rs2 test POP3 server ready')
    user, authed = None, False
    msgs, deleted = [], set()

    def load():
        names = sorted(n for n in os.listdir(INBOX) if n.lower().endswith('.eml'))
        out = []
        for n in names:
            with open(os.path.join(INBOX, n), 'rb') as f:
                d = f.read().replace(b'\r\n', b'\n').replace(b'\n', b'\r\n')
            out.append((n, d))
        return out

    def get(arg):
        try:
            k = int(arg)
        except (TypeError, ValueError):
            return None
        if 1 <= k <= len(msgs) and k not in deleted:
            return k
        return None

    while True:
        l = c.line()
        if l is None:
            log(f'pop: {peer} closed')
            return
        w = l.split()
        cmd = w[0].upper() if w else ''
        log('pop: C:', 'PASS ****' if cmd == 'PASS' else l)
        a = w[1] if len(w) > 1 else None
        if cmd == 'QUIT':
            for k in sorted(deleted):
                os.remove(os.path.join(INBOX, msgs[k - 1][0]))
            c.send(f'+OK bye ({len(deleted)} deleted)')
            log(f'pop: QUIT, {len(deleted)} deleted')
            return
        if cmd == 'CAPA':
            c.send('+OK capabilities follow')
            for x in ('USER', 'UIDL', 'TOP', '.'):
                c.send(x)
        elif cmd == 'NOOP':
            c.send('+OK')
        elif not authed:
            if cmd == 'USER' and a:
                user = a
                c.send('+OK send PASS')
            elif cmd == 'PASS' and user is not None:
                pw = l.split(' ', 1)[1] if ' ' in l else ''
                if user == opts['--user'] and pw == opts['--password']:
                    authed = True
                    msgs = load()
                    c.send(f'+OK {user} has {len(msgs)} messages')
                    log(f'pop: {user} logged in, {len(msgs)} messages')
                else:
                    user = None
                    c.send('-ERR wrong user name or password')
            else:
                c.send('-ERR USER and PASS first')
        elif cmd == 'STAT':
            live = [m for k, m in enumerate(msgs, 1) if k not in deleted]
            c.send(f'+OK {len(live)} {sum(len(d) for _, d in live)}')
        elif cmd in ('LIST', 'UIDL'):
            val = (lambda k: len(msgs[k - 1][1])) if cmd == 'LIST' else (lambda k: msgs[k - 1][0])
            if a:
                k = get(a)
                c.send(f'+OK {k} {val(k)}' if k else '-ERR no such message')
            else:
                c.send('+OK')
                for k in range(1, len(msgs) + 1):
                    if k not in deleted:
                        c.send(f'{k} {val(k)}')
                c.send('.')
        elif cmd in ('RETR', 'TOP'):
            k = get(a)
            if not k:
                c.send('-ERR no such message')
                continue
            d = msgs[k - 1][1]
            lines = d.split(b'\r\n')
            if lines and lines[-1] == b'':
                lines.pop()
            if cmd == 'TOP':
                n = int(w[2]) if len(w) > 2 else 0
                h = lines.index(b'') if b'' in lines else len(lines)
                lines = lines[:h + 1 + n]
            c.send(f'+OK {len(d)} octets')
            out = b''.join((b'.' + x if x.startswith(b'.') else x) + b'\r\n' for x in lines) + b'.\r\n'
            s.sendall(out)
            log(f'pop: sent message {k} ({len(d)} octets)')
        elif cmd == 'DELE':
            k = get(a)
            if k:
                deleted.add(k)
                c.send(f'+OK message {k} deleted')
            else:
                c.send('-ERR no such message')
        elif cmd == 'RSET':
            deleted.clear()
            c.send('+OK')
        else:
            c.send('-ERR unknown command')


def smtp_session(s, peer):
    c = Conn(s)
    c.send('220 rs2 test SMTP receiver ready')
    while True:
        l = c.line()
        if l is None:
            return
        log('smtp: C:', l)
        cmd = l[:4].upper()
        if cmd in ('HELO', 'EHLO'):
            c.send('250 hello')
        elif cmd in ('MAIL', 'RCPT', 'RSET', 'NOOP'):
            c.send('250 OK')
        elif cmd == 'DATA':
            c.send('354 end with <CRLF>.<CRLF>')
            body = []
            while True:
                x = c.line()
                if x is None or x == '.':
                    break
                body.append(x[1:] if x.startswith('..') else x)
            n = len(os.listdir(SENT)) + 1
            with open(os.path.join(SENT, f'{n:04}.eml'), 'w', newline='') as f:
                f.write('\r\n'.join(body) + '\r\n')
            c.send('250 kept')
            log(f'smtp: kept message {n} ({len(body)} lines)')
        elif cmd == 'QUIT':
            c.send('221 bye')
            return
        else:
            c.send('502 not implemented')


def serve(port, fn, name):
    ls = socket.socket()
    ls.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    ls.bind(('127.0.0.1', port))
    ls.listen(5)
    log(f'{name} on 127.0.0.1:{port}')
    while True:
        s, peer = ls.accept()
        log(f'{name}: connection from {peer}')

        def run(s=s, peer=peer):
            try:
                fn(s, peer)
            except OSError as e:
                log(f'{name}: {e}')
            finally:
                s.close()
        threading.Thread(target=run, daemon=True).start()


threading.Thread(target=serve, args=(int(opts['--smtp']), smtp_session, 'smtp'), daemon=True).start()
log(f'mailbox {INBOX}: {len(os.listdir(INBOX))} messages')
serve(int(opts['--pop']), pop_session, 'pop')
