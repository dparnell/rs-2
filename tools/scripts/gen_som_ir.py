"""Generate the text descriptions of the interface repositories rs2 ships
(xtask turns each into a binary .IR file in \\OS2\\ETC: xtask/src/somir.rs;
the format of both is in docs/formats/som-ir.md), read from IDL the way
SOM's compiler reads it (__SOMIDL__ defined):

- userland/somir/som-ir.txt (SOM.IR): SOM.DLL's SOMObject, SOMClass and
  SOMClassMgr, SOMU.DLL's, SOMIR.DLL's and SOMD.DLL's classes and the
  types they use, from the OS/2 Toolkit's IDL (os2tk45/som/include);
- userland/somir/wpsh-ir.txt (WPSH.IR): the Workplace Shell's classes
  (os2tk45/idl) and their types;
- userland/somir/wpdserv-ir.txt (WPDSERV.IR): the Workplace Shell's DSOM
  server class WPDServer, the classes it is built on and the socket
  classes;
- userland/somir/rexx-ir.txt (REXX.IR): Object REXX's SOM classes (the DLF
  classes of REXXSOM.DLL), from rs2's own IDL in userland/somir/idl.

usage: python3 -I tools/scripts/gen_som_ir.py REPO [TARGET...]
(targets: som wpsh wpdserv rexx; all of them when none is named)
"""
import os, re, sys

repo = sys.argv[1]
TK = os.path.join(repo, 'work/media/boot/toolkit/os2tk45')
# Where #include looks: rs2's own IDL first (classes the Toolkit does not
# describe), then the Toolkit's.
PATHS = [os.path.join(repo, 'userland/somir/idl'), os.path.join(TK, 'idl'), os.path.join(TK, 'som/include')]

# What each repository holds. `files`: the IDL files compiled, in the
# order their definitions go into the repository (what they include comes
# first). A top-level definition goes in when the file it comes from is in
# `types` (types, constants, modules) or `ifaces` (interfaces); what the
# files SOMIR lists before it hold (SOM.IR's classes) is left out, as SOM's
# compiler leaves it out when it updates a repository.
SOM_FILES = [
    # SOM.DLL
    'somobj.idl', 'somcls.idl', 'somcm.idl',
    # SOMU.DLL
    'snglicls.idl', 'sombacls.idl', 'somtrcls.idl', 'somida.idl', 'somsid.idl',
    # SOMIR.DLL
    'containd.idl', 'containr.idl', 'attribdf.idl', 'constdef.idl', 'excptdef.idl',
    'intfacdf.idl', 'moduledf.idl', 'operatdf.idl', 'paramdef.idl', 'typedef.idl', 'repostry.idl',
    # SOMD.DLL
    'stexcep.idl', 'somdtype.idl', 'somdobj.idl', 'somdcprx.idl', 'om.idl', 'somdom.idl',
    'somdserv.idl', 'servmgr.idl', 'impldef.idl', 'implrep.idl', 'nvlist.idl', 'request.idl',
    'cntxt.idl', 'principl.idl', 'orb.idl', 'boa.idl', 'somoa.idl',
]
# The Workplace Shell classes of PMWP.DLL that IBM's WPSH.IR describes, in
# its order (wpimage.idl and wpminwin.idl are rs2's: the Toolkit includes
# the first but does not have it).
WPSH_FILES = [
    'wpobject.idl', 'wpfsys.idl', 'wpfolder.idl', 'wpdataf.idl', 'wpabs.idl', 'wppgmf.idl',
    'wppgm.idl', 'wptrans.idl', 'wpshadow.idl', 'wptemps.idl', 'wpstart.idl',
    'wpdrives.idl', 'wpminwin.idl', 'wpcmdf.idl', 'wpdisk.idl', 'wpclsmgr.idl', 'wpdesk.idl',
    'wpicon.idl', 'wpmet.idl', 'wpmwv.idl', 'wpnetgrp.idl', 'wpnetlnk.idl', 'wpnetwrk.idl',
    'wpimage.idl', 'wpbitmap.idl', 'wppif.idl', 'wpptr.idl', 'wprootf.idl', 'wpserver.idl',
    'wpshdir.idl', 'wpshred.idl', 'wplnchpd.idl',
]
TARGETS = {
    'som': dict(dst='som-ir.txt', ir='SOM.IR', files=SOM_FILES, types=None, ifaces=None),
    'wpsh': dict(dst='wpsh-ir.txt', ir='WPSH.IR', files=WPSH_FILES,
                 types={'wptypes.idl', 'wplnchpd.idl'}, ifaces=set(WPSH_FILES)),
    'wpdserv': dict(dst='wpdserv-ir.txt', ir='WPDSERV.IR',
                    files=['somobj.idl', 'containd.idl', 'somdtype.idl', 'somdserv.idl', 'wpdserv.idl',
                           'stexcep.idl', 'somssock.idl', 'tcpsock.idl'],
                    types={'somobj.idl', 'containd.idl', 'somdtype.idl', 'stexcep.idl'},
                    ifaces={'somobj.idl', 'somdserv.idl', 'wpdserv.idl', 'somssock.idl', 'tcpsock.idl'}),
    'rexx': dict(dst='rexx-ir.txt', ir='REXX.IR', files=['somobj.idl', 'dlf.idl'],
                 types={'somobj.idl', 'dlf.idl'}, ifaces={'somobj.idl', 'dlf.idl'}),
}

