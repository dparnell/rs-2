"""Convert a GNU unifont-style .hex font (8x16 glyphs) to the kernel's
binary format: sorted records of <u32 codepoint LE><16 bytes rows>.

usage: python3 -I mkfont.py unscii-16.hex out.bin
Source font: unscii-16 by viznut (public domain), http://viznut.fi/unscii/
"""
import struct, sys
out = []
for line in open(sys.argv[1]):
    cp, bits = line.strip().split(':')
    if len(bits) != 32:
        continue
    out.append((int(cp, 16), bytes.fromhex(bits)))
out.sort()
with open(sys.argv[2], 'wb') as f:
    for cp, b in out:
        f.write(struct.pack('<I', cp) + b)
print(len(out), 'glyphs')
