"""Derive the 8x14 and 8x8 text fonts from the 8x16 one.

usage: python3 -I mksmallfonts.py kernel/assets/unscii-16.bin kernel/assets

Writes unscii-14.bin and unscii-8.bin in the kernel's font format (sorted
records of <u32 codepoint LE><one byte per glyph row>). Source font:
unscii-16 by viznut (public domain), http://viznut.fi/unscii/ (see
mkfont.py). The rows of every glyph are merged in fixed groups, so the
baseline, the x-height and box-drawing lines line up across glyphs:

* 14 rows: rows 0-1 and 14-15 are ORed together (they are blank in most
  glyphs; accents and descenders keep their pixels).
* 8 rows: two rows for the cap height above the x-height, five for the
  x-height, one for descenders. Text glyphs keep the fullest row of each
  group; box drawing, block and shade characters OR the group so lines stay
  continuous. A few glyphs (digits 0 4 8, $ % & X Y) are drawn by hand.
"""
import os, struct, sys

ROWS14 = [(0, 2)] + [(r, r + 1) for r in range(2, 14)] + [(14, 16)]
ROWS8 = [(0, 4), (4, 6), (6, 7), (7, 9), (9, 10), (10, 12), (12, 13), (13, 16)]


# 8x8 glyphs drawn by hand where merging rows loses the shape.
OVERRIDES8 = {
    '0': ['..####..', '.##..##.', '.##.###.', '.###.##.', '.##..##.', '.##..##.', '..####..', '........'],
    '4': ['....##..', '...###..', '..####..', '.##.##..', '.######.', '....##..', '....##..', '........'],
    '8': ['..####..', '.##..##.', '.##..##.', '..####..', '.##..##.', '.##..##.', '..####..', '........'],
    '$': ['...##...', '..#####.', '.##.....', '..####..', '.....##.', '.#####..', '...##...', '........'],
    '%': ['........', '##...##.', '##..##..', '...##...', '..##....', '.##..##.', '##...##.', '........'],
    '&': ['..###...', '.##.##..', '..###...', '.###.##.', '##.###..', '##..##..', '.###.##.', '........'],
    'X': ['##...##.', '.##.##..', '..###...', '..###...', '..###...', '.##.##..', '##...##.', '........'],
    'Y': ['.##..##.', '.##..##.', '.##..##.', '..####..', '...##...', '...##...', '...##...', '........'],
}


def bitmap(rows):
    return bytes(int(r.replace('.', '0').replace('#', '1'), 2) for r in rows)


def merge(rows, groups, use_or):
    out = []
    for a, b in groups:
        part = rows[a:b]
        if use_or:
            v = 0
            for x in part:
                v |= x
        else:
            v = max(part, key=lambda x: bin(x).count('1'))
        out.append(v)
    return bytes(out)


src = open(sys.argv[1], 'rb').read()
glyphs = []
for i in range(len(src) // 20):
    cp = struct.unpack_from('<I', src, i * 20)[0]
    glyphs.append((cp, src[i * 20 + 4:i * 20 + 20]))
for name, groups in (('unscii-14.bin', ROWS14), ('unscii-8.bin', ROWS8)):
    with open(os.path.join(sys.argv[2], name), 'wb') as f:
        for cp, rows in glyphs:
            lines = 0x2500 <= cp < 0x25a0
            if len(groups) == 8 and chr(cp) in OVERRIDES8:
                g = bitmap(OVERRIDES8[chr(cp)])
            else:
                g = merge(rows, groups, len(groups) == 14 or lines)
            f.write(struct.pack('<I', cp) + g)
    print(name, len(glyphs), 'glyphs')