# ---- Preprocessing ----

def strip_comments(s):
    out, i, n = [], 0, len(s)
    while i < n:
        if s.startswith('//', i):
            j = s.find('\n', i)
            i = n if j < 0 else j
        elif s.startswith('/*', i):
            j = s.find('*/', i + 2)
            seg = s[i:(n if j < 0 else j + 2)]
            out.append('\n' * seg.count('\n'))
            i = n if j < 0 else j + 2
        elif s[i] == '"':
            j = i + 1
            while j < n and s[j] != '"':
                j += 2 if s[j] == '\\' else 1
            out.append(s[i:j + 1])
            i = j + 1
        else:
            out.append(s[i])
            i += 1
    return ''.join(out)

macros = {}
done_files = set()

def reset():
    macros.clear()
    macros['__SOMIDL__'] = '1'
    done_files.clear()

def cond_value(expr):
    expr = re.sub(r'defined\s*\(\s*(\w+)\s*\)', lambda m: '1' if m.group(1) in macros else '0', expr)
    expr = re.sub(r'defined\s+(\w+)', lambda m: '1' if m.group(1) in macros else '0', expr)
    expr = re.sub(r'\b[A-Za-z_]\w*\b', lambda m: macros.get(m.group(0), '0') or '0', expr)
    expr = expr.replace('&&', ' and ').replace('||', ' or ').replace('!', ' not ')
    try:
        return bool(eval(expr, {}))
    except Exception:
        return False

def preprocess(name, toks):
    """Read an IDL file (and what it includes) into tokens (with the file
    and line each comes from)."""
    if name in done_files:
        return
    path = next((os.path.join(d, name) for d in PATHS if os.path.exists(os.path.join(d, name))), None)
    if path is None:
        print('missing', name, file=sys.stderr)
        return
    done_files.add(name)
    src = strip_comments(open(path, encoding='latin1').read().replace('\r', ''))
    lines = src.split('\n')
    stack = []  # (active, taken)
    i = 0
    while i < len(lines):
        line = lines[i]
        lineno = i + 1
        while line.rstrip().endswith('\\') and i + 1 < len(lines):
            i += 1
            line = line.rstrip()[:-1] + ' ' + lines[i]
        i += 1
        t = line.strip()
        active = all(a for a, _ in stack)
        if t.startswith('#'):
            d = t[1:].strip()
            word = re.match(r'\w*', d).group(0)
            rest = d[len(word):].strip()
            if word == 'ifdef':
                on = rest.split()[0] in macros
                stack.append((on, on))
            elif word == 'ifndef':
                on = rest.split()[0] not in macros
                stack.append((on, on))
            elif word == 'if':
                on = cond_value(rest)
                stack.append((on, on))
            elif word == 'elif':
                a, taken = stack[-1]
                on = not taken and cond_value(rest)
                stack[-1] = (on, taken or on)
            elif word == 'else':
                a, taken = stack[-1]
                stack[-1] = (not taken, True)
            elif word == 'endif':
                stack.pop()
            elif not active:
                pass
            elif word == 'define':
                m = re.match(r'(\w+)(.*)', rest)
                macros[m.group(1)] = m.group(2).strip()
            elif word == 'undef':
                macros.pop(rest.split()[0], None)
            elif word == 'include':
                m = re.match(r'[<"]([^>"]+)[>"]', rest)
                if m:
                    preprocess(m.group(1).lower(), toks)
            elif word == 'pragma':
                m = re.match(r'modifier\s+([\w:]+)\s*:(.*?);?\s*$', rest)
                if m:
                    # Applied where it is (a foreign type is one from then on).
                    toks.append((('PRAGMA', m.group(1), parse_mod_list(m.group(2))), name, lineno))
            continue
        if active:
            for tok in tokenize(line):
                toks.append((tok, name, lineno))

