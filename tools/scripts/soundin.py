#!/usr/bin/env python3
"""Sound input (and output) for QEMU through its D-Bus audio backend.

    python3 -I tools/scripts/soundin.py QMP_SOCKET IN [OUT.wav]

QEMU's sound backends on macOS cannot record from a file, so `cargo
xtask run --sound-in IN` starts QEMU with `-audiodev dbus` and
`-display dbus,p2p=yes` and runs this script, which:

- connects to QEMU's QMP socket, hands QEMU one end of a socket pair
  (`getfd`, `add_client protocol=@dbus-display`) and speaks D-Bus over
  the other end (peer to peer: no bus daemon);
- registers an AudioInListener (org.qemu.Display1.Audio
  RegisterInListener, with a socket pair of its own) and answers QEMU's
  Read calls with IN: a WAV file (PCM 8/16-bit, mono/stereo, any rate;
  played in a loop) or `tone:FREQ` (a sine at a third of full scale);
- with OUT, registers an AudioOutListener too and writes what the guest
  plays to OUT.wav (the header's sizes kept up to date).

Only the parts of the D-Bus wire protocol these calls need are here:
little-endian messages, EXTERNAL authentication, unix fd passing.
"""

import array
import math
import os
import socket
import struct
import sys
import threading

# ---- D-Bus marshalling ----

ALIGN = {"y": 1, "b": 4, "n": 2, "q": 2, "i": 4, "u": 4, "x": 8, "t": 8, "d": 8,
         "s": 4, "o": 4, "g": 1, "a": 4, "(": 8, "{": 8, "v": 1, "h": 4}


def split_sig(sig):
    """The complete types of a signature."""
    out, i = [], 0
    while i < len(sig):
        j = sig_end(sig, i)
        out.append(sig[i:j])
        i = j
    return out


def sig_end(sig, i):
    c = sig[i]
    if c == "a":
        return sig_end(sig, i + 1)
    if c in "({":
        close = ")" if c == "(" else "}"
        i += 1
        while sig[i] != close:
            i = sig_end(sig, i)
        return i + 1
    return i + 1


class Writer:
    def __init__(self):
        self.b = bytearray()

    def pad(self, n):
        while len(self.b) % n:
            self.b.append(0)

    def put(self, t, v):
        c = t[0]
        self.pad(ALIGN[c])
        if c == "y":
            self.b.append(v)
        elif c in "bu h":
            self.b += struct.pack("<I", int(v))
        elif c == "i":
            self.b += struct.pack("<i", v)
        elif c == "n":
            self.b += struct.pack("<h", v)
        elif c == "q":
            self.b += struct.pack("<H", v)
        elif c == "x":
            self.b += struct.pack("<q", v)
        elif c == "t":
            self.b += struct.pack("<Q", v)
        elif c == "d":
            self.b += struct.pack("<d", v)
        elif c in "so":
            e = v.encode()
            self.b += struct.pack("<I", len(e)) + e + b"\0"
        elif c == "g":
            e = v.encode()
            self.b += bytes([len(e)]) + e + b"\0"
        elif c == "v":
            sig, val = v
            self.put("g", sig)
            self.put(sig, val)
        elif c == "a":
            et = t[1:]
            at = len(self.b)
            self.b += b"\0\0\0\0"
            self.pad(ALIGN[et[0]])
            start = len(self.b)
            if et == "y":
                self.b += bytes(v)
            else:
                for e in v:
                    self.put(et, e)
            struct.pack_into("<I", self.b, at, len(self.b) - start)
        elif c in "({":
            for st, e in zip(split_sig(t[1:-1]), v):
                self.put(st, e)
        else:
            raise ValueError("cannot marshal " + t)


