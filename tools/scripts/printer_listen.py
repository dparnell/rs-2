"""A network printer on the host for rs2's printing tests.

usage: python3 -I printer_listen.py lpd|ipp PORT OUTDIR [COUNT]

lpd: an RFC 1179 line printer daemon: accepts "receive a printer job"
     (control and data files, acknowledged one by one) and writes each
     data file to OUTDIR/lpd-N.prn (the control file to lpd-N.ctl).
ipp: an IPP/1.1 printer over HTTP: accepts Print-Job, answers
     successful-ok, and writes the document to OUTDIR/ipp-N.prn (the
     operation attributes to ipp-N.attrs).

It serves COUNT jobs (default 1), then exits; each saved file's name is
printed. rs2's guest reaches the host as 10.0.2.2 under QEMU's user
network.
"""
import os
import socket
import struct
import sys


def read_line(conn):
    b = b''
    while not b.endswith(b'\n'):
        c = conn.recv(1)
        if not c:
            raise EOFError
        b += c
    return b[:-1]


def read_exact(conn, n):
    b = b''
    while len(b) < n:
        c = conn.recv(min(65536, n - len(b)))
        if not c:
            raise EOFError
        b += c
    return b


def lpd_job(conn, out, n):
    cmd = read_line(conn)
    if not cmd.startswith(b'\x02'):
        conn.sendall(b'\x01')
        return None
    queue = cmd[1:]
    conn.sendall(b'\x00')
    saved = []
    while True:
        try:
            sub = read_line(conn)
        except EOFError:
            break
        kind, rest = sub[:1], sub[1:]
        if kind not in (b'\x02', b'\x03'):
            conn.sendall(b'\x01')
            break
        count, name = rest.split(b' ', 1)
        conn.sendall(b'\x00')
        data = read_exact(conn, int(count))
        if read_exact(conn, 1) != b'\x00':
            conn.sendall(b'\x01')
            break
        conn.sendall(b'\x00')
        ext = 'ctl' if kind == b'\x02' else 'prn'
        path = os.path.join(out, 'lpd-%d.%s' % (n, ext))
        with open(path, 'wb') as f:
            f.write(data)
        saved.append(path)
        print('saved %s (%d bytes, queue %s, %s)' % (path, len(data), queue.decode(errors='replace'), name.decode(errors='replace')), flush=True)
    return saved


def ipp_attrs(body):
    """Parse an IPP request's attributes; return (operation, attrs, data)."""
    ver, op, rid = struct.unpack('>HHI', body[:8])
    i = 8
    attrs = []
    name = b''
    while i < len(body):
        tag = body[i]
        i += 1
        if tag == 0x03:
            break
        if tag < 0x10:
            continue
        nlen = struct.unpack('>H', body[i:i + 2])[0]
        i += 2
        if nlen:
            name = body[i:i + nlen]
        i += nlen
        vlen = struct.unpack('>H', body[i:i + 2])[0]
        i += 2
        attrs.append((tag, name.decode(errors='replace'), body[i:i + vlen]))
        i += vlen
    return op, rid, attrs, body[i:]


def ipp_job(conn, out, n):
    head = b''
    while b'\r\n\r\n' not in head:
        c = conn.recv(4096)
        if not c:
            return None
        head += c
    head, rest = head.split(b'\r\n\r\n', 1)
    length = 0
    for line in head.split(b'\r\n')[1:]:
        k, _, v = line.partition(b':')
        if k.strip().lower() == b'content-length':
            length = int(v.strip())
    body = rest + read_exact(conn, length - len(rest))
    op, rid, attrs, data = ipp_attrs(body)
    status = 0x0000 if op == 0x0002 else 0x0501
    resp = struct.pack('>HHI', 0x0101, status, rid) + b'\x01'
    for tag, name, value in ((0x47, b'attributes-charset', b'utf-8'), (0x48, b'attributes-natural-language', b'en')):
        resp += bytes([tag]) + struct.pack('>H', len(name)) + name + struct.pack('>H', len(value)) + value
    resp += b'\x02' + bytes([0x21]) + struct.pack('>H', 6) + b'job-id' + struct.pack('>HI', 4, n)
    resp += b'\x03'
    conn.sendall(b'HTTP/1.1 200 OK\r\nContent-Type: application/ipp\r\nContent-Length: %d\r\nConnection: close\r\n\r\n' % len(resp) + resp)
    path = os.path.join(out, 'ipp-%d.prn' % n)
    with open(path, 'wb') as f:
        f.write(data)
    with open(os.path.join(out, 'ipp-%d.attrs' % n), 'w') as f:
        f.write('operation 0x%04x\n' % op)
        for tag, name, value in attrs:
            f.write('%s 0x%02x %s\n' % (name, tag, value.decode(errors='replace')))
    print('saved %s (%d bytes, operation 0x%04x)' % (path, len(data), op), flush=True)
    return [path]


def main():
    kind, port, out = sys.argv[1], int(sys.argv[2]), sys.argv[3]
    count = int(sys.argv[4]) if len(sys.argv) > 4 else 1
    os.makedirs(out, exist_ok=True)
    s = socket.socket()
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.bind(('127.0.0.1', port))
    s.listen(4)
    print('listening (%s) on port %d' % (kind, port), flush=True)
    n = 0
    while n < count:
        conn, peer = s.accept()
        conn.settimeout(120)
        n += 1
        try:
            (lpd_job if kind == 'lpd' else ipp_job)(conn, out, n)
        except Exception as e:
            print('job %d failed: %s' % (n, e), flush=True)
        finally:
            conn.close()


if __name__ == '__main__':
    main()
