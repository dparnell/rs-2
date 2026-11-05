#!/usr/bin/env python3
"""Every LX module under ROOT, what it imports that is not in ROOT, and which
packages provide the missing module names (read as data only).

usage: python3 -I lximports.py ROOT primary.sqlite
"""
import os, struct, sys, sqlite3, collections
root, dbp = sys.argv[1], sys.argv[2]
SYS = {'DOSCALLS','DOSCALL1','MSG','NLS','PMWIN','PMGPI','PMSHAPI','PMWP','SOCKETS','SO32DLL','TCP32DLL','TCPIP32','TCPIPDLL','UCONV','LIBUNI','VIOCALLS','KBDCALLS','MOUCALLS','SESMGR','QUECALLS','PMSPL','PMCTLS','PMDRAG','HELPMGR','MMPM','REXX','REXXAPI','PMVIOP','PMMERGE', 'MDM','MMIO','SW','SOM','PMCLIB','PMBIDI','PMDDE','PMSDMRI', 'MDM', 'BDCALLS','DIVE','BVSCALLS'}
have = set(); imps = collections.defaultdict(set)
for d, _, fs in os.walk(root):
    for f in fs:
        p = os.path.join(d, f)
        if os.path.islink(p): continue
        if f.lower().endswith('.dll') or f.lower().endswith('.pyd'): have.add(f.rsplit('.', 1)[0].upper())
        try:
            b = open(p, 'rb').read(0x10000)
            if b[:2] == b'MZ': o = struct.unpack_from('<I', b, 0x3c)[0]
            else: o = 0
            if b[o:o+2] != b'LX': continue
            b = open(p, 'rb').read()
            t, n = struct.unpack_from('<II', b, o+0x70)
            q = o + t
            for _ in range(n):
                l = b[q]; imps[b[q+1:q+1+l].decode('latin1').upper()].add(os.path.relpath(p, root)); q += l+1
        except Exception as e:
            print('err', p, e)
missing = {k: v for k, v in imps.items() if k not in have and k not in SYS}
db = sqlite3.connect(dbp)
for k in sorted(missing):
    prov = db.execute("select distinct p.name,p.version,p.release,p.arch from provides r join packages p using(pkgKey) where lower(r.name)=? order by p.time_build desc limit 3", (k.lower()+'.dll',)).fetchall()
    print(k, sorted(missing[k])[:3], len(missing[k]), prov)