TOKEN = re.compile(r'\s*(?:("(?:[^"\\]|\\.)*")|(::)|([A-Za-z_]\w*)|(0[xX][0-9a-fA-F]+|\d+\.\d*(?:[eE][-+]?\d+)?|\d+)|(\S))')

def tokenize(line, depth=0):
    out = []
    for m in TOKEN.finditer(line):
        t = m.group(1) or m.group(2) or m.group(3) or m.group(4) or m.group(5)
        if t is None:
            continue
        if m.group(3) and t in macros and macros[t] and depth < 8 and t != '__SOMIDL__':
            out.extend(tokenize(macros[t], depth + 1))
        else:
            out.append(t)
    return out

def parse_mod_list(text):
    """`a = b, c, d = "e"`: modifiers (value None when there is none)."""
    mods = []
    for part in split_top(text, ','):
        part = part.strip()
        if not part:
            continue
        if '=' in part:
            k, v = part.split('=', 1)
            mods.append((k.strip(), unquote(v.strip())))
        else:
            mods.append((part, None))
    return mods

def split_top(text, sep):
    out, cur, q = [], '', False
    for c in text:
        if c == '"':
            q = not q
        if c == sep and not q:
            out.append(cur)
            cur = ''
        else:
            cur += c
    out.append(cur)
    return out

def unquote(v):
    if len(v) >= 2 and v[0] == '"' and v[-1] == '"':
        return v[1:-1]
    return v

# ---- Parsing ----

BASIC = {'short': 'short', 'long': 'long', 'float': 'float', 'double': 'double', 'char': 'char',
         'boolean': 'boolean', 'octet': 'octet', 'any': 'any', 'void': 'void', 'TypeCode': 'TypeCode',
         'Principal': 'Principal', 'Object': 'objref(::SOMObject)'}

class Def:
    def __init__(self, kind, name, scope, file, line):
        self.kind, self.name, self.scope, self.file, self.line = kind, name, scope, file, line
        self.mods = []
        self.children = []
        self.tc = None
        self.extra = {}

    def id(self):
        return '::'.join(self.scope + [self.name])

