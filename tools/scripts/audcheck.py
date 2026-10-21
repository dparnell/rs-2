#!/usr/bin/env python3
"""Check the sound AUDTEST.EXE played, as QEMU recorded it.

    cargo xtask run --headless --timeout 45 --init "RS2\\AUDTEST.EXE" --sound-wav out.wav
    python3 -I tools/scripts/audcheck.py out.wav

The recording is split into tones at silences of more than 20 ms; each
tone's length and frequency (rising zero crossings; Goertzel filters for
the two-stream mix) must match what AUDTEST plays (see the table in
userland/audtest/src/main.rs). A dropout inside a tone splits it and
fails the check. Exit code 0 when everything matches.
"""

import math
import sys

# (name, frequencies, minimum and maximum length in seconds, parts)
EXPECTED = [
    ("440 Hz 48000/16/stereo", [440], 0.97, 1.03, 1),
    ("880 Hz 22050/8/mono", [880], 0.97, 1.03, 1),
    ("660 Hz 11025/16/mono", [660], 0.48, 0.53, 1),
    ("1000 Hz 44100/16/stereo", [1000], 0.48, 0.53, 1),
    ("330 Hz 8000/8/stereo", [330], 0.48, 0.53, 1),
    ("300 + 1500 Hz, two streams", [300, 1500], 0.97, 1.03, 1),
    ("500 Hz, stopped", [500], 0.2, 0.6, 1),
    ("750 Hz, paused", [750], 0.57, 0.63, 2),
    ("600 Hz, drain timeout", [600], 0.05, 0.6, 1),
]

LOUD = 300          # sample magnitude that counts as sound
GAP_S = 0.020       # silence that ends a tone
MIN_TONE_S = 0.030  # shorter bursts are ignored


def read(path):
    """Sample rate and left-channel samples. QEMU fills in the header's
    sizes only when it exits normally: a killed QEMU leaves them 0, so
    the data runs to the end of the file then."""
    b = open(path, "rb").read()
    if b[:4] != b"RIFF" or b[8:12] != b"WAVE":
        sys.exit(f"{path}: not a WAV file")
    at, fmt, data = 12, None, None
    while at + 8 <= len(b):
        cid, size = b[at:at + 4], int.from_bytes(b[at + 4:at + 8], "little")
        body = b[at + 8:at + 8 + size] if size else b[at + 8:]
        if cid == b"fmt ":
            fmt = body
        elif cid == b"data":
            data = body
            break
        at += 8 + size + (size & 1)
    if fmt is None or data is None:
        sys.exit(f"{path}: no fmt or data chunk")
    ch = int.from_bytes(fmt[2:4], "little")
    rate = int.from_bytes(fmt[4:8], "little")
    bits = int.from_bytes(fmt[14:16], "little")
    if bits != 16:
        sys.exit(f"{path}: {bits}-bit samples, expected 16")
    data = data[:len(data) - len(data) % (2 * ch)]
    return rate, list(memoryview(data).cast("h")[::ch])


def segments(rate, x):
    gap = int(GAP_S * rate)
    segs, start, quiet = [], None, 0
    for i, v in enumerate(x):
        if abs(v) > LOUD:
            if start is None:
                start = i
            quiet = 0
        elif start is not None:
            quiet += 1
            if quiet > gap:
                segs.append((start, i - quiet + 1))
                start, quiet = None, 0
    if start is not None:
        segs.append((start, len(x)))
    return [(a, b) for a, b in segs if b - a >= MIN_TONE_S * rate]


def zero_cross_hz(rate, x):
    n, last = 0, 0
    for v in x:
        if v > LOUD and last < 0:
            n += 1
        if v > LOUD:
            last = 1
        elif v < -LOUD:
            last = -1
    return n * rate / len(x)


def goertzel(rate, x, f):
    k = 2 * math.cos(2 * math.pi * f / rate)
    s1 = s2 = 0.0
    for v in x:
        s1, s2 = v + k * s1 - s2, s1
    return math.sqrt(max(s1 * s1 + s2 * s2 - k * s1 * s2, 0)) / len(x)


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: audcheck.py FILE.wav")
    rate, x = read(sys.argv[1])
    segs = segments(rate, x)
    print(f"{sys.argv[1]}: {len(x) / rate:.2f} s at {rate} Hz, {len(segs)} tone(s)")
    fails = 0
    i = 0
    for name, freqs, lo, hi, parts in EXPECTED:
        if i + parts > len(segs):
            print(f"FAIL {name}: missing")
            fails += 1
            continue
        mine = segs[i:i + parts]
        i += parts
        length = sum(b - a for a, b in mine) / rate
        tone = [v for a, b in mine for v in x[a:b]]
        where = f"at {mine[0][0] / rate:.2f} s, {length:.3f} s"
        ok = lo <= length <= hi
        if len(freqs) == 1:
            hz = zero_cross_hz(rate, tone)
            ok = ok and abs(hz - freqs[0]) <= freqs[0] * 0.03
            what = f"{hz:.0f} Hz"
        else:
            # Each frequency well above a probe between them.
            probe = goertzel(rate, tone, (freqs[0] + freqs[1]) / 2 + 17)
            mags = [goertzel(rate, tone, f) for f in freqs]
            ok = ok and all(m > 10 * probe + 100 for m in mags)
            what = ", ".join(f"{f} Hz {m:.0f}" for f, m in zip(freqs, mags)) + f" (probe {probe:.0f})"
        print(f"{'PASS' if ok else 'FAIL'} {name}: {where}, {what}")
        fails += not ok
    for a, b in segs[i:]:
        print(f"FAIL unexpected sound at {a / rate:.2f} s, {(b - a) / rate:.3f} s")
        fails += 1
    print(f"audcheck: {fails} failure(s)")
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
