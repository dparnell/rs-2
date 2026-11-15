#!/usr/bin/env python3
"""Turn an rs2 crash QR code back into the crash report.

The panic screen shows a QR code holding "RS2C1:" + base45(zlib(report)).
Scan it with any phone QR reader and paste the text, or give a photo or
screenshot (decoded with macOS Core Image through qrdecode.swift):

    python3 -I tools/scripts/crashqr.py 'RS2C1:...'
    python3 -I tools/scripts/crashqr.py photo.jpg
    pbpaste | python3 -I tools/scripts/crashqr.py
"""
import os
import subprocess
import sys
import zlib

CHARSET = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:"


def b45decode(s):
    # Some readers trim a trailing blank (a valid base45 character): a
    # length of 3n+1 can only come from that.
    if len(s) % 3 == 1:
        s += " "
    out = bytearray()
    for i in range(0, len(s), 3):
        chunk = s[i:i + 3]
        n = sum(CHARSET.index(c) * 45 ** k for k, c in enumerate(chunk))
        out += n.to_bytes(2 if len(chunk) == 3 else 1, "big")
    return bytes(out)


def decode(text):
    text = text.strip("\r\n")
    start = text.find("RS2C1:")
    if start < 0:
        raise SystemExit("not an rs2 crash code (no RS2C1: prefix)")
    return zlib.decompress(b45decode(text[start + 6:])).decode("latin-1")


def main():
    arg = sys.argv[1] if len(sys.argv) > 1 else None
    if arg is None:
        text = sys.stdin.read()
    elif os.path.isfile(arg):
        swift = os.path.join(os.path.dirname(os.path.abspath(__file__)), "qrdecode.swift")
        r = subprocess.run(["swift", swift, arg], capture_output=True, text=True)
        if r.returncode != 0:
            raise SystemExit(r.stdout + r.stderr)
        text = next((l for l in r.stdout.splitlines() if "RS2C1:" in l), r.stdout)
    else:
        text = arg
    sys.stdout.write(decode(text))
    if not sys.stdout.isatty():
        return
    print()


if __name__ == "__main__":
    main()