class Parser:
    def __init__(self, toks):
        self.consts = {}
        self.t = toks
        self.i = 0
        self.types = {}      # scoped name -> TypeCode text (resolved)
        self.ifaces = set()  # scoped names of interfaces
        self.top = []
        self.by_id = {}

    def peek(self, k=0):
        return self.t[self.i + k][0] if self.i + k < len(self.t) else None

    def where(self):
        return self.t[self.i][1:] if self.i < len(self.t) else ('?', 0)

    def next(self):
        tok = self.t[self.i][0]
        self.i += 1
        return tok

    def expect(self, s):
        tok = self.next()
        if tok != s:
            raise SyntaxError('expected %r, got %r at %s' % (s, tok, self.where()))

    def scoped_name(self):
        parts = []
        if self.peek() == '::':
            self.next()
            parts.append('')
        parts.append(self.next())
        while self.peek() == '::':
            self.next()
            parts.append(self.next())
        return parts

    def resolve(self, parts, scope):
        """A scoped name to its full name (searching the enclosing scopes)."""
        if parts[0] == '':
            return '::'.join(parts[1:])
        name = '::'.join(parts)
        for k in range(len(scope), -1, -1):
            cand = '::'.join(scope[:k] + [name])
            if cand in self.types or cand in self.ifaces:
                return cand
        return name

    def type_spec(self, scope):
        t = self.peek()
        if t == 'unsigned':
            self.next()
            b = self.next()
            if b == 'long' and self.peek() == 'long':
                self.next()
            return 'u' + b
        if t == 'long' and self.peek(1) == 'long':
            self.next(); self.next()
            return 'long'
        if t == 'long' and self.peek(1) == 'double':
            self.next(); self.next()
            return 'double'
        if t in ('struct', 'union', 'enum'):
            return self.constructed(scope)
        if t == 'string':
            self.next()
            n = 0
            if self.peek() == '<':
                self.next()
                n = self.const_expr(scope)
                self.expect('>')
            return 'string(%d)' % n
        if t == 'sequence':
            self.next()
            self.expect('<')
            el = self.full_type(scope)
            n = 0
            if self.peek() == ',':
                self.next()
                n = self.const_expr(scope)
            self.expect('>')
            return 'sequence(%s,%d)' % (el, n)
        if t in BASIC and self.peek(1) != '::':
            self.next()
            return BASIC[t]
        parts = self.scoped_name()
        full = self.resolve(parts, scope)
        if full in self.ifaces:
            return 'objref(::%s)' % full
        if full in self.types:
            return self.types[full]
        if full == 'SOMFOREIGN':
            return 'pointer(void)'
        print('unknown type', full, 'at', self.where(), file=sys.stderr)
        return 'pointer(void)'

    def full_type(self, scope):
        t = self.type_spec(scope)
        while self.peek() == '*':
            self.next()
            t = 'pointer(%s)' % t
        return t

    def declarator(self, base, scope):
        """`*name[10][2]`: the name and its type."""
        t = base
        while self.peek() == '*':
            self.next()
            t = 'pointer(%s)' % t
        name = self.next()
        dims = []
        while self.peek() == '[':
            self.next()
            dims.append(self.const_expr(scope))
            self.expect(']')
        for d in reversed(dims):
            t = 'array(%s,%d)' % (t, d)
        return name, t

    def const_expr(self, scope):
        """A constant expression up to `>`, `]`, `,`, `;` (as a number)."""
        toks = []
        depth = 0
        while True:
            p = self.peek()
            if depth == 0 and p in ('>', ']', ',', ';'):
                break
            if p == '(':
                depth += 1
            if p == ')':
                if depth == 0:
                    break
                depth -= 1
            toks.append(self.next())
        expr = ' '.join(toks)
        expr = re.sub(r'\b[A-Za-z_]\w*\b', lambda m: str(self.consts.get(m.group(0), 0)), expr)
        try:
            v = eval(expr, {})
            return int(v) if isinstance(v, (int, float)) and float(v).is_integer() else v
        except Exception:
            return 0

    def constructed(self, scope):
        kind = self.next()
        name = self.next()
        full = '::'.join(scope + [name])
        if kind == 'enum':
            self.expect('{')
            names = []
            while self.peek() != '}':
                names.append(self.next())
                if self.peek() == ',':
                    self.next()
            self.expect('}')
            for k, n in enumerate(names):
                self.consts[n] = k
            tc = 'enum(%s,%s)' % (name, ','.join(names))
            self.types[full] = tc
            return tc
        if kind == 'struct':
            if self.peek() != '{':
                return self.types.get(full, 'self(%s)' % name)
            # Members may refer to the struct itself (through a pointer).
            self.types[full] = 'self(%s)' % name
            self.expect('{')
            members = self.members(scope)
            self.expect('}')
            tc = 'struct(%s%s)' % (name, ''.join(',%s,%s' % m for m in members))
            self.types[full] = tc
            return tc
        # union NAME switch (type) { case x: type m; default: type m; }
        self.types[full] = 'self(%s)' % name
        self.expect('switch')
        self.expect('(')
        sw = self.full_type(scope)
        self.expect(')')
        self.expect('{')
        arms = []
        while self.peek() != '}':
            labels = []
            while self.peek() in ('case', 'default'):
                if self.next() == 'case':
                    toks = []
                    while self.peek() != ':':
                        toks.append(self.next())
                    self.next()
                    lab = ' '.join(toks)
                    labels.append((1, str(self.consts.get(lab, lab))))
                else:
                    self.expect(':')
                    labels.append((2, None))
            mt = self.full_type(scope)
            mname, mt = self.declarator(mt, scope)
            self.expect(';')
            for flag, lab in labels:
                arms.append((flag, lab, mname, mt))
        self.expect('}')
        tc = 'union(%s,%s%s)' % (name, sw, ''.join(',%d,%s,%s,%s' % (f, l if l is not None else '-', n, t) for f, l, n, t in arms))
        self.types[full] = tc
        return tc

    def members(self, scope):
        out = []
        while self.peek() != '}':
            base = self.full_type(scope)
            while True:
                n, t = self.declarator(base, scope)
                out.append((n, t))
                if self.peek() == ',':
                    self.next()
                    continue
                break
            self.expect(';')
        return out

    # -- definitions --

    def definitions(self, scope, out, end=None):
        while self.i < len(self.t) and self.peek() != end:
            self.definition(scope, out)

    def new(self, kind, name, scope, out, file, line):
        d = Def(kind, name, scope, file, line)
        out.append(d)
        self.by_id[d.id()] = d
        return d

    def pragma(self, scope):
        """`#pragma modifier name: mods`: modifiers for a definition; a
        typedef of SOMFOREIGN with an impctx is a foreign type."""
        _, name, mods = self.next()
        full = name
        if full not in self.by_id:
            for k in range(len(scope), -1, -1):
                cand = '::'.join(scope[:k] + [name])
                if cand in self.by_id:
                    full = cand
                    break
        d = self.by_id.get(full)
        if d is None:
            return
        d.mods.extend(mods)
        if d.kind == 'typedef' and d.tc == 'pointer(void)':
            ctx = dict(mods).get('impctx')
            if ctx is not None:
                length = dict(mods).get('length') or '4'
                d.tc = 'foreign("%s","%s",%s)' % (d.name, ctx, length)
                self.types[d.id()] = d.tc

    def definition(self, scope, out):
        t = self.peek()
        file, line = self.where()
        if isinstance(t, tuple):
            self.pragma(scope)
            return
        if t == ';':
            self.next()
            return
        if t == 'module':
            self.next()
            name = self.next()
            d = self.by_id.get('::'.join(scope + [name]))
            if d is None:
                d = self.new('module', name, scope, out, file, line)
            self.expect('{')
            self.definitions(scope + [name], d.children, '}')
            self.expect('}')
            return
        if t == 'interface':
            self.interface(scope, out)
            return
        if t == 'typedef':
            self.next()
            base = self.full_type(scope)
            while True:
                n, tc = self.declarator(base, scope)
                full = '::'.join(scope + [n])
                if base == 'pointer(void)' and self.t[self.i - 1][0] == n and tc == 'pointer(void)':
                    pass
                d = self.new('typedef', n, scope, out, file, line)
                d.tc = tc
                self.types[full] = tc
                if self.peek() == ',':
                    self.next()
                    continue
                break
            self.expect(';')
            return
        if t in ('struct', 'union', 'enum'):
            name = self.peek(1)
            tc = self.constructed(scope)
            d = self.new('typedef', name, scope, out, file, line)
            d.tc = tc
            self.expect(';')
            return
        if t == 'const':
            self.next()
            tc = self.full_type(scope)
            name = self.next()
            self.expect('=')
            toks = []
            while self.peek() != ';':
                toks.append(self.next())
            self.next()
            text = ' '.join(toks)
            if len(toks) == 1 and toks[0].startswith('"'):
                value = toks[0]  # kept as written (quoted), as IBM's are
            else:
                e = re.sub(r'\b[A-Za-z_]\w*\b', lambda m: str(self.consts.get(m.group(0), 0)), text)
                try:
                    value = eval(e, {})
                except Exception:
                    value = text
                if isinstance(value, float) and value.is_integer() and 'float' not in tc and 'double' not in tc:
                    value = int(value)
                self.consts[name] = value
            d = self.new('const', name, scope, out, file, line)
            d.tc = tc
            d.extra['value'] = str(value)
            return
        if t == 'exception':
            self.next()
            name = self.next()
            self.expect('{')
            members = self.members(scope)
            self.expect('}')
            self.expect(';')
            d = self.new('exception', name, scope, out, file, line)
            d.tc = 'struct(%s%s)' % (name, ''.join(',%s,%s' % m for m in members))
            self.types['::'.join(scope + [name])] = d.tc
            return
        raise SyntaxError('unexpected %r at %s' % (t, (file, line)))

    def interface(self, scope, out):
        self.next()
        file, line = self.where()
        name = self.next()
        full = '::'.join(scope + [name])
        self.ifaces.add(full)
        if self.peek() == ';':
            self.next()
            return
        bases = []
        if self.peek() == ':':
            self.next()
            while True:
                parts = self.scoped_name()
                bases.append('::' + self.resolve(parts, scope))
                if self.peek() == ',':
                    self.next()
                    continue
                break
        d = self.new('interface', name, scope, out, file, line)
        d.extra['bases'] = bases
        self.expect('{')
        inner = scope + [name]
        data = []
        while self.peek() != '}':
            t = self.peek()
            fl = self.where()
            if isinstance(t, tuple):
                self.pragma(inner)
            elif t == 'implementation':
                self.next()
                self.expect('{')
                self.implementation(d, inner, data)
                self.expect('}')
                self.expect(';')
            elif t in ('attribute', 'readonly'):
                ro = False
                if self.next() == 'readonly':
                    ro = True
                    self.expect('attribute')
                base = self.full_type(inner)
                while True:
                    n, tc = self.declarator(base, inner)
                    a = self.new('attribute', n, inner, d.children, *fl)
                    a.tc = tc
                    a.extra['mode'] = 2 if ro else 1
                    if self.peek() == ',':
                        self.next()
                        continue
                    break
                self.expect(';')
            elif t in ('typedef', 'struct', 'union', 'enum', 'const', 'exception', ';'):
                self.definition(inner, d.children)
            else:
                self.operation(d, inner, fl)
        self.expect('}')
        self.expect(';')
        d.extra['data'] = data

    def operation(self, iface, scope, fl):
        mode = 1
        if self.peek() == 'oneway':
            self.next()
            mode = 2
        result = self.full_type(scope)
        name = self.next()
        op = self.new('operation', name, scope, iface.children, *fl)
        op.tc = result
        op.extra['mode'] = mode
        self.expect('(')
        while self.peek() != ')':
            pfl = self.where()
            pm = {'in': 1, 'out': 2, 'inout': 3}.get(self.peek())
            if pm:
                self.next()
            else:
                pm = 1
            pt = self.full_type(scope)
            if self.peek() in (',', ')'):
                pn = 'arg%d' % len(op.children)  # (void)-style
            else:
                pn, pt = self.declarator(pt, scope)
            if pt == 'void' and pn.startswith('arg'):
                pass
            else:
                p = self.new('parameter', pn, scope + [name], op.children, *pfl)
                p.tc = pt
                p.extra['mode'] = pm
            if self.peek() == ',':
                self.next()
        self.expect(')')
        raises, contexts = [], []
        while self.peek() in ('raises', 'context'):
            w = self.next()
            self.expect('(')
            while self.peek() != ')':
                if w == 'raises':
                    parts = self.scoped_name()
                    raises.append('::' + self.resolve(parts, scope))
                else:
                    contexts.append(unquote(self.next()))
                if self.peek() == ',':
                    self.next()
            self.expect(')')
        self.expect(';')
        op.extra['raises'] = raises
        op.extra['contexts'] = contexts

    def implementation(self, iface, scope, data):
        own = {c.name: c for c in iface.children}
        agg = {}
        while self.peek() != '}':
            t = self.peek()
            if isinstance(t, tuple):
                self.pragma(scope)
                continue
            if t == ';':
                self.next()
                continue
            if t == 'passthru':
                while self.next() != ';':
                    pass
                continue
            # `name: mods;`, `name = value;`, `releaseorder: a, b;` or data
            if self.peek(1) == ':' and self.peek(2) != ':':
                name = self.next()
                self.next()
                toks = []
                while self.peek() != ';':
                    toks.append(self.next())
                self.next()
                text = ' '.join(toks)
                if name == 'releaseorder':
                    iface.mods.append(('releaseorder', ','.join(x for x in toks if x != ',')))
                    continue
                mods = parse_mod_list(text)
                if name in own:
                    own[name].mods.extend(mods)
                else:
                    for k, v in mods:
                        if v is None:
                            agg.setdefault(k, []).append(name)
                        else:
                            agg.setdefault(k, []).append(name)
                continue
            if self.peek(1) == ';':
                # `local;`: a modifier without a value
                iface.mods.append((self.next(), None))
                self.next()
                continue
            if self.peek(1) == '=':
                name = self.next()
                self.next()
                toks = []
                while self.peek() != ';':
                    toks.append(self.next())
                self.next()
                iface.mods.append((name, unquote(' '.join(toks))))
                continue
            # instance data
            base = self.full_type(scope)
            while True:
                n, tc = self.declarator(base, scope)
                data.append((n, tc))
                if self.peek() == ',':
                    self.next()
                    continue
                break
            self.expect(';')
        for k, names in agg.items():
            iface.mods.append((k, ','.join(names)))

