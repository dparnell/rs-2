"""The host's side of a shared clipboard through QEMU's VNC server (RFB
3.8 with the extended clipboard pseudo-encoding, UTF-8; classic Latin-1
cut text as well), for testing the guest agent headless (`cargo xtask run
--agent --headless` puts the server on work/qemu-vnc.sock).

usage: python3 -I vnc_clipboard.py SOCKET [--set TEXT] [--wait SECONDS]

Connects, offers TEXT as the host's clipboard (if given), and prints
every text the guest puts on the clipboard (`GUEST: ...`) until SECONDS
have passed (default 10).
"""
import socket, struct, sys, time, zlib

EXT = 0xC0A1E5CE - (1 << 32)  # the pseudo-encoding as a signed number
CAPS, REQUEST, PEEK, NOTIFY, PROVIDE = 1 << 24, 1 << 25, 1 << 26, 1 << 27, 1 << 28
TEXT = 1


def recv(s, n):
    b = b''
    while len(b) < n:
        c = s.recv(n - len(b))
        if not c:
            raise EOFError
        b += c
    return b


def main():
    args = sys.argv[1:]
    path, offer, wait = args[0], None, 10.0
    i = 1
    while i < len(args):
        if args[i] == '--set':
            offer = args[i + 1]; i += 2
        elif args[i] == '--wait':
            wait = float(args[i + 1]); i += 2
        else:
            sys.exit(__doc__)
    s = socket.socket(socket.AF_UNIX)
    s.connect(path)
    recv(s, 12)
    s.sendall(b'RFB 003.008\n')
    n = recv(s, 1)[0]
    types = recv(s, n)
    assert 1 in types, types
    s.sendall(b'\x01')
    assert struct.unpack('>I', recv(s, 4))[0] == 0
    s.sendall(b'\x01')  # shared
    w, h = struct.unpack('>HH', recv(s, 4))
    recv(s, 16)
    name_len = struct.unpack('>I', recv(s, 4))[0]
    recv(s, name_len)
    # SetEncodings: raw and the extended clipboard.
    s.sendall(struct.pack('>BxHii', 2, 2, 0, EXT))

    def ext_send(flags, payload=b''):
        body = struct.pack('>I', flags) + payload
        s.sendall(struct.pack('>BxxxI', 6, (-len(body)) & 0xffffffff) + body)

    def provide(text):
        data = text.encode('utf-8') + b'\0'
        ext_send(PROVIDE | TEXT, zlib.compress(struct.pack('>I', len(data)) + data))

    ext = False
    if offer is not None:
        # Classic first; replaced by the extended form once the server
        # says it has it.
        b = offer.encode('latin-1', 'replace')
        s.sendall(struct.pack('>BxxxI', 6, len(b)) + b)
    end = time.time() + wait
    s.settimeout(0.5)
    while time.time() < end:
        try:
            t = recv(s, 1)[0]
        except socket.timeout:
            continue
        except EOFError:
            break
        s.settimeout(None)
        if t == 3:
            recv(s, 3)
            ln = struct.unpack('>i', recv(s, 4))[0]
            if ln >= 0:
                print('GUEST:', recv(s, ln).decode('latin-1'), flush=True)
            else:
                body = recv(s, -ln)
                flags = struct.unpack('>I', body[:4])[0]
                if flags & CAPS:
                    ext = True
                    ext_send(CAPS | REQUEST | PEEK | NOTIFY | PROVIDE | TEXT, struct.pack('>I', 1 << 20))
                    if offer is not None:
                        provide(offer)
                        print('HOST: offered', repr(offer), flush=True)
                elif flags & NOTIFY and flags & TEXT:
                    ext_send(REQUEST | TEXT)
                elif flags & REQUEST and flags & TEXT and offer is not None:
                    provide(offer)
                elif flags & PROVIDE:
                    raw = zlib.decompress(body[4:])
                    size = struct.unpack('>I', raw[:4])[0]
                    print('GUEST:', raw[4:4 + size].rstrip(b'\0').decode('utf-8', 'replace'), flush=True)
        elif t == 2:
            pass
        elif t == 1:
            recv(s, 3)
            n = struct.unpack('>H', recv(s, 2))[0]
            recv(s, 6 * n)
        elif t == 0:
            sys.exit('unexpected framebuffer update')
        s.settimeout(0.5)


main()
