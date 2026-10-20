"""Name imports in an lximage symbols.txt: imp_PMWIN_925 -> PMWIN_WinQueryClassInfo.

usage: python3 -I tools/ghidra/impnames.py REPO SYMBOLS.TXT
Names come from rs2's own export lists (userland/*/exports.def, which use
IBM's ordinals) and the kernel's API name table (kernel/src/api/names_gen.rs).
"""
import glob, os, re, sys

repo, path = sys.argv[1], sys.argv[2]
names = {}
for d in glob.glob(os.path.join(repo, "userland", "*", "exports.def")):
    mod = os.path.basename(os.path.dirname(d)).upper()
    for line in open(d, errors="replace"):
        m = re.match(r"\s*([A-Za-z_]\w*)(?:=\S+)?\s+@(\d+)", line)
        if m:
            names[(mod, m.group(2))] = m.group(1)
for m in re.finditer(r'\("(\w+)", "(\w+)", (\d+)\)', open(os.path.join(repo, "kernel/src/api/names_gen.rs")).read()):
    names.setdefault((m.group(1), m.group(3)), m.group(2))
out = []
for line in open(path):
    m = re.match(r"(\S+) imp_(\w+?)_(\d+)$", line.strip())
    if m and (m.group(2), m.group(3)) in names:
        line = "%s %s_%s\n" % (m.group(1), m.group(2), names[(m.group(2), m.group(3))])
    out.append(line)
open(path, "w").writelines(out)
