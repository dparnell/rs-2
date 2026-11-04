import zlib, struct, sys
w, h = 160, 100
raw = b''
for y in range(h):
    row = [0]
    for x in range(w):
        a = 255 if (x - 80) ** 2 + (y - 50) ** 2 < 2000 else 120
        row += [x * 255 // w, y * 255 // h, 128, a]
    raw += bytes(row)
def chunk(t, d):
    return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)
png = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 6, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw)) + chunk(b'IEND', b'')
open(sys.argv[1], 'wb').write(png)
