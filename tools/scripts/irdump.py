"""Dump a SOM interface repository file (SOM.IR, WPSH.IR, ...) as indented
text: every definition with its record offset (IID), class-specific data
and modifiers. The format is in docs/formats/som-ir.md.

usage: python3 -I tools/scripts/irdump.py FILE.IR
"""
import struct, sys
d = open(sys.argv[1], 'rb').read()
def i(o): return struct.unpack('>i', d[o:o + 4])[0]
def s(o):
    n = i(o)
    if n == -1: return None, o + 5
    return d[o + 4:o + 4 + n].decode('latin1'), o + 4 + n + 1
CODES = {0: 'ConstantDef', 1: 'TypeDef', 2: 'ExceptionDef', 3: 'InterfaceDef', 4: 'ModuleDef', 5: 'AttributeDef', 6: 'OperationDef', 7: 'ParameterDef'}
K = {1:'null',2:'void',3:'short',4:'long',5:'ushort',6:'ulong',7:'float',8:'double',9:'boolean',10:'char',11:'octet',12:'any',13:'TypeCode',14:'Principal',15:'objref',16:'struct',17:'union',18:'enum',19:'string',20:'sequence',21:'array',101:'pointer',102:'self',103:'foreign'}
def tc(o):
    k = i(o)
    if k == -1: return 'NULL', o + 4
    al = i(o + 4); o += 16
    n = K.get(k, k)
    if k == 15 or k == 102:
        v, o = s(o); return '%s(%s)' % (n, v), o
    if k == 16 or k == 18:
        name, o = s(o); cnt = i(o); o += 4; ms = []
        for _ in range(cnt):
            mn, o = s(o)
            if k == 16:
                mt, o = tc(o); ms.append('%s %s' % (mt, mn))
            else: ms.append(mn)
        return '%s %s{%s}' % (n, name, '; '.join(ms)), o
    if k == 17:
        name, o = s(o); sw, o = tc(o); cnt = i(o); o += 4; ms = []
        for _ in range(cnt):
            flag = i(o); o += 4; lab = None
            if flag == 1: lab = i(o); o += 4
            mn, o = s(o); mt, o = tc(o); ms.append('%s:%s %s' % (lab if flag == 1 else 'default', mt, mn))
        return 'union %s switch(%s){%s}' % (name, sw, '; '.join(ms)), o
    if k == 19: return 'string<%d>' % i(o), o + 4
    if k in (20, 21):
        e, o = tc(o); return '%s<%s,%d>' % (n, e, i(o)), o + 4
    if k == 101:
        e, o = tc(o); return '%s*' % e, o
    if k == 103:
        a, o = s(o); b, o = s(o); return 'foreign(%s,%s,%d)' % (a, b, i(o)), o + 4
    return n + ('/a%d' % al if al else ''), o
def strseq(o):
    n = i(o); o += 4; v = []
    for _ in range(n):
        x, o = s(o); v.append(x)
    return v, o
def contents(iid):
    if not iid: return []
    assert i(iid + 4) == 8, hex(iid)
    n = i(iid + 8); o = iid + 12; out = []
    for _ in range(n):
        e, code = i(o), i(o + 4)
        name, o = s(o + 8)
        out.append((e, code, name))
    return out
def var(iid):
    if not iid: return None
    assert i(iid + 4) == 0xb, hex(iid)
    o = iid + 16
    n = i(o); o += 4; mods = []
    for _ in range(n):
        a, o = s(o); b, o = s(o); mods.append((a, b))
    return mods, o
def show(iid, depth):
    size, code = i(iid), i(iid + 4)
    name, o = s(iid + 8)
    defin, refs, v = i(o + 4), i(o + 8), i(o + 12)
    o += 16
    kind = CODES.get(code, code)
    vd = var(v)
    mods = vd[0] if vd else []
    vo = vd[1] if vd else None
    extra = ''
    cont = None
    if kind in ('InterfaceDef', 'ModuleDef'):
        cont = i(o)
    if kind == 'OperationDef':
        cont = i(o); extra = 'mode=%d' % i(o + 8)
        if vo:
            r, vo = tc(vo); ctx, vo = strseq(vo); raises, vo = strseq(vo)
            extra += ' result=%s contexts=%s raises=%s' % (r, ctx, raises)
    if kind in ('AttributeDef', 'ParameterDef'):
        extra = 'mode=%d' % i(o)
        if vo: t, vo = tc(vo); extra += ' type=%s' % t
    if kind in ('TypeDef', 'ExceptionDef') and vo:
        t, vo = tc(vo); extra = 'type=%s' % t
    if kind == 'ConstantDef' and vo:
        t, vo = tc(vo); val, vo = s(vo); extra = 'type=%s value=%s' % (t, val)
    if kind == 'InterfaceDef' and vo:
        b, vo = strseq(vo); t, vo = tc(vo); extra = 'bases=%s instanceData=%s' % (b, t)
    print('%s%s %s @%x %s%s' % ('  ' * depth, kind, name, iid, extra, (' mods=%s' % mods) if mods else ''))
    for e, c, n in contents(cont):
        show(e, depth + 1)
print('header', [hex(i(o)) for o in range(0, 0x20, 4)])
for e, code, name in contents(i(8)):
    show(e, 0)