# ---- Output ----

def esc(s):
    return s.replace('\\', '\\\\').replace('\t', '\\t').replace('\n', '\\n')

def as_ir(iface):
    """An interface's contents as SOM's compiler puts them in the
    repository: an attribute's accessors (_get_ and, unless it is readonly,
    _set_, with its modifiers and an `attribute` one) first, then the
    operations, the other definitions and the AttributeDefs; the instance
    data is the attributes' (those without `nodata`), then the
    implementation section's."""
    attrs = [c for c in iface.children if c.kind == 'attribute']
    ops = [c for c in iface.children if c.kind == 'operation']
    rest = [c for c in iface.children if c.kind not in ('attribute', 'operation')]
    acc = []
    for a in attrs:
        g = Def('operation', '_get_' + a.name, a.scope, a.file, a.line)
        g.tc = a.tc
        g.extra = {'mode': 1, 'raises': [], 'contexts': []}
        g.mods = [('attribute', None)] + a.mods
        acc.append(g)
        if a.extra['mode'] != 2:
            s = Def('operation', '_set_' + a.name, a.scope, a.file, a.line)
            s.tc = 'void'
            s.extra = {'mode': 1, 'raises': [], 'contexts': []}
            s.mods = [('attribute', None)] + a.mods
            p = Def('parameter', a.name, a.scope + [s.name], a.file, a.line)
            p.tc = a.tc
            p.extra = {'mode': 1}
            s.children = [p]
            acc.append(s)
    iface.children = acc + ops + rest + attrs
    data = [(a.name, a.tc) for a in attrs if 'nodata' not in dict(a.mods)]
    iface.extra['data'] = data + (iface.extra.get('data') or [])