class Reader:
    def __init__(self, b, pos=0):
        self.b, self.p = b, pos

    def align(self, n):
        self.p = (self.p + n - 1) // n * n

    def get(self, t):
        c = t[0]
        self.align(ALIGN[c])
        b = self.b
        if c == "y":
            self.p += 1
            return b[self.p - 1]
        fixed = {"b": "<I", "u": "<I", "h": "<I", "i": "<i", "n": "<h", "q": "<H", "x": "<q", "t": "<Q", "d": "<d"}
        if c in fixed:
            f = fixed[c]
            v = struct.unpack_from(f, b, self.p)[0]
            self.p += struct.calcsize(f)
            return v
        if c in "so":
            n = struct.unpack_from("<I", b, self.p)[0]
            s = b[self.p + 4:self.p + 4 + n].decode()
            self.p += 5 + n
            return s
        if c == "g":
            n = b[self.p]
            s = b[self.p + 1:self.p + 1 + n].decode()
            self.p += 2 + n
            return s
        if c == "v":
            sig = self.get("g")
            return (sig, self.get(sig))
        if c == "a":
            n = struct.unpack_from("<I", b, self.p)[0]
            self.p += 4
            et = t[1:]
            self.align(ALIGN[et[0]])
            end = self.p + n
            if et == "y":
                v = bytes(b[self.p:end])
                self.p = end
                return v
            out = []
            while self.p < end:
                out.append(self.get(et))
            return out
        if c in "({":
            return [self.get(st) for st in split_sig(t[1:-1])]
        raise ValueError("cannot unmarshal " + t)


CALL, RETURN, ERROR, SIGNAL = 1, 2, 3, 4
F_PATH, F_INTERFACE, F_MEMBER, F_ERROR, F_REPLY, F_DEST, F_SENDER, F_SIG, F_FDS = range(1, 10)
FIELD_TYPE = {F_PATH: "o", F_INTERFACE: "s", F_MEMBER: "s", F_ERROR: "s", F_REPLY: "u",
              F_DEST: "s", F_SENDER: "s", F_SIG: "g", F_FDS: "u"}


class Conn:
    """A peer-to-peer D-Bus connection over a unix socket (we are the
    authentication client)."""

    def __init__(self, sock):
        self.s = sock
        self.serial = 0
        self.buf = b""
        self.lock = threading.Lock()
        self.auth()

    def line(self):
        while b"\r\n" not in self.buf:
            d = self.s.recv(4096)
            if not d:
                raise EOFError("auth: connection closed")
            self.buf += d
        l, self.buf = self.buf.split(b"\r\n", 1)
        return l.decode()

    def auth(self):
        uid = str(os.getuid()).encode().hex()
        self.s.sendall(b"\0AUTH EXTERNAL " + uid.encode() + b"\r\n")
        r = self.line()
        if not r.startswith("OK"):
            self.s.sendall(b"AUTH ANONYMOUS\r\n")
            r = self.line()
            if not r.startswith("OK"):
                raise RuntimeError("D-Bus authentication refused: " + r)
        self.s.sendall(b"NEGOTIATE_UNIX_FD\r\n")
        self.fds_ok = self.line().startswith("AGREE")
        self.s.sendall(b"BEGIN\r\n")

    def send(self, typ, fields, sig="", args=(), fds=(), flags=0):
        body = Writer()
        for t, v in zip(split_sig(sig), args):
            body.put(t, v)
        with self.lock:
            self.serial += 1
            serial = self.serial
            fl = dict(fields)
            if sig:
                fl[F_SIG] = sig
            if fds:
                fl[F_FDS] = len(fds)
            h = Writer()
            h.b += struct.pack("<cBBBII", b"l", typ, flags, 1, len(body.b), serial)
            h.put("a(yv)", [(k, (FIELD_TYPE[k], v)) for k, v in fl.items()])
            h.pad(8)
            msg = bytes(h.b + body.b)
            if fds:
                self.s.sendmsg([msg], [(socket.SOL_SOCKET, socket.SCM_RIGHTS, array.array("i", fds))])
            else:
                self.s.sendall(msg)
        return serial

    def call(self, path, iface, member, sig="", args=(), fds=()):
        return self.send(CALL, {F_PATH: path, F_INTERFACE: iface, F_MEMBER: member}, sig, args, fds)

    def reply(self, m, sig="", args=()):
        if m["flags"] & 1:  # NO_REPLY_EXPECTED
            return
        self.send(RETURN, {F_REPLY: m["serial"]}, sig, args)

    def error(self, m, name, text):
        self.send(ERROR, {F_REPLY: m["serial"], F_ERROR: name}, "s", [text])

    def need(self, n):
        while len(self.buf) < n:
            d = self.s.recv(1 << 16)
            if not d:
                raise EOFError
            self.buf += d

    def recv(self):
        self.need(16)
        if self.buf[0:1] != b"l":
            raise RuntimeError("big-endian D-Bus message")
        typ, flags, _, blen, serial, flen = struct.unpack_from("<BBBIII", self.buf, 1)
        hlen = (16 + flen + 7) // 8 * 8
        self.need(hlen + blen)
        r = Reader(self.buf, 12)
        fields = dict((k, v[1]) for k, v in r.get("a(yv)"))
        body = self.buf[hlen:hlen + blen]
        self.buf = self.buf[hlen + blen:]
        sig = fields.get(F_SIG, "")
        br = Reader(body)
        args = [br.get(t) for t in split_sig(sig)]
        return {"type": typ, "flags": flags, "serial": serial, "member": fields.get(F_MEMBER),
                "path": fields.get(F_PATH), "iface": fields.get(F_INTERFACE),
                "reply": fields.get(F_REPLY), "error": fields.get(F_ERROR), "args": args}


