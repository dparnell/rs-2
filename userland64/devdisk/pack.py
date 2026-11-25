"""pack.py DIR ZIP TIME: a zip archive of a directory, the same for the same
files (TIME: theirs, seconds since 1970). Text that OS/2 programs read
(.CMD, .TXT) gets CRLF line ends."""

import os
import sys
import time
import zipfile

top, out, when = sys.argv[1:]
stamp = time.gmtime(int(when))[:6]
names = []
for d, dirs, files in os.walk(top):
    dirs.sort()
    names += [os.path.relpath(os.path.join(d, f), top).replace(os.sep, "/") for f in files]
# OS/2's names have no case, and some are devices.
seen = {}
for n in sorted(names):
    if n.lower() in seen:
        sys.exit(f"pack.py: {n} and {seen[n.lower()]} are one file on OS/2")
    seen[n.lower()] = n
    stem = n.rsplit("/", 1)[-1].split(".")[0].upper()
    if stem in ("CON", "PRN", "NUL", "AUX", "CLOCK$", "KBD$", "SCREEN$") or stem[:3] in ("COM", "LPT") and stem[3:] in tuple("123456789"):
        print(f"pack.py: left out, a device's name: {n}")
        names.remove(n)
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
    for n in sorted(names):
        with open(os.path.join(top, n), "rb") as f:
            data = f.read()
        base = n.removesuffix(".rs2in").upper()
        if "/vendor/" not in n and base.endswith((".CMD", "README.TXT", "VERSION.TXT", "LICENSE.TXT")):
            data = data.replace(b"\r\n", b"\n").replace(b"\n", b"\r\n")
        i = zipfile.ZipInfo(n, stamp)
        i.compress_type = zipfile.ZIP_DEFLATED
        i.external_attr = 0o644 << 16
        z.writestr(i, data)
