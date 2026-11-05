#!/usr/bin/env python3
"""The dependency closure of RPM names, from a repository's primary.sqlite
(docs/PACKAGES.md): one line per package, its location under the repository
and what pulled it in. Reads the metadata only; versions in requirements are
not compared, so check the result with lximports.py.

usage: python3 -I rpmclosure.py primary.sqlite NAME...
"""
import sqlite3, sys, re, functools, collections
db = sqlite3.connect(sys.argv[1]); roots = sys.argv[2:]
def segs(v): return re.findall(r'\d+|[a-zA-Z]+', v)
def vc(a, b):
    A, B = segs(a), segs(b)
    for x, y in zip(A, B):
        if x.isdigit() and y.isdigit():
            x, y = int(x), int(y)
            if x != y: return (x > y) - (x < y)
        elif x.isdigit(): return 1
        elif y.isdigit(): return -1
        elif x != y: return (x > y) - (x < y)
    return (len(A) > len(B)) - (len(A) < len(B))
pk = {}
for key, name, ep, ver, rel, arch, href, size in db.execute(
        "select pkgKey,name,epoch,version,release,arch,location_href,size_package from packages where arch!='src'"):
    pk[key] = dict(key=key, name=name, epoch=int(ep or 0), ver=ver, rel=rel, arch=arch, href=href, size=size)
ARCH = {'pentium4': 3, 'i686': 2, 'i386': 1, 'noarch': 1}
def better(a, b):
    c = (a['epoch'] > b['epoch']) - (a['epoch'] < b['epoch']) or vc(a['ver'], b['ver']) or vc(a['rel'], b['rel'])
    if c: return c
    return ARCH.get(a['arch'], 0) - ARCH.get(b['arch'], 0)
byname = collections.defaultdict(list)
for p in pk.values():
    if p['arch'] in ARCH: byname[p['name']].append(p)
for n in byname: byname[n].sort(key=functools.cmp_to_key(better), reverse=True)
prov = collections.defaultdict(list)
for name, key in db.execute("select name,pkgKey from provides"):
    if key in pk: prov[name].append(key)
fileprov = collections.defaultdict(list)
for name, key in db.execute("select name,pkgKey from files"):
    if key in pk: fileprov[name].append(key)
def newest(keys):
    ps = [pk[k] for k in keys if pk[k]['arch'] in ARCH]
    best = [p for p in ps if byname[p['name']][0]['key'] == p['key']] or ps
    best.sort(key=functools.cmp_to_key(better), reverse=True)
    return best[0] if best else None
done = {}; todo = [byname[r][0] for r in roots]; missing = []; why = {}
while todo:
    p = todo.pop()
    if p['name'] in done: continue
    done[p['name']] = p
    for (name,) in db.execute("select name from requires where pkgKey=?", (p['key'],)):
        if name.startswith('rpmlib('): continue
        ks = prov.get(name, []) + fileprov.get(name, [])
        if any(pk[k]['name'] in done for k in ks): continue
        q = newest(ks)
        if q is None:
            missing.append((p['name'], name)); continue
        why.setdefault(q['name'], (p['name'], name))
        todo.append(q)
tot = 0
for n in sorted(done):
    p = done[n]; tot += p['size']
    print(p['href'], *why.get(n, ('-', '-')))
print('#total', tot / 1e6, 'MB', len(done), file=sys.stderr)
for m in missing: print('#missing', m, file=sys.stderr)
