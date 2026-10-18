"""Derive 16-bit _Pascal signatures for VIO/KBD/MOU from bsesub.h.

usage: python3 -I gen_sigs16.py bsesub.h names_gen.rs
Prints Rust table bodies (ordinal, "sig") per module.
"""
import re, sys
src = open(sys.argv[1], encoding='latin1').read()
src = re.sub(r'/\*.*?\*/', '', src, flags=re.S)
protos = {}
for m in re.finditer(r'APIRET16\s+APIENTRY16\s+(\w+)\s*\(([^;]*?)\)\s*;', src, re.S):
    name, args = m.group(1), m.group(2).strip()
    sig = ''
    if args and args.upper() not in ('VOID',):
        for a in args.split(','):
            t = a.split()[0]
            if t.startswith('P') or t in ('PSZ', 'PCH'):
                sig += 'p'
            elif t in ('ULONG', 'LONG'):
                sig += 'd'
            else:
                sig += 'w'
    protos[name.upper()] = (name, sig)
mods = {1: [], 2: [], 3: []}
for line in open(sys.argv[2]):
    m = re.match(r'\s+\((\d+), (\d+), (true|false), "(\w+)"\)', line)
    if not m:
        continue
    mi, o, n = int(m.group(1)), int(m.group(2)), m.group(4)
    if mi in mods and n.upper() in protos:
        nm, sig = protos[n.upper()]
        mods[mi].append((o, sig, nm))
for mi, name in [(1, 'VIOCALLS'), (2, 'KBDCALLS'), (3, 'MOUCALLS')]:
    print(f'const {name}: &[(u16, &str)] = &[')
    for o, sig, nm in mods[mi]:
        print(f'    ({o}, "{sig}"), // {nm}')
    print('];\n')
