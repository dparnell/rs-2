#!/usr/bin/env python3
# Writes rs2's own Mahjongg layouts as OS/2 Mahjongg game files (.MAH):
# python3 -I gen.py (in this directory). Each has 144 tiles; positions in
# tile units, y upwards.

import os

here = os.path.dirname(os.path.abspath(__file__))


def rect(z, x0, y0, nx, ny):
    return [(z, x0 + i, y0 + j) for j in range(ny) for i in range(nx)]


def pyramid():
    t = rect(0, 0, 0, 12, 6) + rect(1, 1, 1, 10, 4) + rect(2, 2, 2, 8, 2)
    t += rect(3, 3, 2, 6, 2) + [(4, 4.5, 2.5), (4, 5.5, 2.5), (4, 6.5, 2.5), (4, 3.5, 2.5)]
    return dict(name='Pyramid', about='Five steps up to a narrow top', dx=12, dy=6, xs=0.2, ys=6.2, color=(150, 110, 60), tiles=t)


def fortress():
    wall = [(x, y) for x in range(14) for y in range(8) if x in (0, 13) or y in (0, 7)]
    t = [(0, x, y) for x, y in wall] + rect(0, 3, 2, 8, 4) + [(1, x, y) for x, y in wall]
    for z in range(2, 6):
        t += [(z, 0, 0), (z, 13, 0), (z, 0, 7), (z, 13, 7)]
    t += rect(1, 5, 3, 4, 2) + rect(2, 5, 3, 4, 2)
    return dict(name='Fortress', about='Walls, four towers and a keep', dx=14, dy=8, xs=5.0, ys=6.1, color=(70, 90, 120), tiles=t)


def twin_peaks():
    t = []
    for x0 in (0, 8):
        t += rect(0, x0, 0, 6, 6) + rect(1, x0 + 0.5, 0.5, 5, 5) + rect(2, x0 + 1.5, 1.5, 3, 3) + [(3, x0 + 2.5, 2.5)]
    t += [(0, 6, 2.5), (0, 7, 2.5)]
    return dict(name='Twin Peaks', about='Two hills and the path between them', dx=14, dy=6, xs=6.0, ys=5.0, color=(40, 110, 140), tiles=t)


def fmt(v):
    return '%4.1f' % v


def write(fn, g):
    tiles = g['tiles']
    assert len(tiles) == 144, (g['name'], len(tiles))
    for i, a in enumerate(tiles):
        for b in tiles[i + 1:]:
            assert not (a[0] == b[0] and abs(a[1] - b[1]) < 1 and abs(a[2] - b[2]) < 1), (g['name'], a, b)
    out = [':GAME  type    = "Mahjongg Solitaire 2.00"',
           '       name    = "%s"' % g['name'],
           '       help    = ""',
           '       tileset = "DEFAULT"',
           '       author  = "rs2"',
           '       about   = "%s"' % g['about'],
           '       .',
           ':BOARD dx      = %d' % g['dx'],
           '       dy      = %d' % g['dy'],
           "       x_stat  = '%.1f'" % g['xs'],
           "       y_stat  = '%.1f'" % g['ys'],
           '       bitmap  = "DEFAULT"',
           '       color   = "RGB(%d,%d,%d)"' % g['color'],
           '       use     = "COLOR"',
           '       .']
    for z in range(max(t[0] for t in tiles) + 1):
        out.append(':LEVEL %d.' % z)
        on = [t for t in tiles if t[0] == z]
        rows = [on[i:i + 6] for i in range(0, len(on), 6)]
        for k, r in enumerate(rows):
            s = ', '.join('(%s, %s)' % (fmt(x), fmt(y)) for _, x, y in r)
            out.append('  ' + s + (',' if k + 1 < len(rows) else ''))
    out += [':ELEVEL.', ':EGAME.']
    open(os.path.join(here, fn), 'w', newline='').write('\r\n'.join(out) + '\r\n')


write('PYRAMID.MAH', pyramid())
write('FORTRESS.MAH', fortress())
write('TWINPEAK.MAH', twin_peaks())
# OS/2's Mahjongg folder has a shadow of DEFAULT.MAH, which stands for the
# built-in layout; rs2's file says so.
open(os.path.join(here, 'DEFAULT.MAH'), 'w', newline='').write('rs2 Mahjongg: this file stands for the built-in Turtle layout.\r\n')
