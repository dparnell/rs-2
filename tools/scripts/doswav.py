#!/usr/bin/env python3
"""Tones in a DOS session's sound, as `dosrun --wav` or QEMU recorded it.

    python3 -I tools/scripts/doswav.py FILE.wav [EXPECT...]

Prints each channel's tones (sound of one pitch, in 20 ms windows):
start, length and frequency (rising zero crossings). Each
EXPECT is `CHANNEL:HZ:SECONDS` (CHANNEL L, R or B for both): a tone of
that frequency (within 3%) and length (within 15%, at least 40 ms) must
be there, in the order given per channel. Exit code 0 when all are.
"""

import struct
import sys

LOUD = 300
GAP_S = 0.020
MIN_TONE_S = 0.030


def read(path):
    b = open(path, "rb").read()
    if b[:4] != b"RIFF" or b[8:12] != b"WAVE":
        sys.exit(f"{path}: not a WAV file")
    pos, fmt, data = 12, None, None
    while pos + 8 <= len(b):
        cid, size = b[pos:pos + 4], struct.unpack("<I", b[pos + 4:pos + 8])[0]
        body = b[pos + 8:pos + 8 + size] if size else b[pos + 8:]
        if cid == b"fmt ":
            fmt = struct.unpack("<HHIIHH", body[:16])
        elif cid == b"data":
            data = body
            break
        pos += 8 + size + (size & 1)
    _, ch, rate, _, _, bits = fmt
    if bits == 16:
        n = len(data) // 2
        s = struct.unpack(f"<{n}h", data[:n * 2])
    else:
        s = [(v - 128) * 256 for v in data]
    chans = [s[i::ch] for i in range(ch)]
    if ch == 1:
        chans.append(chans[0])
    return rate, chans[0], chans[1]


def tones(rate, s):
    """(start, seconds, Hz) of each tone: 20 ms windows with sound, joined
    while their frequency stays within 6% (a change of pitch starts a new
    tone, as does a silence of more than 20 ms)."""
    win = int(rate * 0.02)
    wins = []
    for w in range(0, len(s) - win + 1, win):
        seg = s[w:w + win]
        if max(abs(v) for v in seg) < LOUD:
            wins.append(None)
            continue
        ups = [k for k in range(1, win) if seg[k - 1] < 0 <= seg[k]]
        hz = (len(ups) - 1) * rate / (ups[-1] - ups[0]) if len(ups) > 2 else 0
        wins.append(hz)
    out = []
    cur = None
    for i, hz in enumerate(wins + [None]):
        if cur and hz is not None and abs(hz - cur[2] / cur[1]) <= 0.06 * max(cur[2] / cur[1], 1):
            cur[1] += 1
            cur[2] += hz
            continue
        if cur and cur[1] * 0.02 >= MIN_TONE_S:
            out.append((cur[0] * 0.02, cur[1] * 0.02, cur[2] / cur[1]))
        cur = [i, 1, hz] if hz is not None else None
    return out


def main():
    rate, left, right = read(sys.argv[1])
    found = {"L": tones(rate, left), "R": tones(rate, right)}
    for c in "LR":
        for t, secs, hz in found[c]:
            print(f"{c} {t:7.3f}s {secs:6.3f}s {hz:8.1f} Hz")
    bad = 0
    pos = {"L": 0, "R": 0}
    for e in sys.argv[2:]:
        c, hz, secs = e.split(":")
        hz, secs = float(hz), float(secs)
        for ch in ("LR" if c == "B" else c):
            lst = found[ch]
            k = pos[ch]
            while k < len(lst) and not (abs(lst[k][2] - hz) <= hz * 0.03 and abs(lst[k][1] - secs) <= max(secs * 0.15, 0.04)):
                k += 1
            if k == len(lst):
                print(f"missing: {ch} {hz} Hz {secs} s")
                bad += 1
            else:
                pos[ch] = k + 1
    print("sound OK" if bad == 0 else f"sound FAILED: {bad} missing")
    sys.exit(1 if bad else 0)


main()