def emit(d, out, depth):
    if d.kind == 'interface':
        as_ir(d)
    mods = [('file', d.file), ('line', str(d.line))] if d.kind not in ('parameter',) else [('line', str(d.line))]
    if d.kind == 'module':
        out.append('module\t%s' % d.name)
    elif d.kind == 'interface':
        data = d.extra.get('data') or []
        dtc = 'struct(%sData%s)' % (d.name, ''.join(',%s,%s' % m for m in data)) if data else 'null'
        out.append('interface\t%s\t%s\t%s' % (d.name, ','.join(d.extra['bases']), dtc))
    elif d.kind == 'operation':
        out.append('operation\t%s\t%d\t%s\t%s\t%s' % (d.name, d.extra['mode'], d.tc, ','.join(d.extra['contexts']), ','.join(d.extra['raises'])))
        mods = [('line', str(d.line))]
    elif d.kind == 'attribute':
        out.append('attribute\t%s\t%d\t%s' % (d.name, d.extra['mode'], d.tc))
        mods = [('line', str(d.line))]
    elif d.kind == 'parameter':
        out.append('parameter\t%s\t%d\t%s' % (d.name, d.extra['mode'], d.tc))
    elif d.kind == 'typedef':
        out.append('typedef\t%s\t%s' % (d.name, d.tc))
    elif d.kind == 'const':
        out.append('const\t%s\t%s\t%s' % (d.name, d.tc, esc(d.extra['value'])))
    elif d.kind == 'exception':
        out.append('exception\t%s\t%s' % (d.name, d.tc))
    for k, v in mods + d.mods:
        out.append('mod\t%s\t%s' % (k, '\\0' if v is None else esc(v)))
    if d.kind in ('module', 'interface', 'operation'):
        for c in d.children:
            emit(c, out, depth + 1)
        out.append('end')

def keep(d, t):
    """Whether a top-level definition goes into target t's repository."""
    if d.kind == 'interface':
        return t['ifaces'] is None or d.file in t['ifaces']
    return t['types'] is None or d.file in t['types']

def generate(name):
    t = TARGETS[name]
    reset()
    toks = []
    for f in t['files']:
        preprocess(f, toks)
    p = Parser(toks)
    p.definitions([], p.top)
    out = ['# %s for rs2: generated by tools/scripts/gen_som_ir.py from IDL;' % t['ir'],
           '# xtask writes \\OS2\\ETC\\%s from it.' % t['ir']]
    n = 0
    for d in p.top:
        if keep(d, t):
            emit(d, out, 0)
            n += 1
    dst = os.path.join(repo, 'userland/somir', t['dst'])
    open(dst, 'w').write('\n'.join(out) + '\n')
    print('%s: %d definitions at the top level, %d lines' % (t['ir'], n, len(out)))

for name in sys.argv[2:] or list(TARGETS):
    generate(name)
