"""A TFTP server (RFC 1350, octet and netascii, 512-byte blocks) on the
host that loses packets on purpose, to check that a client retransmits
(IBM's 16-bit TFTP times out with TCPIPDLL's _alarm/_xsignal).

It serves and stores files in ROOT. The first transmission of DATA
block 3 of each read and the first ACK of block 5 of each write are not
sent; the client must time out and send its last packet again. Each
transfer is logged with the retransmissions it needed.

  python3 -I tftplossy.py ROOT [port]       (port 6969)

Then in the guest (QEMU user networking: the host is 10.0.2.2):
  tftp 10.0.2.2 6969   binary   get big.bin C:\\BIG.BIN   put C:\\BIG.BIN back.bin
"""
import os, socket, struct, sys, threading, time

ROOT = os.path.abspath(sys.argv[1])
PORT = int(sys.argv[2]) if len(sys.argv) > 2 else 6969
os.makedirs(ROOT, exist_ok=True)


def log(*a):
    print(time.strftime('%H:%M:%S'), *a, flush=True)


def transfer(op, name, peer):
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.bind(('0.0.0.0', 0))
    # The server never retransmits on its own: only a repeated packet from
    # the client brings the lost one again.
    s.settimeout(1)
    path = os.path.join(ROOT, os.path.basename(name))
    resent = 0
    if op == 1:
        try:
            data = open(path, 'rb').read()
        except OSError:
            s.sendto(struct.pack('>HH', 5, 1) + b'File not found\0', peer)
            return
        block, dropped = 1, False
        while True:
            chunk = data[(block - 1) * 512:block * 512]
            pkt = struct.pack('>HH', 3, block) + chunk
            tries = 0
            if block == 3 and not dropped:
                dropped = True
                log('tftp: block 3 dropped on purpose')
            else:
                s.sendto(pkt, peer)
            while True:
                try:
                    r, a = s.recvfrom(1024)
                except socket.timeout:
                    tries += 1
                    if tries > 30:
                        log('tftp: read of %s gave up at block %d' % (name, block))
                        return
                    continue
                op2, n = struct.unpack('>HH', r[:4])
                if op2 == 4 and n == block:
                    break
                if op2 == 4 and n == block - 1:
                    # The client timed out and repeated its last ACK.
                    resent += 1
                    s.sendto(pkt, peer)
            if len(chunk) < 512:
                break
            block += 1
        log('tftp: sent %s (%d bytes) to %s; the client retransmitted %d time(s)' % (name, len(data), peer, resent))
    else:
        out = bytearray()
        block, dropped = 0, False
        s.sendto(struct.pack('>HH', 4, 0), peer)
        waits = 0
        while True:
            try:
                r, a = s.recvfrom(2048)
                waits = 0
            except socket.timeout:
                waits += 1
                if waits > 30:
                    log('tftp: write of %s timed out after block %d' % (name, block))
                    return
                continue
            op2, n = struct.unpack('>HH', r[:4])
            if op2 != 3:
                continue
            if n == block:
                resent += 1
                s.sendto(struct.pack('>HH', 4, n), peer)
                continue
            if n != block + 1:
                continue
            block = n
            out += r[4:]
            if block == 5 and not dropped:
                dropped = True
                log('tftp: ACK 5 dropped on purpose')
                continue
            s.sendto(struct.pack('>HH', 4, block), peer)
            if len(r) - 4 < 512:
                break
        open(path, 'wb').write(out)
        log('tftp: received %s (%d bytes) from %s; the client retransmitted %d time(s)' % (name, len(out), peer, resent))


srv = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
srv.bind(('0.0.0.0', PORT))
log('tftp: serving', ROOT, 'on', PORT)
while True:
    r, peer = srv.recvfrom(1024)
    op = struct.unpack('>H', r[:2])[0]
    if op not in (1, 2):
        continue
    name = r[2:].split(b'\0')[0].decode('latin1')
    log('tftp:', 'RRQ' if op == 1 else 'WRQ', name, 'from', peer)
    threading.Thread(target=transfer, args=(op, name, peer), daemon=True).start()