# ---- Sound ----

def load_wav(path):
    """Frames of a PCM WAV file as (rate, channels, list of int16 per channel interleaved)."""
    b = open(path, "rb").read()
    if b[:4] != b"RIFF" or b[8:12] != b"WAVE":
        sys.exit(f"{path}: not a WAV file")
    p, fmt, data = 12, None, None
    while p + 8 <= len(b):
        cid, n = b[p:p + 4], struct.unpack_from("<I", b, p + 4)[0]
        if cid == b"fmt ":
            fmt = struct.unpack_from("<HHIIHH", b, p + 8)
        elif cid == b"data":
            data = b[p + 8:p + 8 + n]
        p += 8 + n + (n & 1)
    if not fmt or data is None or fmt[0] != 1 or fmt[5] not in (8, 16):
        sys.exit(f"{path}: not 8- or 16-bit PCM")
    _, ch, rate, _, _, bits = fmt
    if bits == 8:
        s = [(v - 128) << 8 for v in data]
    else:
        s = list(array.array("h", data[:len(data) // 2 * 2]))
    return rate, ch, s


class Source:
    """What the guest records: a looped WAV file or a sine, delivered in
    the format QEMU asks for (signed 16-bit assumed)."""

    def __init__(self, spec):
        if spec.startswith("tone:"):
            self.tone = float(spec[5:])
        else:
            self.tone = None
            self.rate, self.ch, self.s = load_wav(spec)
            self.frames = len(self.s) // self.ch
        self.t = 0.0  # position in source frames (or seconds for the tone)

    def read(self, nbytes, rate, ch, bits):
        bpf = ch * bits // 8
        n = nbytes // bpf
        out = array.array("h")
        for _ in range(n):
            if self.tone is not None:
                v = int(math.sin(2 * math.pi * self.tone * self.t) * 32767 / 3)
                self.t += 1.0 / rate
                if self.t >= 1000.0:
                    self.t -= 1000.0
                fr = [v, v]
            else:
                i = int(self.t) % self.frames
                fr = [self.s[i * self.ch + min(c, self.ch - 1)] for c in range(2)]
                self.t = (self.t + self.rate / rate) % self.frames
            out.extend(fr[:ch] if ch <= 2 else fr + [0] * (ch - 2))
        if bits == 8:
            return bytes(((v >> 8) + 128) & 0xff for v in out)
        return out.tobytes()


class WavOut:
    def __init__(self, path):
        self.f = open(path, "wb")
        self.n = 0
        self.fmt = (48000, 2, 16)
        self.header()

    def header(self):
        rate, ch, bits = self.fmt
        self.f.seek(0)
        self.f.write(b"RIFF" + struct.pack("<I", 36 + self.n) + b"WAVEfmt " +
                     struct.pack("<IHHIIHH", 16, 1, ch, rate, rate * ch * bits // 8, ch * bits // 8, bits) +
                     b"data" + struct.pack("<I", self.n))
        self.f.seek(0, 2)

    def write(self, d):
        self.f.write(d)
        self.n += len(d)
        self.header()
        self.f.flush()


def listener(sock, kind, path, src, out):
    """Serve QEMU's calls on a listener connection."""
    c = Conn(sock)
    voices = {}
    try:
        while True:
            m = c.recv()
            if m["type"] != CALL:
                continue
            mem, a = m["member"], m["args"]
            if mem == "Init":
                # id, bits, is_signed, is_float, freq, nchannels, ...
                voices[a[0]] = (a[4], a[5], a[1])
                print(f"soundin: {kind} voice {a[4]} Hz, {a[5]} channels, {a[1]} bits", file=sys.stderr)
                if out and kind == "out":
                    out.fmt = (a[4], a[5], a[1])
                    out.header()
                c.reply(m)
            elif mem == "Read":
                rate, ch, bits = voices.get(a[0], (48000, 2, 16))
                c.reply(m, "ay", [src.read(a[1], rate, ch, bits)])
            elif mem == "Write":
                if out:
                    out.write(a[1])
                c.reply(m)
            elif mem == "GetAll":
                c.reply(m, "a{sv}", [[]])
            elif m["iface"] == "org.freedesktop.DBus.Introspectable":
                c.reply(m, "s", ["<node/>"])
            else:
                c.reply(m)
    except EOFError:
        pass


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    qmp_path, spec = sys.argv[1], sys.argv[2]
    out = WavOut(sys.argv[3]) if len(sys.argv) > 3 else None
    src = Source(spec)
    q = socket.socket(socket.AF_UNIX)
    for _ in range(100):
        try:
            q.connect(qmp_path)
            break
        except OSError:
            import time
            time.sleep(0.1)
    else:
        sys.exit("soundin: no QMP socket")
    qf = q.makefile("rb")

    def qmp(cmd, fds=()):
        data = (cmd + "\r\n").encode()
        if fds:
            q.sendmsg([data], [(socket.SOL_SOCKET, socket.SCM_RIGHTS, array.array("i", fds))])
        else:
            q.sendall(data)
        while True:
            r = qf.readline().decode()
            if '"return"' in r or '"error"' in r:
                if '"error"' in r:
                    sys.exit("soundin: QMP: " + r)
                return r

    qf.readline()  # greeting
    qmp('{"execute":"qmp_capabilities"}')
    mine, theirs = socket.socketpair()
    qmp('{"execute":"getfd","arguments":{"fdname":"soundin"}}', [theirs.fileno()])
    qmp('{"execute":"add_client","arguments":{"protocol":"@dbus-display","fdname":"soundin"}}')
    theirs.close()
    q.close()
    c = Conn(mine)
    kinds = [("in", "RegisterInListener", "/org/qemu/Display1/AudioInListener")]
    if out:
        kinds.append(("out", "RegisterOutListener", "/org/qemu/Display1/AudioOutListener"))
    for kind, method, path in kinds:
        a, b = socket.socketpair()
        serial = c.call("/org/qemu/Display1/Audio", "org.qemu.Display1.Audio", method, "h", [0], [b.fileno()])
        b.close()
        # QEMU authenticates the listener connection before it answers.
        threading.Thread(target=listener, args=(a, kind, path, src, out), daemon=True).start()
        while True:
            m = c.recv()
            if m["reply"] == serial:
                if m["type"] == ERROR:
                    sys.exit(f"soundin: {method}: {m['error']} {m['args']}")
                break
        print(f"soundin: {kind} listener registered", file=sys.stderr)
    try:
        while True:
            c.recv()
    except EOFError:
        pass


if __name__ == "__main__":
    main()
