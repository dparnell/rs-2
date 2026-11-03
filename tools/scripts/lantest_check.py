#!/usr/bin/env python3
"""Host side of tools/scripts/lantest.sh.

prepare DATADIR        put the files the guest expects on the share
check DATADIR SPOOLDIR compare what the guest left there (byte for byte)
"""
import os
import sys


def pattern(n, seed):
    """The guest's test pattern (userland/lanreq/src/bin/lantest.rs)."""
    out = bytearray(n)
    x = seed
    for i in range(n):
        x = (x * 1664525 + 1013904223) & 0xFFFFFFFF
        out[i] = x >> 24
    return bytes(out)


def prepare(d):
    with open(os.path.join(d, 'hello.txt'), 'wb') as f:
        f.write(b'Hello from the host\r\n')
    with open(os.path.join(d, 'HOSTDATA.BIN'), 'wb') as f:
        f.write(pattern(5 * 1024 * 1024, 7))


def check(d, spool):
    fails = 0

    def ok(name, cond, detail=''):
        nonlocal fails
        print(('PASS ' if cond else 'FAIL ') + 'host: ' + name + ('' if cond else ' ' + detail))
        fails += 0 if cond else 1

    def read(*p):
        try:
            with open(os.path.join(d, *p), 'rb') as f:
                return f.read()
        except OSError:
            return None

    ok('FROMRS2.BIN written by the guest', read('FROMRS2.BIN') == pattern(5 * 1024 * 1024 + 123, 42))
    host = pattern(5 * 1024 * 1024, 7)
    ok('CMD COPY in and out (BACK.BIN)', read('CMDDIR', 'BACK.BIN') == host)
    ok('XCOPY round trip', read('XCBACK', 'BACK.BIN') == host, str(os.listdir(d)))
    ok('REN and DEL (CONFIG.TXT gone)', read('CMDDIR', 'CONFIG.TXT') is None and read('CMDDIR', 'CONFIG.SYS') is None)
    ok('RD (EMPTY gone)', not os.path.exists(os.path.join(d, 'EMPTY')))
    jobs = sorted(os.listdir(spool)) if os.path.isdir(spool) else []
    texts = [open(os.path.join(spool, j), 'rb').read() for j in jobs]
    ok('print job from LPT2', any(b'rs2 LAN printing' in t for t in texts), str(jobs))
    ok('print job from COPY to LPT3', len(jobs) >= 2, str(jobs))
    print('host check: %d failed' % fails)
    return 1 if fails else 0


if __name__ == '__main__':
    if sys.argv[1] == 'prepare':
        prepare(sys.argv[2])
    else:
        sys.exit(check(sys.argv[2], sys.argv[3]))
