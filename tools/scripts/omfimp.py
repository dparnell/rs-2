"""Dump IMPDEF records from OMF import libraries as TSV: MODULE ORDINAL NAME.

usage: python3 -I omfimp.py LIB...
"""
import struct, sys

def pstr(b, o):
    n = b[o]
    return b[o + 1:o + 1 + n].decode('latin1'), o + 1 + n

def scan(path):
    d = open(path, 'rb').read()
    if d[0] != 0xF0:
        return
    page = struct.unpack_from('<H', d, 1)[0] + 3
    o = page
    while o + 3 <= len(d):
        t, n = d[o], struct.unpack_from('<H', d, o + 1)[0]
        if t == 0xF1:  # library end
            break
        body = d[o + 3:o + 3 + n - 1]
        if t == 0x88 and len(body) > 3 and body[1] == 0xA0 and body[2] == 0x01:
            byord = body[3]
            internal, p = pstr(body, 4)
            module, p = pstr(body, p)
            if byord:
                ordinal = struct.unpack_from('<H', body, p)[0]
                yield module, ordinal, internal
            else:
                ext, p = pstr(body, p)
                yield module, None, ext or internal
        o += 3 + n
        if t in (0x8A, 0x8B):  # MODEND: next member starts on a page boundary
            o = (o + page - 1) // page * page

for path in sys.argv[1:]:
    for m, ordinal, name in scan(path):
        print(f"{m}\t{'' if ordinal is None else ordinal}\t{name}")
