#!/usr/bin/env python3
"""Host-side SMB file server for testing rs2's LAN requester (crates/smb):
the counterpart of the guest's SMB client, standard library only.

Dialects: SMB 2.0.2 and SMB 2.1 (from an SMB2 NEGOTIATE, or from an SMB1
multi-protocol NEGOTIATE listing "SMB 2.002"/"SMB 2.???"), or with --smb1
only SMB1 "NT LM 0.12".

Transports: direct TCP (--port, default 4445, the stand-in for 445) and the
NetBIOS session service (--nbport, default 4139, the stand-in for 139:
RFC 1002 SESSION REQUEST answered for the server name or *SMBSERVER).
Both carry SMB messages in 4-byte NetBIOS session framing; keepalives are
answered. NBNS (UDP, --nbns-port, default 4137) answers NAME QUERY for the
server name (<00>, <20>) and the workgroup (<00>, <1D>, <1E>) with --ip,
and NODE STATUS with the name table and --mac.

Logons: NTLMSSP (raw or inside SPNEGO), NTLMv2/LMv2 checked against the
--user NAME:PASSWORD accounts (any domain), KEY_EXCH session keys (RC4).
--guest admits anonymous and unknown users as guests. --require-signing
makes the server demand (and produce) SMB2 HMAC-SHA256 / SMB1 MD5
signatures.

Shares: --share NAME=DIR disk shares on host directories, --printer
NAME=SPOOLDIR print queues (every file created and written there becomes a
job file jobNNNN-USER-NAME.prn in SPOOLDIR), and IPC$ with the named pipes
\\PIPE\\srvsvc (DCE/RPC NetrShareEnum, NetrShareGetInfo, NetrServerGetInfo)
and \\PIPE\\wkssvc (NetrWkstaGetInfo), and (SMB1) RAP on \\PIPE\\LANMAN
(NetShareEnum, NetServerEnum2, NetServerGetInfo, NetWkstaGetInfo).
Extended attributes and the DOS hidden/system/archive bits live in a
sidecar file .smbhost-eas.json in each directory (hidden from listings);
the read-only bit is the host file's write permission.

Logs one line per request (command, path, status) to stdout.

usage: python3 -I smbhost.py --share DATA=/tmp/smbdata --user rs2:secret
           [--printer LPT=/tmp/spool] [--guest] [--require-signing] [--smb1]
           [--max-dialect 0202|0210] [--max-io 1048576] [--name SMBHOST]
           [--workgroup WORKGROUP] [--ip 127.0.0.1] [--bind 127.0.0.1]
           [--port 4445] [--nbport 4139] [--nbns-port 4137]
"""
import argparse, collections, hashlib, hmac, json, os, re, socket, struct
import sys, threading, time, traceback, uuid, zlib


def log(*a):
    print(time.strftime('%H:%M:%S'), *a, flush=True)


def u16(b, o):
    return struct.unpack_from('<H', b, o)[0]


def u32(b, o):
    return struct.unpack_from('<I', b, o)[0]


def u64(b, o):
    return struct.unpack_from('<Q', b, o)[0]


EPOCH_DIFF = 11644473600


def filetime(t):
    return int((t + EPOCH_DIFF) * 10000000) if t else 0


def now_ft():
    return filetime(time.time())


def ft_to_ns(ft):
    return (ft - EPOCH_DIFF * 10000000) * 100


# ---------------------------------------------------------------- crypto

def md4(msg):
    def rol(v, s):
        v &= 0xffffffff
        return ((v << s) | (v >> (32 - s))) & 0xffffffff
    ml = len(msg) * 8
    msg = msg + b'\x80' + b'\0' * ((55 - len(msg)) % 64) + struct.pack('<Q', ml)
    a, b, c, d = 0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476
    r2 = (0, 4, 8, 12, 1, 5, 9, 13, 2, 6, 10, 14, 3, 7, 11, 15)
    r3 = (0, 8, 4, 12, 2, 10, 6, 14, 1, 9, 5, 13, 3, 11, 7, 15)
    for i in range(0, len(msg), 64):
        X = struct.unpack('<16I', msg[i:i + 64])
        h = [a, b, c, d]
        for k in range(16):
            x, y, z = h[1], h[2], h[3]
            h[0] = rol(h[0] + ((x & y) | (~x & z)) + X[k], (3, 7, 11, 19)[k % 4])
            h = [h[3], h[0], h[1], h[2]]
        for j, k in enumerate(r2):
            x, y, z = h[1], h[2], h[3]
            h[0] = rol(h[0] + ((x & y) | (x & z) | (y & z)) + X[k] + 0x5a827999, (3, 5, 9, 13)[j % 4])
            h = [h[3], h[0], h[1], h[2]]
        for j, k in enumerate(r3):
            x, y, z = h[1], h[2], h[3]
            h[0] = rol(h[0] + (x ^ y ^ z) + X[k] + 0x6ed9eba1, (3, 9, 11, 15)[j % 4])
            h = [h[3], h[0], h[1], h[2]]
        a = (a + h[0]) & 0xffffffff
        b = (b + h[1]) & 0xffffffff
        c = (c + h[2]) & 0xffffffff
        d = (d + h[3]) & 0xffffffff
    return struct.pack('<4I', a, b, c, d)


def rc4(key, data):
    S = list(range(256))
    j = 0
    for i in range(256):
        j = (j + S[i] + key[i % len(key)]) & 255
        S[i], S[j] = S[j], S[i]
    i = j = 0
    out = bytearray()
    for ch in data:
        i = (i + 1) & 255
        j = (j + S[i]) & 255
        S[i], S[j] = S[j], S[i]
        out.append(ch ^ S[(S[i] + S[j]) & 255])
    return bytes(out)


def hmac_md5(k, m):
    return hmac.new(k, m, hashlib.md5).digest()


# ---------------------------------------------------------------- NTSTATUS

STATUS_NAMES = {}


def _st(name, v):
    STATUS_NAMES[v] = 'STATUS_' + name
    return v


SUCCESS = _st('SUCCESS', 0)
BUFFER_OVERFLOW = _st('BUFFER_OVERFLOW', 0x80000005)
NO_MORE_FILES = _st('NO_MORE_FILES', 0x80000006)
NOT_IMPLEMENTED = _st('NOT_IMPLEMENTED', 0xC0000002)
INVALID_INFO_CLASS = _st('INVALID_INFO_CLASS', 0xC0000003)
INFO_LENGTH_MISMATCH = _st('INFO_LENGTH_MISMATCH', 0xC0000004)
INVALID_HANDLE = _st('INVALID_HANDLE', 0xC0000008)
INVALID_PARAMETER = _st('INVALID_PARAMETER', 0xC000000D)
NO_SUCH_FILE = _st('NO_SUCH_FILE', 0xC000000F)
INVALID_DEVICE_REQUEST = _st('INVALID_DEVICE_REQUEST', 0xC0000010)
END_OF_FILE = _st('END_OF_FILE', 0xC0000011)
MORE_PROCESSING_REQUIRED = _st('MORE_PROCESSING_REQUIRED', 0xC0000016)
ACCESS_DENIED = _st('ACCESS_DENIED', 0xC0000022)
BUFFER_TOO_SMALL = _st('BUFFER_TOO_SMALL', 0xC0000023)
OBJECT_NAME_INVALID = _st('OBJECT_NAME_INVALID', 0xC0000033)
OBJECT_NAME_NOT_FOUND = _st('OBJECT_NAME_NOT_FOUND', 0xC0000034)
OBJECT_NAME_COLLISION = _st('OBJECT_NAME_COLLISION', 0xC0000035)
OBJECT_PATH_NOT_FOUND = _st('OBJECT_PATH_NOT_FOUND', 0xC000003A)
SHARING_VIOLATION = _st('SHARING_VIOLATION', 0xC0000043)
NO_EAS_ON_FILE = _st('NO_EAS_ON_FILE', 0xC0000052)
DELETE_PENDING = _st('DELETE_PENDING', 0xC0000056)
LOGON_FAILURE = _st('LOGON_FAILURE', 0xC000006D)
DISK_FULL = _st('DISK_FULL', 0xC000007F)
FILE_IS_A_DIRECTORY = _st('FILE_IS_A_DIRECTORY', 0xC00000BA)
NOT_SUPPORTED = _st('NOT_SUPPORTED', 0xC00000BB)
NETWORK_NAME_DELETED = _st('NETWORK_NAME_DELETED', 0xC00000C9)
BAD_NETWORK_NAME = _st('BAD_NETWORK_NAME', 0xC00000CC)
PIPE_EMPTY = _st('PIPE_EMPTY', 0xC00000D9)
DIRECTORY_NOT_EMPTY = _st('DIRECTORY_NOT_EMPTY', 0xC0000101)
NOT_A_DIRECTORY = _st('NOT_A_DIRECTORY', 0xC0000103)
CANNOT_DELETE = _st('CANNOT_DELETE', 0xC0000121)
FILE_CLOSED = _st('FILE_CLOSED', 0xC0000128)
USER_SESSION_DELETED = _st('USER_SESSION_DELETED', 0xC0000203)
NOT_FOUND = _st('NOT_FOUND', 0xC0000225)


def stname(v):
    return STATUS_NAMES.get(v, 'STATUS_0x%08X' % v)


def is_error(st):
    return st >= 0xC0000000


# ---------------------------------------------------------------- SPNEGO / NTLMSSP

SPNEGO_OID = bytes.fromhex('2b0601050502')
NTLMSSP_OID = bytes.fromhex('2b06010401823702020a')


def der(tag, body):
    n = len(body)
    if n < 0x80:
        ln = bytes([n])
    elif n < 0x100:
        ln = b'\x81' + bytes([n])
    else:
        ln = b'\x82' + n.to_bytes(2, 'big')
    return bytes([tag]) + ln + body


def der_walk(data):
    """Yields the primitive values in a DER blob (recursing into constructed ones)."""
    off = 0
    while off + 2 <= len(data):
        tag = data[off]
        ln = data[off + 1]
        off += 2
        if ln & 0x80:
            n = ln & 0x7f
            ln = int.from_bytes(data[off:off + n], 'big')
            off += n
        val = data[off:off + ln]
        off += ln
        if tag & 0x20:
            yield from der_walk(val)
        else:
            yield tag, val


def extract_ntlm(blob):
    """(NTLMSSP message, wrapped in SPNEGO?) or (None, _)."""
    if blob.startswith(b'NTLMSSP\0'):
        return blob, False
    try:
        for tag, val in der_walk(blob):
            if val.startswith(b'NTLMSSP\0'):
                return val, True
    except (IndexError, ValueError):
        pass
    return None, True


def der_items(data):
    """[(tag, value, raw element)] of the DER elements in data."""
    off = 0
    out = []
    while off + 2 <= len(data):
        start = off
        tag, ln = data[off], data[off + 1]
        off += 2
        if ln & 0x80:
            n = ln & 0x7f
            ln = int.from_bytes(data[off:off + n], 'big')
            off += n
        out.append((tag, data[off:off + ln], data[start:off + ln]))
        off += ln
    return out


def spnego_mech_types(blob):
    """The DER MechTypeList of a NegTokenInit (what mechListMIC signs), or None."""
    try:
        top = der_items(blob)
        if not top or top[0][0] != 0x60:
            return None
        for tag, val, _ in der_items(top[0][1]):
            if tag == 0xa0:
                for t2, v2, _ in der_items(der_items(val)[0][1]):
                    if t2 == 0xa0:
                        return der_items(v2)[0][2]
    except (IndexError, ValueError):
        pass
    return None


def spnego_init():
    mech = der(0x30, der(0x06, NTLMSSP_OID))
    hints = der(0xa3, der(0x30, der(0xa0, der(0x1b, b'not_defined_in_RFC4178@please_ignore'))))
    return der(0x60, der(0x06, SPNEGO_OID) + der(0xa0, der(0x30, der(0xa0, mech) + hints)))


def spnego_resp(state, token=None, mech=True, mic=None):
    body = der(0xa0, der(0x0a, bytes([state])))
    if mech:
        body += der(0xa1, der(0x06, NTLMSSP_OID))
    if token:
        body += der(0xa2, der(0x04, token))
    if mic:
        body += der(0xa3, der(0x04, mic))
    return der(0xa1, der(0x30, body))


NTLM_UNICODE, NTLM_OEM, NTLM_REQUEST_TARGET = 0x1, 0x2, 0x4
NTLM_SIGN, NTLM_SEAL, NTLM_NTLM = 0x10, 0x20, 0x200
NTLM_ALWAYS_SIGN, NTLM_TARGET_DOMAIN = 0x8000, 0x10000
NTLM_ESS, NTLM_TARGET_INFO, NTLM_VERSION = 0x80000, 0x800000, 0x2000000
NTLM_128, NTLM_KEY_EXCH, NTLM_56 = 0x20000000, 0x40000000, 0x80000000


def ntowfv2(password, user, domain):
    return hmac_md5(md4(password.encode('utf-16-le')), (user.upper() + domain).encode('utf-16-le'))


def check_password(srv, user, domain, challenge, lm, nt):
    """Checks an NTLMv2 (or LMv2) response. -> (status, session base key, kind)
    kind: 'user', 'guest' or 'anonymous'."""
    if not user and len(nt) == 0 and len(lm) <= 1:
        if srv.guest:
            return SUCCESS, None, 'anonymous'
        log('logon: anonymous refused (no --guest)')
        return LOGON_FAILURE, None, None
    pw = srv.users.get(user.lower())
    if pw is None:
        if srv.guest:
            log('logon: unknown user %r admitted as guest' % user)
            return SUCCESS, None, 'guest'
        log('logon: unknown user %r' % user)
        return LOGON_FAILURE, None, None
    doms = []
    for d in (domain, domain.upper(), domain.lower(), '', srv.workgroup, srv.name):
        if d not in doms:
            doms.append(d)
    for d in doms:
        k = ntowfv2(pw, user, d)
        if len(nt) > 24 and hmac_md5(k, challenge + nt[16:]) == nt[:16]:
            return SUCCESS, hmac_md5(k, nt[:16]), 'user'
        if len(lm) == 24 and hmac_md5(k, challenge + lm[16:]) == lm[:16]:
            return SUCCESS, hmac_md5(k, lm[:16]), 'user'
    if len(nt) == 24:
        log('logon: %r sent an NTLMv1 response (not supported)' % user)
    else:
        log('logon: wrong password for %r' % user)
    return LOGON_FAILURE, None, None


class NtlmServer:
    def __init__(self, srv):
        self.srv = srv
        self.challenge = os.urandom(8)
        self.flags = 0
        self.mech_types = None
        self.key = None
        self.cflags = 0

    def mic(self):
        """The SPNEGO mechListMIC (an NTLMSSP signature of the client's mechTypes, sequence 0)."""
        if not (self.mech_types and self.key and self.cflags & NTLM_ESS):
            return None
        k = self.key
        sign = hashlib.md5(k + b'session key to server-to-client signing key magic constant\0').digest()
        sk = k if self.cflags & NTLM_128 else k[:7] if self.cflags & NTLM_56 else k[:5]
        seal = hashlib.md5(sk + b'session key to server-to-client sealing key magic constant\0').digest()
        chk = hmac_md5(sign, struct.pack('<I', 0) + self.mech_types)[:8]
        if self.cflags & NTLM_KEY_EXCH:
            chk = rc4(seal, chk)
        return struct.pack('<I', 1) + chk + struct.pack('<I', 0)

    def challenge_msg(self, neg):
        cf = u32(neg, 12) if len(neg) >= 16 else NTLM_UNICODE
        f = NTLM_REQUEST_TARGET | NTLM_NTLM | NTLM_TARGET_DOMAIN | NTLM_TARGET_INFO | NTLM_VERSION
        f |= NTLM_UNICODE if cf & NTLM_UNICODE else NTLM_OEM
        f |= cf & (NTLM_SIGN | NTLM_SEAL | NTLM_ALWAYS_SIGN | NTLM_ESS | NTLM_128 | NTLM_KEY_EXCH | NTLM_56)
        self.flags = f
        srv = self.srv

        def av(i, s):
            v = s if isinstance(s, bytes) else s.encode('utf-16-le')
            return struct.pack('<HH', i, len(v)) + v
        info = (av(2, srv.workgroup) + av(1, srv.name) + av(4, srv.workgroup.lower())
                + av(3, srv.name.lower()) + av(7, struct.pack('<Q', now_ft())) + av(0, b''))
        target = srv.workgroup.encode('utf-16-le' if f & NTLM_UNICODE else 'latin-1')
        hdr = 56
        return (b'NTLMSSP\0' + struct.pack('<IHHII', 2, len(target), len(target), hdr, f)
                + self.challenge + b'\0' * 8
                + struct.pack('<HHI', len(info), len(info), hdr + len(target))
                + struct.pack('<BBHBBBB', 10, 0, 19041, 0, 0, 0, 15)
                + target + info)

    def authenticate(self, msg):
        """-> (status, user, domain, session key or None, kind)"""
        def fld(o):
            ln, _, off = struct.unpack_from('<HHI', msg, o)
            return msg[off:off + ln]
        flags = u32(msg, 60)
        enc = 'utf-16-le' if flags & NTLM_UNICODE else 'latin-1'
        lm, nt = fld(12), fld(20)
        dom, user, ws = (fld(o).decode(enc, 'replace') for o in (28, 36, 44))
        enckey = fld(52)
        st, key, kind = check_password(self.srv, user, dom, self.challenge, lm, nt)
        if st == SUCCESS and key and flags & NTLM_KEY_EXCH and len(enckey) == 16:
            key = rc4(key, enckey)
        self.key, self.cflags = key, flags
        log('logon: user %r domain %r workstation %r -> %s%s' % (
            user, dom, ws, stname(st), ' (%s)' % kind if kind else ''))
        return st, user, dom, key, kind


# ---------------------------------------------------------------- file system

META = '.smbhost-eas.json'
meta_lock = threading.RLock()

ATTR_READONLY, ATTR_HIDDEN, ATTR_SYSTEM = 0x1, 0x2, 0x4
ATTR_DIRECTORY, ATTR_ARCHIVE, ATTR_NORMAL = 0x10, 0x20, 0x80


def meta_where(path, root):
    if os.path.normpath(path) == os.path.normpath(root):
        return root, '.'
    return os.path.split(path)


def meta_load(d):
    try:
        with open(os.path.join(d, META), encoding='utf-8') as f:
            m = json.load(f)
            return m if isinstance(m, dict) else {}
    except (OSError, ValueError):
        return {}


def meta_save(d, m):
    p = os.path.join(d, META)
    m = {k: v for k, v in m.items() if v}
    if not m:
        try:
            os.remove(p)
        except OSError:
            pass
        return
    tmp = p + '.tmp'
    with open(tmp, 'w', encoding='utf-8') as f:
        json.dump(m, f, indent=1, sort_keys=True)
    os.replace(tmp, p)


def meta_get(path, root):
    d, n = meta_where(path, root)
    with meta_lock:
        return meta_load(d).get(n, {})


def meta_update(path, root, fn):
    d, n = meta_where(path, root)
    with meta_lock:
        m = meta_load(d)
        e = m.get(n, {})
        fn(e)
        e = {k: v for k, v in e.items() if v not in (None, [], {})}
        if e:
            m[n] = e
        else:
            m.pop(n, None)
        meta_save(d, m)


def meta_drop(path, root):
    meta_update(path, root, lambda e: e.clear())


def meta_move(old, new, root):
    e = dict(meta_get(old, root))
    meta_drop(old, root)
    if e:
        meta_update(new, root, lambda x: x.update(e))


def get_eas(path, root):
    """[(NAME bytes, flags, value bytes)]"""
    return [(bytes.fromhex(n), f, bytes.fromhex(v)) for n, f, v in meta_get(path, root).get('eas', [])]


def set_eas(path, root, eas):
    def fn(e):
        e['eas'] = [[n.hex(), f, v.hex()] for n, f, v in eas]
    meta_update(path, root, fn)


def ea_size(eas):
    """Size of the OS/2 FEALIST holding the EAs (0 without EAs), like Samba."""
    if not eas:
        return 0
    return 4 + sum(4 + len(n) + 1 + len(v) for n, f, v in eas)


def full_ea_list(eas):
    out = bytearray()
    last = None
    for n, f, v in eas:
        if last is not None:
            out += b'\0' * ((-len(out)) % 4)
            struct.pack_into('<I', out, last, len(out) - last)
        last = len(out)
        out += struct.pack('<IBBH', 0, f, len(n), len(v)) + n + b'\0' + v
    return bytes(out)


def parse_full_ea_list(data):
    eas = []
    off = 0
    while off + 8 <= len(data):
        neo, f, nl, vl = struct.unpack_from('<IBBH', data, off)
        eas.append((data[off + 8:off + 8 + nl].upper(), f, data[off + 9 + nl:off + 9 + nl + vl]))
        if not neo:
            break
        off += neo
    return eas


class Fi:
    """What a directory entry or open file looks like to the client."""
    pass


def file_info(path, root, st=None):
    if st is None:
        st = os.stat(path)
    fi = Fi()
    fi.isdir = stat_isdir(st)
    fi.ct = filetime(getattr(st, 'st_birthtime', st.st_ctime))
    fi.at = filetime(st.st_atime)
    fi.wt = filetime(st.st_mtime)
    fi.chg = filetime(st.st_ctime)
    fi.eof = 0 if fi.isdir else st.st_size
    fi.alloc = 0 if fi.isdir else (st.st_size + 4095) // 4096 * 4096
    fi.ino = st.st_ino
    fi.nlink = 1 if fi.isdir else st.st_nlink
    m = meta_get(path, root)
    a = ATTR_DIRECTORY if fi.isdir else 0
    if not st.st_mode & 0o200:
        a |= ATTR_READONLY
    stored = m.get('attr')
    if stored is None:
        if not fi.isdir:
            a |= ATTR_ARCHIVE
        if os.path.basename(path).startswith('.') and os.path.normpath(path) != os.path.normpath(root):
            a |= ATTR_HIDDEN
    else:
        a |= stored & (ATTR_HIDDEN | ATTR_SYSTEM | ATTR_ARCHIVE)
    fi.attrs = a or ATTR_NORMAL
    fi.eas = [(bytes.fromhex(n), f, bytes.fromhex(v)) for n, f, v in m.get('eas', [])]
    fi.easize = ea_size(fi.eas)
    return fi


def synthetic_info(isdir):
    fi = Fi()
    fi.isdir = isdir
    fi.ct = fi.at = fi.wt = fi.chg = now_ft()
    fi.eof = fi.alloc = 0
    fi.ino = 0
    fi.nlink = 1
    fi.attrs = ATTR_DIRECTORY if isdir else ATTR_NORMAL
    fi.eas = []
    fi.easize = 0
    return fi


def stat_isdir(st):
    return (st.st_mode & 0o170000) == 0o040000


WILDCARDS = set('*?<>"')
BADCHARS = set('/:|\0') | set(chr(i) for i in range(1, 32))


def lookup(d, name):
    """The actual name of entry `name` in directory d (case-insensitive), or None."""
    if name.lower() == META.lower():
        return None
    try:
        names = os.listdir(d)
    except OSError:
        return None
    if name in names:
        return name
    low = name.lower()
    for n in names:
        if n.lower() == low:
            return n
    return None


def resolve(root, name):
    """-> (status, host path, exists, share-relative name '\\a\\b')"""
    parts = [p for p in name.split('\\') if p != '']
    for p in parts:
        if p in ('.', '..') or set(p) & BADCHARS:
            return OBJECT_NAME_INVALID, None, False, None
        if set(p) & WILDCARDS:
            return OBJECT_NAME_INVALID, None, False, None
    cur = root
    rel = []
    for p in parts[:-1]:
        act = lookup(cur, p)
        if act is None or not os.path.isdir(os.path.join(cur, act)):
            return OBJECT_PATH_NOT_FOUND, None, False, None
        cur = os.path.join(cur, act)
        rel.append(act)
    if not parts:
        return SUCCESS, root, True, '\\'
    act = lookup(cur, parts[-1])
    if act is None:
        rel.append(parts[-1])
        return SUCCESS, os.path.join(cur, parts[-1]), False, '\\' + '\\'.join(rel)
    rel.append(act)
    return SUCCESS, os.path.join(cur, act), True, '\\' + '\\'.join(rel)


def wild_match(pat, name):
    if pat in ('*', '*.*', '<.*', '<"*'):
        return True
    if not set(pat) & WILDCARDS:
        return pat.lower() == name.lower()
    rx = ''
    for ch in pat:
        if ch in '*<':
            rx += '.*'
        elif ch == '?':
            rx += '.'
        elif ch == '>':
            rx += '.?'
        elif ch == '"':
            rx += r'\.?'
        else:
            rx += re.escape(ch)
    return re.fullmatch(rx, name, re.I | re.S) is not None


SHORT_OK = set('ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789$%\'-_@~`!(){}^#&')


def is_83(name):
    if name in ('.', '..'):
        return True
    base, dot, ext = name.partition('.')
    if '.' in ext or not base:
        return False
    if len(base) > 8 or len(ext) > 3:
        return False
    return set((base + ext).upper()) <= SHORT_OK


def short_name(name, used):
    if is_83(name):
        return ''
    base, ext = (name.rsplit('.', 1) + [''])[:2] if '.' in name.lstrip('.') else (name, '')
    clean = lambda s: ''.join(c for c in s.upper() if c in SHORT_OK)
    b = clean(base)[:6] or 'FILE'
    e = clean(ext)[:3]
    n = 1
    while True:
        cand = '%s~%d%s' % (b[:7 - len(str(n))], n, '.' + e if e else '')
        if cand not in used:
            used.add(cand)
            return cand
        n += 1


class Share:
    def __init__(self, name, kind, path=None, remark=''):
        self.name, self.kind, self.path, self.remark = name, kind, path, remark

    @property
    def stype(self):
        return {'disk': 0, 'print': 1, 'ipc': 0x80000003}[self.kind]


class Tree:
    def __init__(self, tid, share, sess):
        self.tid, self.share, self.sess = tid, share, sess


class Open:
    def __init__(self, conn, tree, kind, path=None, rel='', fd=None, isdir=False, access=0, writable=False):
        self.conn, self.tree, self.kind = conn, tree, kind
        self.path, self.rel, self.fd, self.isdir = path, rel, fd, isdir
        self.access, self.writable = access, writable
        self.delete_pending = False
        self.search = None
        self.pipe = None
        self.job = None
        self.written = 0
        self.closed = False

    @property
    def root(self):
        return self.tree.share.path

    def info(self):
        if self.kind in ('pipe', 'printroot'):
            return synthetic_info(self.kind == 'printroot')
        if self.kind == 'print':
            st = os.fstat(self.fd)
            fi = synthetic_info(False)
            fi.eof = st.st_size
            fi.alloc = (st.st_size + 4095) // 4096 * 4096
            return fi
        st = os.fstat(self.fd) if self.fd is not None else os.stat(self.path)
        return file_info(self.path, self.root, st)

    def close(self):
        if self.closed:
            return
        self.closed = True
        if self.fd is not None:
            try:
                os.close(self.fd)
            except OSError:
                pass
        if self.kind == 'print':
            log('print: job %s for queue %s from %s: %d bytes' % (
                os.path.basename(self.job), self.tree.share.name, self.tree.sess.user or 'guest', self.written))
        if self.delete_pending and self.path:
            try:
                if self.isdir:
                    try:
                        os.remove(os.path.join(self.path, META))
                    except OSError:
                        pass
                    os.rmdir(self.path)
                else:
                    os.unlink(self.path)
                meta_drop(self.path, self.root)
                log('  deleted on close:', self.rel)
            except OSError as e:
                log('  delete on close of %s failed: %s' % (self.rel, e))


# create dispositions, options, access
SUPERSEDE, OPEN, CREATE, OPEN_IF, OVERWRITE, OVERWRITE_IF = range(6)
FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_DELETE_ON_CLOSE = 0x1, 0x40, 0x1000
WRITE_ACCESS = 0x2 | 0x4 | 0x40000000 | 0x10000000
DELETE_ACCESS = 0x10000


def os_status(e):
    import errno
    return {errno.ENOENT: OBJECT_NAME_NOT_FOUND, errno.EEXIST: OBJECT_NAME_COLLISION,
            errno.EACCES: ACCESS_DENIED, errno.EPERM: ACCESS_DENIED, errno.ENOTDIR: OBJECT_PATH_NOT_FOUND,
            errno.EISDIR: FILE_IS_A_DIRECTORY, errno.ENOTEMPTY: DIRECTORY_NOT_EMPTY,
            errno.ENOSPC: DISK_FULL, errno.EROFS: ACCESS_DENIED}.get(e.errno, ACCESS_DENIED)


def dir_empty(path):
    try:
        return not [n for n in os.listdir(path) if n != META]
    except OSError:
        return True


def fs_create(conn, tree, name, disp, opts, access, fattrs):
    """-> (status, Open, create action)"""
    share = tree.share
    name = name.lstrip('\\')
    if share.kind == 'ipc':
        pname = name.lower()
        if pname.startswith('pipe\\'):
            pname = pname[5:]
        if pname not in ('srvsvc', 'wkssvc'):
            return OBJECT_NAME_NOT_FOUND, None, 0
        op = Open(conn, tree, 'pipe', rel='\\' + pname)
        op.pipe = RpcPipe(conn.srv, pname)
        return SUCCESS, op, 1
    if share.kind == 'print':
        if not name:
            return SUCCESS, Open(conn, tree, 'printroot', rel='\\', isdir=True), 1
        srv = conn.srv
        user = re.sub(r'[^A-Za-z0-9_.-]', '_', tree.sess.user or 'guest')
        base = re.sub(r'[^A-Za-z0-9_.-]', '_', name.replace('\\', '_'))[:40]
        while True:
            with srv.lock:
                srv.jobno += 1
                n = srv.jobno
            job = os.path.join(share.path, 'job%04d-%s-%s.prn' % (n, user, base))
            try:
                fd = os.open(job, os.O_RDWR | os.O_CREAT | os.O_EXCL, 0o644)
                break
            except FileExistsError:
                continue
            except OSError as e:
                return os_status(e), None, 0
        op = Open(conn, tree, 'print', path=job, rel='\\' + name, fd=fd, writable=True, access=access)
        op.job = job
        log('print: job %d started for queue %s: %s' % (n, share.name, job))
        return SUCCESS, op, 2
    st, path, exists, rel = resolve(share.path, name)
    if st:
        return st, None, 0
    wants_write = bool(access & WRITE_ACCESS)
    if exists:
        try:
            s = os.stat(path)
        except OSError as e:
            return os_status(e), None, 0
        isdir = stat_isdir(s)
        if disp == CREATE:
            return OBJECT_NAME_COLLISION, None, 0
        if opts & FILE_DIRECTORY_FILE and not isdir:
            return NOT_A_DIRECTORY, None, 0
        if opts & FILE_NON_DIRECTORY_FILE and isdir:
            return FILE_IS_A_DIRECTORY, None, 0
        readonly = not s.st_mode & 0o200
        if isdir:
            if disp in (SUPERSEDE, OVERWRITE, OVERWRITE_IF):
                return INVALID_PARAMETER, None, 0
            op = Open(conn, tree, 'dir', path=path, rel=rel, isdir=True, access=access)
            action = 1
        else:
            truncate = disp in (SUPERSEDE, OVERWRITE, OVERWRITE_IF)
            if readonly and (wants_write or truncate):
                return ACCESS_DENIED, None, 0
            if readonly and opts & FILE_DELETE_ON_CLOSE:
                return CANNOT_DELETE, None, 0
            try:
                fd = os.open(path, os.O_RDWR if not readonly else os.O_RDONLY)
                writable = not readonly
            except PermissionError:
                if wants_write:
                    return ACCESS_DENIED, None, 0
                fd = os.open(path, os.O_RDONLY)
                writable = False
            except OSError as e:
                return os_status(e), None, 0
            if truncate:
                os.ftruncate(fd, 0)
                if fattrs & (ATTR_HIDDEN | ATTR_SYSTEM):
                    meta_update(path, share.path, lambda e: e.__setitem__(
                        'attr', (fattrs & (ATTR_HIDDEN | ATTR_SYSTEM)) | ATTR_ARCHIVE))
            op = Open(conn, tree, 'file', path=path, rel=rel, fd=fd, access=access, writable=writable)
            action = {SUPERSEDE: 0, OVERWRITE: 3, OVERWRITE_IF: 3}.get(disp, 1)
    else:
        if disp in (OPEN, OVERWRITE):
            return OBJECT_NAME_NOT_FOUND, None, 0
        if os.path.basename(path).lower() == META.lower():
            return ACCESS_DENIED, None, 0
        try:
            if opts & FILE_DIRECTORY_FILE:
                os.mkdir(path)
                op = Open(conn, tree, 'dir', path=path, rel=rel, isdir=True, access=access)
            else:
                fd = os.open(path, os.O_RDWR | os.O_CREAT | os.O_EXCL, 0o666)
                op = Open(conn, tree, 'file', path=path, rel=rel, fd=fd, access=access, writable=True)
        except OSError as e:
            return os_status(e), None, 0
        if fattrs & (ATTR_HIDDEN | ATTR_SYSTEM):
            meta_update(path, share.path, lambda e: e.__setitem__(
                'attr', (fattrs & (ATTR_HIDDEN | ATTR_SYSTEM | ATTR_ARCHIVE)) | (0 if op.isdir else ATTR_ARCHIVE)))
        if fattrs & ATTR_READONLY:
            os.chmod(path, os.stat(path).st_mode & ~0o222)
        action = 2
    if opts & FILE_DELETE_ON_CLOSE:
        if op.path == share.path:
            op.close()
            return CANNOT_DELETE, None, 0
        op.delete_pending = True
    return SUCCESS, op, action


def dir_entries(op, pattern):
    """[(name, host path)] in the directory op matching pattern."""
    out = []
    if op.kind != 'dir':
        return out
    if os.path.normpath(op.path) != os.path.normpath(op.root):
        out.append(('.', op.path))
        out.append(('..', os.path.dirname(op.path)))
    try:
        names = sorted(os.listdir(op.path), key=lambda n: (n.lower(), n))
    except OSError:
        names = []
    for n in names:
        if n == META or n == META + '.tmp':
            continue
        out.append((n, os.path.join(op.path, n)))
    return [(n, p) for n, p in out if wild_match(pattern, n)]


def encode_dir_entry(cls, name, fi, index, unicode=True, short=''):
    """An NT directory information entry (NextEntryOffset left 0) or None."""
    nb = name.encode('utf-16-le') if unicode else name.encode('cp850', 'replace')
    sb = short.encode('utf-16-le')[:24]
    common = struct.pack('<IQQQQQQI', index, fi.ct, fi.at, fi.wt, fi.chg, fi.eof, fi.alloc, fi.attrs)
    if cls == 1:
        body = common + struct.pack('<I', len(nb)) + nb
    elif cls == 2:
        body = common + struct.pack('<II', len(nb), fi.easize) + nb
    elif cls == 3:
        body = common + struct.pack('<IIBB', len(nb), fi.easize, len(sb), 0) + sb.ljust(24, b'\0') + nb
    elif cls == 37:
        body = (common + struct.pack('<IIBB', len(nb), fi.easize, len(sb), 0) + sb.ljust(24, b'\0')
                + struct.pack('<HQ', 0, fi.ino) + nb)
    elif cls == 38:
        body = common + struct.pack('<IIIQ', len(nb), fi.easize, 0, fi.ino) + nb
    elif cls == 12:
        body = struct.pack('<II', index, len(nb)) + nb
    else:
        return None
    return b'\0\0\0\0' + body


def pack_entries(entries, maxlen, align=8):
    out = bytearray()
    last = None
    count = 0
    for e in entries:
        pad = (-len(out)) % align if last is not None else 0
        if len(out) + pad + len(e) > maxlen:
            break
        if last is not None:
            out += b'\0' * pad
            struct.pack_into('<I', out, last, len(out) - last)
        last = len(out)
        out += e
        count += 1
    return bytes(out), count


def encode_listing(op, entries, pos, cls, maxlen, single, unicode=True):
    """Packs entries[pos:] -> (buffer, number consumed, last name)."""
    used = set()
    encoded = []
    names = []
    consumed = 0
    size = 0
    for i in range(pos, len(entries)):
        name, path = entries[i]
        try:
            fi = file_info(path, op.root)
        except OSError:
            consumed += 1
            continue
        e = encode_dir_entry(cls, name, fi, i + 1, unicode, short_name(name, used))
        if e is None:
            return None, 0, None
        size += len(e) + 8
        encoded.append(e)
        names.append(name)
        consumed += 1
        if single or size > maxlen:
            break
    buf, count = pack_entries(encoded, maxlen)
    # entries skipped (vanished) before the last packed one count as consumed
    if count < len(encoded):
        consumed -= len(encoded) - count
    return buf, consumed, names[count - 1] if count else None


def query_file_info(op, cls, conn, unicode=True):
    """NT file information class -> (status, bytes)."""
    try:
        fi = op.info()
    except OSError as e:
        return os_status(e), b''
    basic = struct.pack('<QQQQII', fi.ct, fi.at, fi.wt, fi.chg, fi.attrs, 0)
    standard = struct.pack('<QQIBBH', fi.alloc, fi.eof, fi.nlink, 1 if op.delete_pending else 0,
                           1 if fi.isdir else 0, 0)
    rel = op.rel if op.rel else '\\'
    nb = rel.encode('utf-16-le')
    if cls == 4:
        return SUCCESS, basic
    if cls == 5:
        return SUCCESS, standard
    if cls == 6:
        return SUCCESS, struct.pack('<Q', fi.ino)
    if cls == 7:
        return SUCCESS, struct.pack('<I', fi.easize)
    if cls == 8:
        return SUCCESS, struct.pack('<I', op.access or 0x001F01FF)
    if cls == 9:
        return SUCCESS, struct.pack('<I', len(nb)) + nb
    if cls == 14:
        return SUCCESS, struct.pack('<Q', 0)
    if cls in (16, 17):
        return SUCCESS, struct.pack('<I', 0)
    if cls == 18:
        return SUCCESS, (basic + standard + struct.pack('<QIIQII', fi.ino, fi.easize, op.access or 0x001F01FF, 0, 0, 0)
                         + struct.pack('<I', len(nb)) + nb)
    if cls == 21:
        return SUCCESS, struct.pack('<I', len(nb)) + nb
    if cls == 22:
        if fi.isdir:
            return SUCCESS, b''
        sn = '::$DATA'.encode('utf-16-le')
        return SUCCESS, struct.pack('<IIQQ', 0, len(sn), fi.eof, fi.alloc) + sn
    if cls == 28:
        return SUCCESS, struct.pack('<QHBB3s', fi.eof, 0, 0, 0, b'\0' * 3)
    if cls == 34:
        return SUCCESS, struct.pack('<QQQQQQII', fi.ct, fi.at, fi.wt, fi.chg, fi.alloc, fi.eof, fi.attrs, 0)
    if cls == 35:
        return SUCCESS, struct.pack('<II', fi.attrs, 0)
    if cls == 15:
        if not fi.eas:
            return NO_EAS_ON_FILE, b''
        return SUCCESS, full_ea_list(fi.eas)
    return INVALID_INFO_CLASS, b''


def query_full_eas(op, getlist):
    """FileFullEaInformation for the names in a FILE_GET_EA_INFORMATION list (all if empty)."""
    fi = op.info()
    eas = fi.eas
    if getlist:
        want = []
        off = 0
        while off + 5 <= len(getlist):
            neo, nl = struct.unpack_from('<IB', getlist, off)
            want.append(getlist[off + 5:off + 5 + nl])
            if not neo:
                break
            off += neo
        have = {n.upper(): (n, f, v) for n, f, v in eas}
        eas = [have.get(w.upper(), (w.upper(), 0, b'')) for w in want]
    elif not eas:
        return NO_EAS_ON_FILE, b''
    return SUCCESS, full_ea_list(eas)


def set_basic(op, ct, at, wt, chg, attrs):
    if op.kind not in ('file', 'dir'):
        return SUCCESS
    keep = (0, 0xFFFFFFFFFFFFFFFF, 0xFFFFFFFFFFFFFFFE)
    try:
        if at not in keep or wt not in keep:
            st = os.stat(op.path)
            os.utime(op.path, ns=(ft_to_ns(at) if at not in keep else st.st_atime_ns,
                                  ft_to_ns(wt) if wt not in keep else st.st_mtime_ns))
        if attrs and attrs != 0xFFFFFFFF:
            if attrs == ATTR_NORMAL:
                attrs = 0
            mode = os.stat(op.path).st_mode
            if attrs & ATTR_READONLY:
                os.chmod(op.path, mode & ~0o222)
            elif not mode & 0o200:
                os.chmod(op.path, mode | 0o200)
            op.writable = op.writable and not attrs & ATTR_READONLY
            meta_update(op.path, op.root, lambda e: e.__setitem__(
                'attr', attrs & (ATTR_HIDDEN | ATTR_SYSTEM | ATTR_ARCHIVE)))
    except OSError as e:
        return os_status(e)
    return SUCCESS


def set_disposition(op, delete):
    if op.kind not in ('file', 'dir'):
        return SUCCESS
    if delete:
        if op.path == op.root:
            return CANNOT_DELETE
        try:
            st = os.stat(op.path)
        except OSError as e:
            return os_status(e)
        if not st.st_mode & 0o200 and not op.isdir:
            return CANNOT_DELETE
        if op.isdir and not dir_empty(op.path):
            return DIRECTORY_NOT_EMPTY
    op.delete_pending = bool(delete)
    return SUCCESS


def set_eof(op, size):
    if op.fd is None:
        return INVALID_PARAMETER
    if not op.writable:
        return ACCESS_DENIED
    try:
        os.ftruncate(op.fd, size)
    except OSError as e:
        return os_status(e)
    return SUCCESS


def do_rename(op, target, replace, share_relative=True):
    """Renames op to `target` (share-relative, or relative to op's directory)."""
    if op.kind not in ('file', 'dir'):
        return ACCESS_DENIED
    if op.path == op.root:
        return ACCESS_DENIED
    if not share_relative and '\\' not in target.strip('\\'):
        parent = op.rel.rsplit('\\', 1)[0]
        target = parent + '\\' + target.strip('\\')
    st, newpath, exists, newrel = resolve(op.root, target)
    if st:
        return st
    if os.path.basename(newpath).lower() == META.lower():
        return ACCESS_DENIED
    if exists:
        same = False
        try:
            same = os.path.samefile(newpath, op.path)
        except OSError:
            pass
        if not same:
            if not replace:
                return OBJECT_NAME_COLLISION
            if os.path.isdir(newpath):
                return ACCESS_DENIED
    try:
        os.rename(op.path, newpath)
    except OSError as e:
        return os_status(e)
    meta_move(op.path, newpath, op.root)
    log('  renamed %s -> %s' % (op.rel, newrel))
    op.path, op.rel = newpath, newrel
    return SUCCESS


def set_file_info(op, cls, data, share_relative=True):
    try:
        if cls == 4:
            ct, at, wt, chg, attrs = struct.unpack_from('<QQQQI', data)
            return set_basic(op, ct, at, wt, chg, attrs)
        if cls == 10:
            replace = data[0]
            nl = u32(data, 16)
            target = data[20:20 + nl].decode('utf-16-le')
            return do_rename(op, target, replace, share_relative)
        if cls in (13, 64):
            return set_disposition(op, data[0] & 1)
        if cls == 19:
            size = u64(data, 0)
            if op.fd is not None and size < os.fstat(op.fd).st_size:
                return set_eof(op, size)
            return SUCCESS
        if cls == 20:
            return set_eof(op, u64(data, 0))
        if cls == 15:
            if op.kind not in ('file', 'dir'):
                return ACCESS_DENIED
            new = parse_full_ea_list(data)
            eas = {n.upper(): (n, f, v) for n, f, v in get_eas(op.path, op.root)}
            for n, f, v in new:
                if v:
                    eas[n.upper()] = (n.upper(), f, v)
                else:
                    eas.pop(n.upper(), None)
            set_eas(op.path, op.root, list(eas.values()))
            return SUCCESS
        if cls in (14, 16):
            return SUCCESS
    except (struct.error, IndexError, UnicodeDecodeError):
        return INVALID_PARAMETER
    return INVALID_INFO_CLASS


def query_fs_info(share, cls):
    if share.kind == 'disk':
        try:
            v = os.statvfs(share.path)
            total = v.f_blocks * v.f_frsize // 4096
            free = v.f_bavail * v.f_frsize // 4096
        except OSError:
            total, free = 1 << 20, 1 << 19
    else:
        total, free = 0, 0
    label = share.name.encode('utf-16-le')
    serial = zlib.crc32(share.name.encode()) & 0xffffffff
    if cls == 1:
        return SUCCESS, struct.pack('<QIIBB', 0, serial, len(label), 0, 0) + label
    if cls == 3:
        return SUCCESS, struct.pack('<QQII', total, free, 8, 512)
    if cls == 4:
        return SUCCESS, struct.pack('<II', 7 if share.kind != 'ipc' else 0x11, 0x10)
    if cls == 5:
        fsn = 'NTFS'.encode('utf-16-le')
        return SUCCESS, struct.pack('<III', 0x2 | 0x4 | 0x00800000 | 0x00040000, 255, len(fsn)) + fsn
    if cls == 7:
        return SUCCESS, struct.pack('<QQQII', total, free, free, 8, 512)
    if cls == 8:
        return SUCCESS, b'\0' * 48
    if cls == 11:
        return SUCCESS, struct.pack('<IIIIII', 512, 4096, 4096, 4096, 0, 0)[:20] + b'\0' * 8
    return INVALID_INFO_CLASS, b''


def security_descriptor(addl):
    everyone = bytes([1, 1, 0, 0, 0, 0, 0, 1]) + struct.pack('<I', 0)
    ace = struct.pack('<BBHI', 0, 0, 8 + len(everyone), 0x001F01FF) + everyone
    acl = struct.pack('<BBHHH', 2, 0, 8 + len(ace), 1, 0) + ace
    body = b''
    off = 20
    owner = group = dacl = 0
    if addl & 1:
        owner = off + len(body)
        body += everyone
    if addl & 2:
        group = off + len(body)
        body += everyone
    ctl = 0x8000
    if addl & 4:
        dacl = off + len(body)
        body += acl
        ctl |= 0x4
    return struct.pack('<BBHIIII', 1, 0, ctl, owner, group, 0, dacl) + body


# ---------------------------------------------------------------- DCE/RPC pipes

def syntax(u, major, minor=0):
    return uuid.UUID(u).bytes_le + struct.pack('<HH', major, minor)


SRVSVC = syntax('4b324fc8-1670-01d3-1278-5a47bf6ee188', 3)
WKSSVC = syntax('6bffd098-a112-3610-9833-46c3f87e345a', 1)
NDR_SYNTAX = syntax('8a885d04-1ceb-11c9-9fe8-08002b104860', 2)
BTFN_PREFIX = uuid.UUID('6cb71c2c-9812-4540-0000-000000000000').bytes_le[:8]


class NdrW:
    def __init__(self):
        self.b = bytearray()
        self.ref = 0x20000

    def align(self, n):
        self.b += b'\0' * ((-len(self.b)) % n)

    def u32(self, v):
        self.align(4)
        self.b += struct.pack('<I', v)

    def ptr(self, present=True):
        if present:
            self.u32(self.ref)
            self.ref += 4
        else:
            self.u32(0)

    def wstr(self, s):
        e = (s + '\0').encode('utf-16-le')
        n = len(e) // 2
        self.u32(n)
        self.u32(0)
        self.u32(n)
        self.b += e


def ndr_wstr(b, off):
    """A conformant varying string at off -> (str, next offset aligned to 4)."""
    off += (-off) % 4
    mx, o, n = struct.unpack_from('<III', b, off)
    s = b[off + 12:off + 12 + 2 * n].decode('utf-16-le', 'replace').rstrip('\0')
    off += 12 + 2 * n
    return s, off + (-off) % 4


def ndr_unique_wstr(b, off):
    ref = u32(b, off)
    off += 4
    if ref:
        return ndr_wstr(b, off)
    return None, off


class RpcPipe:
    def __init__(self, srv, name):
        self.srv, self.name = srv, name
        self.inbuf = bytearray()
        self.out = collections.deque()
        self.ctx = {}
        self.calls = {}
        self.max_frag = 4280

    def write(self, data):
        self.inbuf += data
        while len(self.inbuf) >= 16:
            flen = u16(self.inbuf, 8)
            if flen < 16:
                self.inbuf.clear()
                break
            if len(self.inbuf) < flen:
                break
            pdu = bytes(self.inbuf[:flen])
            del self.inbuf[:flen]
            try:
                self.pdu(pdu)
            except (struct.error, IndexError, ValueError) as e:
                log('  rpc: bad PDU on %s: %s' % (self.name, e))

    def read(self, n):
        if not self.out:
            return b'', PIPE_EMPTY
        msg = self.out[0]
        if len(msg) > n:
            self.out[0] = msg[n:]
            return msg[:n], BUFFER_OVERFLOW
        self.out.popleft()
        return msg, SUCCESS

    def send(self, ptype, flags, call_id, body):
        self.out.append(struct.pack('<BBBBIHHI', 5, 0, ptype, flags, 0x10, 16 + len(body), 0, call_id) + body)

    def pdu(self, pdu):
        ptype, flags = pdu[2], pdu[3]
        call_id = u32(pdu, 12)
        if ptype in (11, 14):
            max_xmit, max_recv, assoc = struct.unpack_from('<HHI', pdu, 16)
            self.max_frag = max(min(max_recv, 4280), 512)
            n = pdu[24]
            off = 28
            results = []
            for _ in range(n):
                cid, nts = u16(pdu, off), pdu[off + 2]
                abstract = pdu[off + 4:off + 24]
                off += 24
                tss = [pdu[off + 20 * j:off + 20 * j + 20] for j in range(nts)]
                off += 20 * nts
                iface = {SRVSVC: 'srvsvc', WKSSVC: 'wkssvc'}.get(abstract)
                if iface is None:
                    res = (2, 1, b'\0' * 20)
                elif NDR_SYNTAX in tss:
                    res = (0, 0, NDR_SYNTAX)
                    self.ctx[cid] = iface
                elif any(t[:8] == BTFN_PREFIX for t in tss):
                    res = (3, 0, b'\0' * 20)
                else:
                    res = (2, 2, b'\0' * 20)
                results.append(res)
            sec = ('\\PIPE\\%s\0' % self.name).encode() if ptype == 11 else b''
            body = struct.pack('<HHIH', 4280, 4280, assoc or 0x53f0, len(sec)) + sec
            body += b'\0' * ((-(16 + len(body))) % 4)
            body += struct.pack('<BBH', len(results), 0, 0)
            for r in results:
                body += struct.pack('<HH', r[0], r[1]) + r[2]
            log('  rpc: %s on %s: %s' % ('BIND' if ptype == 11 else 'ALTER_CONTEXT', self.name,
                                         ['accept' if r[0] == 0 else 'reject %d' % r[1] for r in results]))
            self.send(12 if ptype == 11 else 15, 3, call_id, body)
            return
        if ptype == 0:
            alloc, cid, opnum = struct.unpack_from('<IHH', pdu, 16)
            off = 24 + (16 if flags & 0x80 else 0)
            auth = u16(pdu, 10)
            end = len(pdu) - (auth + 8 if auth else 0)
            buf = self.calls.setdefault(call_id, bytearray())
            buf += pdu[off:end]
            if not flags & 2:
                return
            stub = bytes(self.calls.pop(call_id))
            iface = self.ctx.get(cid)
            out = None
            if iface == 'srvsvc':
                out = self.srvsvc(opnum, stub)
            elif iface == 'wkssvc':
                out = self.wkssvc(opnum, stub)
            if out is None:
                log('  rpc: %s opnum %d -> fault' % (iface, opnum))
                self.send(3, 3, call_id, struct.pack('<IHBBII', 0, cid, 0, 0, 0x1c010002, 0))
                return
            chunk = self.max_frag - 24
            pos = 0
            while True:
                part = out[pos:pos + chunk]
                f = (1 if pos == 0 else 0) | (2 if pos + chunk >= len(out) else 0)
                self.send(2, f, call_id, struct.pack('<IHBB', len(out) - pos, cid, 0, 0) + part)
                pos += chunk
                if pos >= len(out):
                    break
            return
        log('  rpc: PDU type %d ignored on %s' % (ptype, self.name))

    def shares(self):
        return list(self.srv.shares.values())

    def srvsvc(self, opnum, stub):
        srv = self.srv
        w = NdrW()
        if opnum == 15:
            _, off = ndr_unique_wstr(stub, 0)
            level = u32(stub, off)
            shares = self.shares()
            log('  rpc: NetrShareEnum level %d: %d shares' % (level, len(shares)))
            w.u32(level)
            w.u32(level)
            if level not in (0, 1):
                w.u32(0)
                w.u32(0)
                w.ptr(False)
                w.u32(124)
                return bytes(w.b)
            w.ptr()
            w.u32(len(shares))
            w.ptr(bool(shares))
            if shares:
                w.u32(len(shares))
                for s in shares:
                    w.ptr()
                    if level == 1:
                        w.u32(s.stype)
                        w.ptr()
                for s in shares:
                    w.wstr(s.name)
                    if level == 1:
                        w.wstr(s.remark)
            w.u32(len(shares))
            w.ptr()
            w.u32(0)
            w.u32(0)
            return bytes(w.b)
        if opnum == 16:
            _, off = ndr_unique_wstr(stub, 0)
            name, off = ndr_wstr(stub, off)
            level = u32(stub, off)
            s = srv.shares.get(name.upper())
            log('  rpc: NetrShareGetInfo %r level %d: %s' % (name, level, 'found' if s else 'not found'))
            w.u32(level)
            if s is None or level not in (0, 1):
                w.ptr(False)
                w.u32(2310 if s is None else 124)
                return bytes(w.b)
            w.ptr()
            w.ptr()
            if level == 1:
                w.u32(s.stype)
                w.ptr()
            w.wstr(s.name)
            if level == 1:
                w.wstr(s.remark)
            w.u32(0)
            return bytes(w.b)
        if opnum == 21:
            _, off = ndr_unique_wstr(stub, 0)
            level = u32(stub, off)
            log('  rpc: NetrServerGetInfo level %d' % level)
            w.u32(level)
            if level not in (100, 101):
                w.ptr(False)
                w.u32(124)
                return bytes(w.b)
            w.ptr()
            w.u32(500)
            w.ptr()
            if level == 101:
                w.u32(10)
                w.u32(0)
                w.u32(srv.server_type)
                w.ptr()
            w.wstr(srv.name)
            if level == 101:
                w.wstr(srv.comment)
            w.u32(0)
            return bytes(w.b)
        return None

    def wkssvc(self, opnum, stub):
        srv = self.srv
        if opnum != 0:
            return None
        _, off = ndr_unique_wstr(stub, 0)
        level = u32(stub, off)
        log('  rpc: NetrWkstaGetInfo level %d' % level)
        w = NdrW()
        w.u32(level)
        if level != 100:
            w.ptr(False)
            w.u32(124)
            return bytes(w.b)
        w.ptr()
        w.u32(500)
        w.ptr()
        w.ptr()
        w.u32(10)
        w.u32(0)
        w.wstr(srv.name)
        w.wstr(srv.workgroup)
        w.u32(0)
        return bytes(w.b)


# ---------------------------------------------------------------- sessions

class Session:
    def __init__(self, sid):
        self.sid = sid
        self.ntlm = None
        self.user = None
        self.domain = ''
        self.key = None
        self.kind = None
        self.valid = False
        self.signing = False
        self.trees = {}


SMB2_CMDS = ['NEGOTIATE', 'SESSION_SETUP', 'LOGOFF', 'TREE_CONNECT', 'TREE_DISCONNECT', 'CREATE', 'CLOSE',
             'FLUSH', 'READ', 'WRITE', 'LOCK', 'IOCTL', 'CANCEL', 'ECHO', 'QUERY_DIRECTORY', 'CHANGE_NOTIFY',
             'QUERY_INFO', 'SET_INFO', 'OPLOCK_BREAK']
SMB2_HDR = struct.Struct('<4sHHIHHIIQIIQ16s')
SMB2_ERROR = struct.pack('<HBBI', 9, 0, 0, 0) + b'\0'
FLAG_RESPONSE, FLAG_ASYNC, FLAG_RELATED, FLAG_SIGNED = 0x1, 0x2, 0x4, 0x8


class Req:
    pass


def smb2_sign(key, pkt):
    pkt = bytearray(pkt)
    pkt[48:64] = b'\0' * 16
    return hmac.new(key, bytes(pkt), hashlib.sha256).digest()[:16]


class Conn:
    def __init__(self, srv, sock, addr, nb):
        self.srv, self.sock, self.addr, self.nb = srv, sock, addr, nb
        self.tag = '%s:%d' % addr[:2]
        self.dialect = None
        self.sessions = {}
        self.opens = {}
        self.next_id = 1
        self.nb_ok = not nb
        self.smb1_state = None

    def new_id(self):
        n = self.next_id
        self.next_id += 1
        return n

    def recvn(self, n):
        buf = bytearray()
        while len(buf) < n:
            d = self.sock.recv(n - len(buf))
            if not d:
                return None
            buf += d
        return bytes(buf)

    def send(self, payload, typ=0):
        self.sock.sendall(bytes([typ]) + len(payload).to_bytes(3, 'big') + payload)

    def run(self):
        log('%s connected (%s)' % (self.tag, 'NetBIOS session' if self.nb else 'direct TCP'))
        try:
            while True:
                hdr = self.recvn(4)
                if hdr is None:
                    break
                typ = hdr[0]
                ln = int.from_bytes(hdr[1:4], 'big')
                data = self.recvn(ln) if ln else b''
                if data is None:
                    break
                if typ == 0x81:
                    self.session_request(data)
                    continue
                if typ == 0x85:
                    self.send(b'', 0x85)
                    continue
                if typ != 0:
                    log('%s NetBIOS packet type 0x%02x ignored' % (self.tag, typ))
                    continue
                if not self.nb_ok:
                    log('%s message before SESSION REQUEST; dropping connection' % self.tag)
                    break
                if data[:4] == b'\xfeSMB':
                    if self.srv.smb1_only:
                        log('%s SMB2 message refused (--smb1); dropping connection' % self.tag)
                        break
                    self.smb2(data)
                elif data[:4] == b'\xffSMB':
                    self.smb1(data)
                else:
                    log('%s unknown message %r; dropping connection' % (self.tag, data[:8]))
                    break
        except (ConnectionError, OSError) as e:
            log('%s connection error: %s' % (self.tag, e))
        except (struct.error, IndexError, ValueError, UnicodeDecodeError) as e:
            log('%s malformed message (%s: %s); dropping connection' % (self.tag, type(e).__name__, e))
        except Exception:
            log('%s server error, dropping connection:\n%s' % (self.tag, traceback.format_exc()))
        finally:
            for op in list(self.opens.values()):
                op.close()
            self.opens.clear()
            try:
                self.sock.close()
            except OSError:
                pass
            log('%s disconnected' % self.tag)

    def session_request(self, data):
        called, csfx, off = nb_decode(data, 0)
        calling, gsfx, _ = nb_decode(data, off)
        ok = called.upper() in (self.srv.name.upper(), '*SMBSERVER') or called == '*'
        log('%s SESSION REQUEST called %s<%02X> calling %s<%02X> -> %s' % (
            self.tag, called, csfx, calling, gsfx, 'positive' if ok else 'negative'))
        if ok:
            self.nb_ok = True
            self.send(b'', 0x82)
        else:
            self.send(b'\x82', 0x83)

    # ------------------------------------------------------------ SMB2

    def smb2(self, data):
        off = 0
        last = Req()
        last.fid, last.sid, last.tid, last.status = None, 0, 0, SUCCESS
        responses = []
        while True:
            nxt = u32(data, off + 20)
            msg = data[off:off + nxt] if nxt else data[off:]
            r = self.smb2_one(msg, last)
            if r is not None:
                responses.append(r)
            if not nxt or off + nxt >= len(data):
                break
            off += nxt
        if not responses:
            return
        out = bytearray()
        for i, (pkt, key) in enumerate(responses):
            pkt = bytearray(pkt)
            if i < len(responses) - 1:
                pkt += b'\0' * ((-len(pkt)) % 8)
                struct.pack_into('<I', pkt, 20, len(pkt))
            if key:
                struct.pack_into('<I', pkt, 16, u32(pkt, 16) | FLAG_SIGNED)
                pkt[48:64] = smb2_sign(key, pkt)
            out += pkt
        self.send(bytes(out))

    def smb2_one(self, msg, last):
        (_, _, charge, _, cmd, credreq, flags, _, mid, pid, tid, sid, sig) = SMB2_HDR.unpack_from(msg, 0)
        related = flags & FLAG_RELATED
        if related:
            sid, tid = last.sid, last.tid
        rq = Req()
        rq.msg, rq.body, rq.cmd, rq.mid, rq.sid, rq.tid, rq.last = msg, msg[64:], cmd, mid, sid, tid, last
        rq.detail = ''
        name = SMB2_CMDS[cmd] if cmd < len(SMB2_CMDS) else 'CMD_%d' % cmd
        if cmd == 12:
            log('%s smb2 CANCEL mid %d (nothing pending)' % (self.tag, mid))
            return None
        sess = self.sessions.get(sid) if sid else None
        rq.sess = sess
        status = None
        sign_key = None
        if cmd not in (0, 1) and cmd != 13 or (cmd == 13 and sid):
            if sess is None or not sess.valid:
                status = USER_SESSION_DELETED
        if status is None and sess and sess.valid and sess.key and cmd not in (0, 1):
            if flags & FLAG_SIGNED:
                if smb2_sign(sess.key, msg) != sig:
                    log('%s smb2 %s: bad signature' % (self.tag, name))
                    status = ACCESS_DENIED
            elif sess.signing:
                log('%s smb2 %s: unsigned request on a signing session' % (self.tag, name))
                status = ACCESS_DENIED
            if flags & FLAG_SIGNED or sess.signing:
                sign_key = sess.key
        if status is None and related and is_error(last.status):
            status = last.status
        body = None
        if status is None:
            h = getattr(self, 'smb2_' + name.lower(), None)
            if h is None:
                status = NOT_SUPPORTED
            else:
                try:
                    status, body = h(rq)
                except (struct.error, IndexError, UnicodeDecodeError) as e:
                    log('%s smb2 %s: malformed request (%s)' % (self.tag, name, e))
                    status, body = INVALID_PARAMETER, None
        if cmd == 1 and status == SUCCESS:
            s = self.sessions.get(rq.sid)
            if s and s.signing and s.key:
                sign_key = s.key
        last.sid, last.tid, last.status = rq.sid, rq.tid, status
        if is_error(status) and not (cmd == 1 and status == MORE_PROCESSING_REQUIRED) or body is None:
            body = SMB2_ERROR
        credits = max(credreq, charge, 1)
        credits = max(credits, 32) if cmd in (0, 1) else credits
        hdr = SMB2_HDR.pack(b'\xfeSMB', 64, charge, status, cmd, credits,
                            FLAG_RESPONSE | (flags & FLAG_RELATED), 0, mid, pid, rq.tid, rq.sid, b'\0' * 16)
        log('%s smb2 %s%s -> %s' % (self.tag, name, ' ' + rq.detail if rq.detail else '', stname(status)))
        return hdr + body, sign_key

    def tree_of(self, rq):
        t = rq.sess.trees.get(rq.tid) if rq.sess else None
        return t

    def open_of(self, rq, fid):
        if fid == b'\xff' * 16:
            fid = rq.last.fid
        op = self.opens.get(fid)
        if op is not None:
            rq.last.fid = fid
            rq.detail = (op.tree.share.name + op.rel) if op.rel else op.tree.share.name
        return op

    def negotiate_body(self, d):
        srv = self.srv
        self.dialect = d
        mx = 65536 if d in (0x0202, 0x02FF) else srv.max_io
        caps = 0x4 if d == 0x0210 and mx > 65536 else 0
        secmode = 1 | (2 if srv.require_signing else 0)
        blob = spnego_init()
        return struct.pack('<HHHH16sIIIIQQHHI', 65, secmode, d, 0, srv.guid, caps, mx, mx, mx,
                           now_ft(), srv.start_ft, 128, len(blob), 0) + blob

    def smb2_negotiate(self, rq):
        b = rq.body
        cnt = u16(b, 2)
        dialects = [u16(b, 36 + 2 * i) for i in range(cnt)]
        choose = [d for d in (0x0210, 0x0202) if d in dialects and d <= self.srv.max_dialect]
        rq.detail = 'dialects ' + ','.join('%04x' % d for d in dialects)
        if not choose:
            return NOT_SUPPORTED, None
        rq.detail += ' -> %04x' % choose[0]
        return SUCCESS, self.negotiate_body(choose[0])

    def smb2_session_setup(self, rq):
        b = rq.body
        secmode = b[3]
        blob = rq.msg[u16(b, 12):u16(b, 12) + u16(b, 14)]
        tok, spnego = extract_ntlm(blob)
        if tok is None or len(tok) < 12:
            rq.detail = 'no NTLMSSP token'
            return LOGON_FAILURE, None
        mtype = u32(tok, 8)
        if mtype == 1:
            sid = 0x0000040000000000 | self.new_id()
            sess = Session(sid)
            sess.ntlm = NtlmServer(self.srv)
            sess.ntlm.mech_types = spnego_mech_types(blob) if spnego else None
            self.sessions[sid] = sess
            rq.sid = sid
            chal = sess.ntlm.challenge_msg(tok)
            out = spnego_resp(1, chal) if spnego else chal
            rq.detail = 'NTLMSSP NEGOTIATE'
            return MORE_PROCESSING_REQUIRED, struct.pack('<HHHH', 9, 0, 72, len(out)) + out
        if mtype == 3:
            sess = self.sessions.get(rq.sid)
            if sess is None or sess.ntlm is None:
                rq.detail = 'AUTHENTICATE without NEGOTIATE'
                return USER_SESSION_DELETED, None
            st, user, dom, key, kind = sess.ntlm.authenticate(tok)
            rq.detail = 'NTLMSSP AUTHENTICATE %s\\%s' % (dom, user)
            if st:
                del self.sessions[rq.sid]
                return st, None
            sess.user, sess.domain, sess.kind, sess.valid = (user if kind == 'user' else (user or '')), dom, kind, True
            sess.key = key if kind == 'user' else None
            sess.signing = bool(sess.key) and bool(self.srv.require_signing or secmode & 2)
            sflags = {'guest': 1, 'anonymous': 2}.get(kind, 0)
            out = spnego_resp(0, mech=False, mic=sess.ntlm.mic()) if spnego else b''
            sess.ntlm = None
            rq.detail += ' (%s%s)' % (kind, ', signing' if sess.signing else '')
            return SUCCESS, struct.pack('<HHHH', 9, sflags, 72 if out else 0, len(out)) + out
        rq.detail = 'NTLMSSP message type %d' % mtype
        return LOGON_FAILURE, None

    def close_opens(self, pred):
        for k, op in list(self.opens.items()):
            if pred(op):
                op.close()
                del self.opens[k]

    def smb2_logoff(self, rq):
        sess = rq.sess
        self.close_opens(lambda op: op.tree.sess is sess)
        self.sessions.pop(rq.sid, None)
        rq.detail = sess.user or ''
        return SUCCESS, struct.pack('<HH', 4, 0)

    def smb2_tree_connect(self, rq):
        b = rq.body
        path = rq.msg[u16(b, 4):u16(b, 4) + u16(b, 6)].decode('utf-16-le')
        rq.detail = path
        share = self.srv.shares.get(path.rstrip('\\').rsplit('\\', 1)[-1].upper())
        if share is None:
            return BAD_NETWORK_NAME, None
        tid = self.new_id()
        rq.sess.trees[tid] = Tree(tid, share, rq.sess)
        rq.tid = tid
        stype = {'disk': 1, 'ipc': 2, 'print': 3}[share.kind]
        return SUCCESS, struct.pack('<HBBIII', 16, stype, 0, 0x30 if share.kind == 'ipc' else 0, 0, 0x001F01FF)

    def smb2_tree_disconnect(self, rq):
        t = self.tree_of(rq)
        if t is None:
            return NETWORK_NAME_DELETED, None
        rq.detail = t.share.name
        self.close_opens(lambda op: op.tree is t)
        del rq.sess.trees[rq.tid]
        return SUCCESS, struct.pack('<HH', 4, 0)

    def smb2_create(self, rq):
        t = self.tree_of(rq)
        if t is None:
            return NETWORK_NAME_DELETED, None
        b = rq.body
        access, fattrs, _, disp, opts = struct.unpack_from('<IIIII', b, 24)
        name = rq.msg[u16(b, 44):u16(b, 44) + u16(b, 46)].decode('utf-16-le')
        rq.detail = '%s\\%s disp %d opts 0x%x access 0x%x' % (t.share.name, name, disp, opts, access)
        st, op, action = fs_create(self, t, name, disp, opts, access, fattrs)
        if st:
            return st, None
        fid = struct.pack('<QQ', self.new_id(), self.next_id * 7 + 1)
        self.opens[fid] = op
        rq.last.fid = fid
        fi = op.info()
        return SUCCESS, struct.pack('<HBBIQQQQQQII16sII', 89, 0, 0, action, fi.ct, fi.at, fi.wt, fi.chg,
                                    fi.alloc, fi.eof, fi.attrs, 0, fid, 0, 0) + b'\0'

    def smb2_close(self, rq):
        b = rq.body
        flags = u16(b, 2)
        fid = b[8:24]
        op = self.open_of(rq, fid)
        if op is None:
            return FILE_CLOSED, None
        fi = None
        if flags & 1:
            try:
                fi = op.info()
            except OSError:
                pass
        op.close()
        self.opens.pop(rq.last.fid, None)
        if fi:
            return SUCCESS, struct.pack('<HHIQQQQQQI', 60, 1, 0, fi.ct, fi.at, fi.wt, fi.chg, fi.alloc, fi.eof, fi.attrs)
        return SUCCESS, struct.pack('<HHIQQQQQQI', 60, 0, 0, 0, 0, 0, 0, 0, 0, 0)

    def smb2_flush(self, rq):
        op = self.open_of(rq, rq.body[8:24])
        if op is None:
            return FILE_CLOSED, None
        return SUCCESS, struct.pack('<HH', 4, 0)

    def max_io(self):
        return 65536 if self.dialect == 0x0202 else self.srv.max_io

    def smb2_read(self, rq):
        b = rq.body
        length, offset = u32(b, 4), u64(b, 8)
        minc = u32(b, 32)
        op = self.open_of(rq, b[16:32])
        if op is None:
            return FILE_CLOSED, None
        rq.detail += ' %d@%d' % (length, offset)
        if length > self.max_io():
            return INVALID_PARAMETER, None
        st = SUCCESS
        if op.kind == 'pipe':
            data, st = op.pipe.read(length)
            if st == PIPE_EMPTY:
                return st, None
        elif op.fd is None:
            return INVALID_DEVICE_REQUEST, None
        else:
            data = os.pread(op.fd, length, offset)
            if (not data and length) or len(data) < minc:
                return END_OF_FILE, None
        return st, struct.pack('<HBBIII', 17, 0x50, 0, len(data), 0, 0) + data

    def smb2_write(self, rq):
        b = rq.body
        doff, length, offset = u16(b, 2), u32(b, 4), u64(b, 8)
        op = self.open_of(rq, b[16:32])
        if op is None:
            return FILE_CLOSED, None
        rq.detail += ' %d@%d' % (length, offset)
        if length > self.max_io():
            return INVALID_PARAMETER, None
        data = rq.msg[doff:doff + length]
        if op.kind == 'pipe':
            op.pipe.write(data)
        elif op.fd is None:
            return INVALID_DEVICE_REQUEST, None
        elif not op.writable:
            return ACCESS_DENIED, None
        else:
            if offset == 0xFFFFFFFFFFFFFFFF:
                offset = os.fstat(op.fd).st_size
            try:
                os.pwrite(op.fd, data, offset)
            except OSError as e:
                return os_status(e), None
            op.written += len(data)
        return SUCCESS, struct.pack('<HHIIHH', 17, 0, len(data), 0, 0, 0) + b'\0'

    def smb2_lock(self, rq):
        op = self.open_of(rq, rq.body[8:24])
        if op is None:
            return FILE_CLOSED, None
        return SUCCESS, struct.pack('<HH', 4, 0)

    def smb2_echo(self, rq):
        return SUCCESS, struct.pack('<HH', 4, 0)

    def smb2_ioctl(self, rq):
        b = rq.body
        ctl = u32(b, 4)
        fid = b[8:24]
        inoff, incnt = u32(b, 24), u32(b, 28)
        maxout = u32(b, 44)
        indata = rq.msg[inoff:inoff + incnt]
        rq.detail = 'ctl 0x%08x' % ctl
        out = b''
        st = SUCCESS
        if ctl == 0x0011C017:
            op = self.open_of(rq, fid)
            if op is None:
                return FILE_CLOSED, None
            if op.kind != 'pipe':
                return INVALID_DEVICE_REQUEST, None
            rq.detail += ' PIPE_TRANSCEIVE'
            op.pipe.write(indata)
            out, st = op.pipe.read(maxout)
            if st == PIPE_EMPTY:
                out, st = b'', SUCCESS
        elif ctl == 0x00060194:
            rq.detail += ' DFS_GET_REFERRALS'
            return NOT_FOUND, None
        elif ctl == 0x00110018:
            rq.detail += ' PIPE_WAIT'
        else:
            return NOT_SUPPORTED, None
        return st, struct.pack('<HHI16sIIIIII', 49, 0, ctl, fid, 112, 0, 112, len(out), 0, 0) + out

    def smb2_query_directory(self, rq):
        b = rq.body
        cls, flags, index = b[2], b[3], u32(b, 4)
        noff, nlen, outlen = u16(b, 24), u16(b, 26), u32(b, 28)
        pattern = rq.msg[noff:noff + nlen].decode('utf-16-le') or '*'
        op = self.open_of(rq, b[8:24])
        if op is None:
            return FILE_CLOSED, None
        rq.detail += ' %r class %d flags 0x%x' % (pattern, cls, flags)
        if op.kind != 'dir':
            return INVALID_PARAMETER if op.kind not in ('printroot',) else NO_SUCH_FILE, None
        if cls not in (1, 2, 3, 12, 37, 38):
            return INVALID_INFO_CLASS, None
        first = op.search is None or flags & (1 | 0x10)
        if first:
            op.search = [dir_entries(op, pattern), 0]
            if not op.search[0]:
                return NO_SUCH_FILE, None
        if flags & 4:
            op.search[1] = index
        entries, pos = op.search
        if pos >= len(entries):
            return NO_MORE_FILES, None
        buf, consumed, _ = encode_listing(op, entries, pos, cls, outlen, flags & 2)
        if not buf:
            if pos + consumed >= len(entries):
                op.search[1] = pos + consumed
                return NO_MORE_FILES, None
            return INFO_LENGTH_MISMATCH, None
        op.search[1] = pos + consumed
        rq.detail += ' %d entries' % consumed
        return SUCCESS, struct.pack('<HHI', 9, 72, len(buf)) + buf

    def smb2_query_info(self, rq):
        b = rq.body
        itype, cls, outlen = b[2], b[3], u32(b, 4)
        inoff, inlen, addl = u16(b, 8), u32(b, 12), u32(b, 16)
        op = self.open_of(rq, b[24:40])
        if op is None:
            return FILE_CLOSED, None
        rq.detail += ' type %d class %d' % (itype, cls)
        if itype == 1:
            if cls == 15 and op.kind in ('file', 'dir'):
                st, data = query_full_eas(op, rq.msg[inoff:inoff + inlen] if inlen else b'')
            else:
                st, data = query_file_info(op, cls, self)
        elif itype == 2:
            st, data = query_fs_info(op.tree.share, cls)
        elif itype == 3:
            st, data = SUCCESS, security_descriptor(addl)
        else:
            st, data = NOT_SUPPORTED, b''
        if is_error(st):
            return st, None
        if len(data) > outlen:
            if itype == 3:
                return BUFFER_TOO_SMALL, struct.pack('<HHII', 9, 0, 4, len(data))
            if cls in (1, 5, 9, 15, 18, 22) or itype == 2 and cls in (1, 5):
                data = data[:outlen]
                st = BUFFER_OVERFLOW
            else:
                return INFO_LENGTH_MISMATCH, None
        return st, struct.pack('<HHI', 9, 72, len(data)) + data

    def smb2_set_info(self, rq):
        b = rq.body
        itype, cls, blen, boff = b[2], b[3], u32(b, 4), u16(b, 8)
        op = self.open_of(rq, b[16:32])
        if op is None:
            return FILE_CLOSED, None
        data = rq.msg[boff:boff + blen]
        rq.detail += ' type %d class %d' % (itype, cls)
        if itype == 1:
            st = set_file_info(op, cls, data)
        elif itype == 3:
            st = SUCCESS
        else:
            st = NOT_SUPPORTED
        if st:
            return st, None
        return SUCCESS, struct.pack('<H', 2)

    def smb2_change_notify(self, rq):
        return NOT_SUPPORTED, None

    # ------------------------------------------------------------ SMB1

    def smb1(self, data):
        if self.smb1_state is None:
            self.smb1_state = Smb1(self)
        self.smb1_state.handle(data)


# ---------------------------------------------------------------- NetBIOS names

def nb_decode(data, off):
    """First-level decoded NetBIOS name at off -> (name, suffix, next offset)."""
    ln = data[off]
    enc = data[off + 1:off + 1 + ln]
    raw = bytes(((enc[i] - 0x41) << 4) | ((enc[i + 1] - 0x41) & 0xf) for i in range(0, len(enc) - 1, 2))
    off += 1 + ln
    while off < len(data) and data[off]:
        if data[off] & 0xc0 == 0xc0:
            off += 1
            break
        off += 1 + data[off]
    off += 1
    name = raw[:15].rstrip(b' \0').decode('latin-1')
    return name, raw[15] if len(raw) >= 16 else 0x20, off


def nb_encode(name, suffix):
    raw = name.upper().encode('latin-1')[:15].ljust(15, b' ') + bytes([suffix])
    return bytes([32]) + bytes(c for b in raw for c in (0x41 + (b >> 4), 0x41 + (b & 15))) + b'\0'


def nbns_handle(srv, data):
    tid, flags, qd = struct.unpack_from('>HHH', data)
    if flags & 0x8000 or (flags >> 11) & 0xf != 0 or qd < 1:
        return None
    name, sfx, off = nb_decode(data, 12)
    qtype, qclass = struct.unpack_from('>HH', data, off)
    qraw = data[12:off]
    bcast = flags & 0x0010
    rd = flags & 0x0100
    up = name.upper()
    if qtype == 0x20:
        group = None
        if up == srv.name.upper() and sfx in (0x00, 0x20):
            group = False
        elif up == srv.workgroup.upper() and sfx in (0x00, 0x1e):
            group = True
        elif up == srv.workgroup.upper() and sfx in (0x1d, 0x1b):
            group = False
        log('nbns: NAME QUERY %s<%02X>%s -> %s' % (name, sfx, ' (broadcast)' if bcast else '',
                                                    'not found' if group is None else srv.ip))
        if group is None:
            if bcast:
                return None
            return (struct.pack('>HHHHHH', tid, 0x8483 | rd, 0, 1, 0, 0) + qraw
                    + struct.pack('>HHIH', 0x000a, 1, 0, 0))
        rdata = struct.pack('>H', 0x8000 if group else 0) + socket.inet_aton(srv.ip)
        return (struct.pack('>HHHHHH', tid, 0x8480 | rd, 0, 1, 0, 0) + qraw
                + struct.pack('>HHIH', 0x20, 1, 300000, len(rdata)) + rdata)
    if qtype == 0x21:
        if up not in ('*', srv.name.upper(), srv.ip):
            log('nbns: NODE STATUS for %s<%02X> ignored' % (name, sfx))
            return None
        names = [(srv.name, 0x00, 0), (srv.name, 0x20, 0), (srv.workgroup, 0x00, 0x8000), (srv.workgroup, 0x1e, 0x8000)]
        rdata = bytes([len(names)]) + b''.join(
            n.upper().encode('latin-1')[:15].ljust(15, b' ') + bytes([s]) + struct.pack('>H', g | 0x0400)
            for n, s, g in names) + srv.mac + b'\0' * 40
        log('nbns: NODE STATUS %s<%02X> -> %d names' % (name or '*', sfx, len(names)))
        return (struct.pack('>HHHHHH', tid, 0x8400, 0, 1, 0, 0) + qraw
                + struct.pack('>HHIH', 0x21, 1, 0, len(rdata)) + rdata)
    return None


def nbns_socket(bind, port):
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    try:
        s.bind((bind, port))
    except OSError as e:
        sys.exit('cannot listen on udp %s:%d: %s' % (bind, port, e))
    return s


def nbns_serve(srv, s):
    log('NBNS on udp %s:%d' % s.getsockname())
    while True:
        try:
            data, addr = s.recvfrom(2048)
            resp = nbns_handle(srv, data)
            if resp:
                s.sendto(resp, addr)
        except Exception as e:
            log('nbns: bad packet: %s' % e)


# ---------------------------------------------------------------- SMB1

SMB1_CMDS = {0x00: 'CREATE_DIRECTORY', 0x01: 'DELETE_DIRECTORY', 0x04: 'CLOSE', 0x05: 'FLUSH', 0x06: 'DELETE',
             0x07: 'RENAME', 0x08: 'QUERY_INFORMATION', 0x09: 'SET_INFORMATION', 0x10: 'CHECK_DIRECTORY',
             0x25: 'TRANSACTION', 0x26: 'TRANSACTION_SECONDARY', 0x2B: 'ECHO', 0x2E: 'READ_ANDX',
             0x2F: 'WRITE_ANDX', 0x32: 'TRANSACTION2', 0x33: 'TRANSACTION2_SECONDARY', 0x34: 'FIND_CLOSE2',
             0x71: 'TREE_DISCONNECT', 0x72: 'NEGOTIATE', 0x73: 'SESSION_SETUP_ANDX', 0x74: 'LOGOFF_ANDX',
             0x75: 'TREE_CONNECT_ANDX', 0x80: 'QUERY_INFORMATION_DISK', 0xA0: 'NT_TRANSACT',
             0xA2: 'NT_CREATE_ANDX', 0xA4: 'NT_CANCEL', 0xC0: 'OPEN_PRINT_FILE', 0xC1: 'WRITE_PRINT_FILE',
             0xC2: 'CLOSE_PRINT_FILE'}
ANDX_CMDS = {0x2E, 0x2F, 0x73, 0x74, 0x75, 0xA2}
SMB1_HDR = struct.Struct('<4sBIBHH8sHHHHH')
FLAGS2_LONG_NAMES, FLAGS2_EAS, FLAGS2_SIGNATURE = 0x1, 0x2, 0x4
FLAGS2_EXT_SEC, FLAGS2_NT_STATUS, FLAGS2_UNICODE = 0x800, 0x4000, 0x8000
CAP_UNICODE, CAP_LARGE_FILES, CAP_NT_SMBS, CAP_RPC_REMOTE_APIS = 0x4, 0x8, 0x10, 0x20
CAP_STATUS32, CAP_NT_FIND, CAP_LARGE_READX, CAP_LARGE_WRITEX = 0x40, 0x200, 0x4000, 0x8000
CAP_EXTENDED_SECURITY = 0x80000000

# NTSTATUS -> (DOS error class, code) for clients that did not ask for NT status codes
DOS_ERRORS = {
    SUCCESS: (0, 0), NO_SUCH_FILE: (1, 2), OBJECT_NAME_NOT_FOUND: (1, 2), OBJECT_PATH_NOT_FOUND: (1, 3),
    NOT_A_DIRECTORY: (1, 3), ACCESS_DENIED: (1, 5), CANNOT_DELETE: (1, 5), FILE_IS_A_DIRECTORY: (1, 5),
    DELETE_PENDING: (1, 5), INVALID_HANDLE: (1, 6), FILE_CLOSED: (1, 6), NO_MORE_FILES: (1, 18),
    SHARING_VIOLATION: (1, 32), END_OF_FILE: (1, 38), NOT_SUPPORTED: (1, 50), OBJECT_NAME_COLLISION: (1, 80),
    INVALID_PARAMETER: (1, 87), OBJECT_NAME_INVALID: (1, 123), INVALID_INFO_CLASS: (1, 124),
    DIRECTORY_NOT_EMPTY: (1, 145), PIPE_EMPTY: (1, 232), BUFFER_OVERFLOW: (1, 234),
    MORE_PROCESSING_REQUIRED: (1, 234), NO_EAS_ON_FILE: (1, 282), NOT_IMPLEMENTED: (1, 1),
    INVALID_DEVICE_REQUEST: (1, 1), INFO_LENGTH_MISMATCH: (1, 24), BUFFER_TOO_SMALL: (1, 111),
    LOGON_FAILURE: (2, 2), NETWORK_NAME_DELETED: (2, 5), BAD_NETWORK_NAME: (2, 6),
    USER_SESSION_DELETED: (2, 91), DISK_FULL: (3, 39), NOT_FOUND: (1, 2),
}


def dos_datetime(t):
    lt = time.localtime(t)
    if lt.tm_year < 1980:
        return 0x21, 0
    return (((lt.tm_year - 1980) << 9) | (lt.tm_mon << 5) | lt.tm_mday,
            (lt.tm_hour << 11) | (lt.tm_min << 5) | (lt.tm_sec // 2))


def from_dos_datetime(d, t):
    if not d and not t:
        return None
    return time.mktime(((d >> 9) + 1980, (d >> 5) & 15, d & 31, t >> 11, (t >> 5) & 63, (t & 31) * 2, 0, 0, -1))


def ft_to_unix(ft):
    return ft / 10000000 - EPOCH_DIFF


def fi_dos(fi):
    """(cdate, ctime, adate, atime, wdate, wtime) of a file's FILETIMEs."""
    out = ()
    for ft in (fi.ct, fi.at, fi.wt):
        out += dos_datetime(ft_to_unix(ft))
    return out


def fealist(eas):
    body = b''.join(struct.pack('<BBH', f, len(n), len(v)) + n + b'\0' + v for n, f, v in eas)
    return struct.pack('<I', 4 + len(body)) + body


def parse_fealist(data):
    eas = []
    end = min(u32(data, 0), len(data)) if len(data) >= 4 else 0
    off = 4
    while off + 4 <= end:
        f, nl, vl = struct.unpack_from('<BBH', data, off)
        eas.append((data[off + 4:off + 4 + nl].upper(), f, data[off + 5 + nl:off + 5 + nl + vl]))
        off += 5 + nl + vl
    return eas


def parse_gealist(data):
    names = []
    end = min(u32(data, 0), len(data)) if len(data) >= 4 else 0
    off = 4
    while off + 1 <= end:
        nl = data[off]
        names.append(data[off + 1:off + 1 + nl])
        off += 2 + nl
    return names


class SmbError(Exception):
    def __init__(self, status):
        self.status = status


class Blk:
    """Response bytes builder: the bytes field of an SMB1 block starts at an odd
    offset (blocks start at even offsets), which strings are aligned against."""
    def __init__(self, unicode):
        self.b = bytearray()
        self.unicode = unicode

    def string(self, s, oem=False):
        if self.unicode and not oem:
            if (1 + len(self.b)) % 2:
                self.b += b'\0'
            self.b += (s + '\0').encode('utf-16-le')
        else:
            self.b += (s + '\0').encode('cp850', 'replace')
        return self

    def raw(self, d):
        self.b += d
        return self


class Smb1:
    def __init__(self, conn):
        self.conn = conn
        self.srv = conn.srv
        self.challenge = os.urandom(8)
        self.ext = True
        self.negotiated = False
        self.sessions = {}
        self.pending = {}
        self.trees = {}
        self.fids = {}
        self.searches = {}
        self.trans = {}
        self.client_maxbuf = 16644
        self.sign_key = None
        self.seq = 0
        self.signing = False
        self.next_id = 1

    def new_id(self, used):
        while True:
            n = self.next_id
            self.next_id = self.next_id % 0xFFFE + 1
            if n not in used:
                return n

    # ------------------------------------------------------------ framing

    def handle(self, data):
        if len(data) < 35:
            log('%s smb1 short message' % self.conn.tag)
            return
        (_, cmd, _, flags, flags2, pidhigh, sig, _, tid, pid, uid, mid) = SMB1_HDR.unpack_from(data, 0)
        rq = Req()
        rq.data, rq.flags2, rq.tid, rq.pid, rq.pidhigh, rq.uid, rq.mid = data, flags2, tid, pid, pidhigh, uid, mid
        rq.unicode = bool(flags2 & FLAGS2_UNICODE)
        rq.nt = bool(flags2 & FLAGS2_NT_STATUS)
        # signatures
        seq = None
        if self.signing:
            seq = self.seq
            self.seq += 1 if cmd == 0xA4 else 2
            chk = bytearray(data)
            chk[14:22] = struct.pack('<Q', seq)
            good = hashlib.md5(self.sign_key + bytes(chk)).digest()[:8] == sig
            if not good and (flags2 & FLAGS2_SIGNATURE or self.srv.require_signing):
                log('%s smb1 %s: bad or missing signature' % (self.conn.tag, SMB1_CMDS.get(cmd, hex(cmd))))
                self.reply(rq, cmd, ACCESS_DENIED, [], seq)
                return
        if cmd == 0xA4:
            log('%s smb1 NT_CANCEL mid %d (nothing pending)' % (self.conn.tag, mid))
            return
        if cmd == 0x2B:
            self.echo(rq, seq)
            return
        blocks = []
        status = SUCCESS
        off = 32
        out_len = 32
        details = []
        while True:
            wc = data[off]
            words = data[off + 1:off + 1 + 2 * wc]
            bc = u16(data, off + 1 + 2 * wc)
            boff = off + 3 + 2 * wc
            rq.words, rq.bytes, rq.boff = words, data[boff:boff + bc], boff
            out_len += out_len % 2
            rq.S = out_len
            rq.detail = ''
            name = SMB1_CMDS.get(cmd, 'CMD_0x%02X' % cmd)
            try:
                r = self.dispatch(rq, cmd)
            except SmbError as e:
                r = (e.status, b'', b'')
            except (struct.error, IndexError, UnicodeDecodeError, ValueError) as e:
                log('%s smb1 %s: malformed request (%s)' % (self.conn.tag, name, e))
                r = (INVALID_PARAMETER, b'', b'')
            if r is None:
                return
            st, w, b = r
            details.append('%s%s' % (name, ' ' + rq.detail if rq.detail else ''))
            if is_error(st) and not (cmd == 0x73 and st == MORE_PROCESSING_REQUIRED):
                w, b = b'', b''
            blocks.append((cmd, w, b))
            out_len += 3 + len(w) + len(b)
            status = st
            if is_error(st) and not (cmd == 0x73 and st == MORE_PROCESSING_REQUIRED):
                break
            if cmd not in ANDX_CMDS or len(words) < 4 or words[0] == 0xFF:
                break
            cmd, off = words[0], u16(words, 2)
            if off < 32 or off >= len(data):
                break
        log('%s smb1 %s -> %s' % (self.conn.tag, ' + '.join(details), stname(status)))
        self.reply(rq, blocks[0][0], status, blocks, seq)

    def reply(self, rq, cmd, status, blocks, seq):
        out = bytearray(32)
        for i, (c, w, b) in enumerate(blocks):
            out += b'\0' * (len(out) % 2)
            start = len(out)
            w = bytearray(w)
            out += bytes([len(w) // 2]) + w + struct.pack('<H', len(b) & 0xFFFF) + b
            if c in ANDX_CMDS and len(w) >= 4:
                if i + 1 < len(blocks):
                    nxt = blocks[i + 1][0]
                    struct.pack_into('<BBH', out, start + 1, nxt, 0, len(out) + len(out) % 2)
                else:
                    struct.pack_into('<BBH', out, start + 1, 0xFF, 0, 0)
        if not blocks:
            out += b'\0\0\0'
        if rq.nt or status == MORE_PROCESSING_REQUIRED:
            st = struct.pack('<I', status)
        else:
            cls, code = DOS_ERRORS.get(status, (2, 1) if is_error(status) else (0, 0))
            st = struct.pack('<BBH', cls, 0, code)
        flags2 = (rq.flags2 & (FLAGS2_UNICODE | FLAGS2_NT_STATUS | FLAGS2_EXT_SEC)) | FLAGS2_LONG_NAMES | FLAGS2_EAS
        if self.signing:
            flags2 |= FLAGS2_SIGNATURE
        out[0:32] = SMB1_HDR.pack(b'\xffSMB', cmd, 0, 0x98, flags2, rq.pidhigh, b'\0' * 8, 0, rq.tid,
                                  rq.pid, rq.uid, rq.mid)
        out[5:9] = st
        if self.signing:
            out[14:22] = struct.pack('<Q', 1 if seq is None else seq + 1)
            out[14:22] = hashlib.md5(self.sign_key + bytes(out)).digest()[:8]
        self.conn.send(bytes(out))

    def echo(self, rq, seq):
        wc = rq.data[32]
        count = u16(rq.data, 33) if wc else 1
        bc = u16(rq.data, 33 + 2 * wc)
        payload = rq.data[35 + 2 * wc:35 + 2 * wc + bc]
        log('%s smb1 ECHO x%d -> STATUS_SUCCESS' % (self.conn.tag, count))
        for i in range(count):
            self.reply(rq, 0x2B, SUCCESS, [(0x2B, struct.pack('<H', i + 1), payload)], seq)

    # ------------------------------------------------------------ helpers

    def string(self, rq, off, aligned=True, unicode=None):
        """A string at absolute message offset off -> (str, next offset)."""
        d = rq.data
        uni = rq.unicode if unicode is None else unicode
        if uni:
            if aligned and off % 2:
                off += 1
            end = off
            while end + 1 < len(d) and (d[end] or d[end + 1]):
                end += 2
            return d[off:end].decode('utf-16-le', 'replace'), end + 2
        end = d.find(b'\0', off)
        if end < 0:
            end = len(d)
        return d[off:end].decode('cp850', 'replace'), end + 1

    def pstring(self, rq, buf, off):
        """A string inside a transaction parameter block (no alignment)."""
        if rq.unicode:
            end = off
            while end + 1 < len(buf) and (buf[end] or buf[end + 1]):
                end += 2
            return buf[off:end].decode('utf-16-le', 'replace')
        end = buf.find(b'\0', off)
        return buf[off:end if end >= 0 else len(buf)].decode('cp850', 'replace')

    def enc(self, rq, s):
        return s.encode('utf-16-le') if rq.unicode else s.encode('cp850', 'replace')

    def session(self, rq):
        s = self.sessions.get(rq.uid)
        if s is None:
            raise SmbError(USER_SESSION_DELETED)
        return s

    def tree(self, rq):
        self.session(rq)
        t = self.trees.get(rq.tid)
        if t is None:
            raise SmbError(NETWORK_NAME_DELETED)
        return t

    def disk_tree(self, rq):
        t = self.tree(rq)
        if t.share.kind != 'disk':
            raise SmbError(ACCESS_DENIED)
        return t

    def fid(self, rq, fid):
        op = self.fids.get(fid)
        if op is None:
            raise SmbError(INVALID_HANDLE)
        rq.detail = op.tree.share.name + op.rel
        return op

    def path_name(self, rq, off):
        """A buffer-format 0x04 path in the bytes at absolute offset off -> (name, next)."""
        if rq.data[off] == 4:
            off += 1
        return self.string(rq, off)

    def open_path(self, t, name, disp=OPEN, opts=0, access=0x80):
        st, op, action = fs_create(self.conn, t, name, disp, opts, access, 0)
        if st:
            raise SmbError(st)
        return op

    def add_fid(self, op):
        fid = self.new_id(self.fids)
        self.fids[fid] = op
        return fid

    def andx(self, rest=b''):
        return b'\xff\0\0\0' + rest

    # ------------------------------------------------------------ commands

    def dispatch(self, rq, cmd):
        h = getattr(self, 'c_%02x' % cmd, None)
        if h is None:
            if cmd != 0x72 and not self.negotiated:
                raise SmbError(INVALID_PARAMETER)
            return NOT_IMPLEMENTED, b'', b''
        if cmd != 0x72 and not self.negotiated:
            raise SmbError(INVALID_PARAMETER)
        return h(rq)

    def c_72(self, rq):  # NEGOTIATE
        dialects = []
        b = rq.bytes
        i = 0
        while i < len(b):
            if b[i] != 2:
                break
            end = b.find(b'\0', i + 1)
            if end < 0:
                break
            dialects.append(b[i + 1:end].decode('latin-1'))
            i = end + 1
        rq.detail = ', '.join(dialects)
        srv = self.srv
        if not srv.smb1_only and ('SMB 2.???' in dialects or 'SMB 2.002' in dialects):
            d = 0x02FF if 'SMB 2.???' in dialects and srv.max_dialect >= 0x0210 else 0x0202
            body = self.conn.negotiate_body(d)
            hdr = SMB2_HDR.pack(b'\xfeSMB', 64, 0, 0, 0, 1, FLAG_RESPONSE, 0, 0, 0, 0, 0, b'\0' * 16)
            log('%s smb1 NEGOTIATE %s -> SMB2 NEGOTIATE response dialect %04x' % (self.conn.tag, rq.detail, d))
            self.conn.send(hdr + body)
            self.conn.smb1_state = None
            return None
        if 'NT LM 0.12' not in dialects:
            return SUCCESS, struct.pack('<H', 0xFFFF), b''
        idx = dialects.index('NT LM 0.12')
        self.negotiated = True
        self.ext = bool(rq.flags2 & FLAGS2_EXT_SEC)
        secmode = 0x03 | 0x04 | (0x08 if srv.require_signing else 0)
        caps = (CAP_UNICODE | CAP_LARGE_FILES | CAP_NT_SMBS | CAP_RPC_REMOTE_APIS | CAP_STATUS32 | CAP_NT_FIND
                | CAP_LARGE_READX | CAP_LARGE_WRITEX | (CAP_EXTENDED_SECURITY if self.ext else 0))
        words = struct.pack('<HBHHIIIIQhB', idx, secmode, 50, 1, 65535, 65536, 0, caps, now_ft(), 0,
                            0 if self.ext else 8)
        if self.ext:
            b = srv.guid + spnego_init()
        else:
            b = self.challenge + (srv.workgroup + '\0').encode('utf-16-le' if rq.unicode else 'cp850') \
                + (srv.name + '\0').encode('utf-16-le' if rq.unicode else 'cp850')
        rq.detail += ' -> NT LM 0.12 (%s security)' % ('extended' if self.ext else 'challenge/response')
        return SUCCESS, words, b

    def c_73(self, rq):  # SESSION_SETUP_ANDX
        w = rq.words
        srv = self.srv
        self.client_maxbuf = u16(w, 4) or self.client_maxbuf
        if len(w) == 24:  # extended security
            bloblen = u16(w, 14)
            blob = rq.bytes[:bloblen]
            tok, spnego = extract_ntlm(blob)
            if tok is None or len(tok) < 12:
                return LOGON_FAILURE, b'', b''
            mtype = u32(tok, 8)
            if mtype == 1:
                uid = self.new_id(set(self.sessions) | set(self.pending))
                ntlm = NtlmServer(srv)
                self.pending[uid] = ntlm
                ntlm.mech_types = spnego_mech_types(blob) if spnego else None
                rq.uid = uid
                chal = ntlm.challenge_msg(tok)
                out = spnego_resp(1, chal) if spnego else chal
                rq.detail = 'NTLMSSP NEGOTIATE'
                return (MORE_PROCESSING_REQUIRED, self.andx(struct.pack('<HH', 0, len(out))),
                        self.ss_strings(rq, Blk(rq.unicode).raw(out)))
            if mtype != 3:
                return LOGON_FAILURE, b'', b''
            ntlm = self.pending.pop(rq.uid, None)
            if ntlm is None:
                return LOGON_FAILURE, b'', b''
            st, user, dom, key, kind = ntlm.authenticate(tok)
            rq.detail = 'NTLMSSP AUTHENTICATE %s\\%s' % (dom, user)
            if st:
                return st, b'', b''
            mac = key
            out = spnego_resp(0, mech=False, mic=ntlm.mic()) if spnego else b''
        else:  # LM/NT challenge-response in the password fields
            oemlen, unilen = u16(w, 14), u16(w, 16)
            lm = rq.bytes[:oemlen]
            nt = rq.bytes[oemlen:oemlen + unilen]
            off = rq.boff + oemlen + unilen
            user, off = self.string(rq, off)
            dom, off = self.string(rq, off)
            st, key, kind = check_password(srv, user, dom, self.challenge, lm, nt)
            rq.detail = 'password fields %s\\%s (LM %d, NT %d bytes)' % (dom, user, len(lm), len(nt))
            log('logon: user %r domain %r -> %s%s' % (user, dom, stname(st), ' (%s)' % kind if kind else ''))
            if st:
                return st, b'', b''
            mac = key + nt if key else None
            out = None
            rq.uid = self.new_id(set(self.sessions) | set(self.pending))
        sess = Session(rq.uid)
        sess.user, sess.domain, sess.kind, sess.valid = (user if kind == 'user' else (user or '')), dom, kind, True
        sess.key = key if kind == 'user' else None
        self.sessions[rq.uid] = sess
        if mac and kind == 'user' and not self.signing and (srv.require_signing or rq.flags2 & FLAGS2_SIGNATURE):
            self.sign_key = mac
            self.signing = True
            self.seq = 2
            rq.detail += ' (signing)'
        action = 1 if kind != 'user' else 0
        if out is None:
            return SUCCESS, self.andx(struct.pack('<H', action)), self.ss_strings(rq, Blk(rq.unicode))
        return SUCCESS, self.andx(struct.pack('<HH', action, len(out))), self.ss_strings(rq, Blk(rq.unicode).raw(out))

    def ss_strings(self, rq, blk):
        return bytes(blk.string('Unix').string('rs2 smbhost').string(self.srv.workgroup).b)

    def c_74(self, rq):  # LOGOFF_ANDX
        sess = self.session(rq)
        for k, op in list(self.fids.items()):
            if op.tree.sess is sess:
                op.close()
                del self.fids[k]
        for k, t in list(self.trees.items()):
            if t.sess is sess:
                del self.trees[k]
        del self.sessions[rq.uid]
        rq.detail = sess.user or ''
        return SUCCESS, self.andx(), b''

    def c_75(self, rq):  # TREE_CONNECT_ANDX
        sess = self.session(rq)
        flags, pwlen = u16(rq.words, 4), u16(rq.words, 6)
        path, off = self.string(rq, rq.boff + pwlen)
        service, _ = self.string(rq, off, unicode=False)
        rq.detail = '%s (%s)' % (path, service)
        share = self.srv.shares.get(path.rstrip('\\').rsplit('\\', 1)[-1].upper())
        if share is None:
            return BAD_NETWORK_NAME, b'', b''
        tid = self.new_id(self.trees)
        self.trees[tid] = Tree(tid, share, sess)
        rq.tid = tid
        svc = {'disk': 'A:', 'ipc': 'IPC', 'print': 'LPT1:'}[share.kind]
        words = struct.pack('<H', 0x0001)
        if flags & 0x8:
            words += struct.pack('<II', 0x001F01FF, 0x001F01FF)
        b = Blk(rq.unicode).string(svc, oem=True).string('NTFS' if share.kind == 'disk' else '')
        return SUCCESS, self.andx(words), bytes(b.b)

    def c_71(self, rq):  # TREE_DISCONNECT
        t = self.tree(rq)
        rq.detail = t.share.name
        for k, op in list(self.fids.items()):
            if op.tree is t:
                op.close()
                del self.fids[k]
        del self.trees[rq.tid]
        return SUCCESS, b'', b''

    def c_a2(self, rq):  # NT_CREATE_ANDX
        t = self.tree(rq)
        w = rq.words
        (_, nlen, flags, rootfid, access, alloc, fattrs, shacc, disp, opts, imp, secf) = struct.unpack_from(
            '<BHIIIQIIIIIB', w, 4)
        name, _ = self.string(rq, rq.boff)
        if rootfid:
            root = self.fid(rq, rootfid)
            name = root.rel.rstrip('\\') + '\\' + name
        rq.detail = '%s\\%s disp %d opts 0x%x access 0x%x' % (t.share.name, name.lstrip('\\'), disp, opts, access)
        st, op, action = fs_create(self.conn, t, name, disp, opts, access, fattrs)
        if st:
            return st, b'', b''
        fid = self.add_fid(op)
        fi = op.info()
        rtype = {'pipe': 2, 'print': 3}.get(op.kind, 0)
        words = struct.pack('<BHIQQQQIQQHHB', 0, fid, action, fi.ct, fi.at, fi.wt, fi.chg, fi.attrs, fi.alloc,
                            fi.eof, rtype, 0x05ff if op.kind == 'pipe' else 0, 1 if fi.isdir else 0)
        return SUCCESS, self.andx(words), b''

    def c_04(self, rq):  # CLOSE
        fid, wt = struct.unpack_from('<HI', rq.words)
        op = self.fid(rq, fid)
        if wt not in (0, 0xFFFFFFFF) and op.kind == 'file':
            try:
                os.utime(op.path, (os.stat(op.path).st_atime, wt))
            except OSError:
                pass
        op.close()
        del self.fids[fid]
        return SUCCESS, b'', b''

    def c_05(self, rq):  # FLUSH
        self.tree(rq)
        return SUCCESS, b'', b''

    def c_2e(self, rq):  # READ_ANDX
        self.tree(rq)
        w = rq.words
        fid, off, mx, mn, high = struct.unpack_from('<HIHHI', w, 4)
        if len(w) >= 24:
            off |= u32(w, 20) << 32
        op = self.fid(rq, fid)
        if high != 0xFFFFFFFF and op.kind != 'pipe':
            mx |= (high & 0xFFFF) << 16
        rq.detail += ' %d@%d' % (mx, off)
        st = SUCCESS
        if op.kind == 'pipe':
            data, st = op.pipe.read(mx)
            if st == PIPE_EMPTY:
                return st, b'', b''
        elif op.fd is None:
            return INVALID_DEVICE_REQUEST, b'', b''
        else:
            data = os.pread(op.fd, mx, off)
        doff = rq.S + 1 + 24 + 2 + 1
        words = struct.pack('<HHHHHH8s', 0xFFFF if op.kind == 'pipe' else 0, 0, 0, len(data) & 0xFFFF, doff,
                            len(data) >> 16, b'')
        return st, self.andx(words), b'\0' + data

    def c_2f(self, rq):  # WRITE_ANDX
        self.tree(rq)
        w = rq.words
        fid, off, _, mode, _, lhigh, ln, doff = struct.unpack_from('<HIIHHHHH', w, 4)
        if len(w) >= 28:
            off |= u32(w, 24) << 32
        ln |= lhigh << 16
        data = rq.data[doff:doff + ln]
        op = self.fid(rq, fid)
        rq.detail += ' %d@%d' % (ln, off)
        if op.kind == 'pipe':
            if mode & 0x0008 and mode & 0x0004:
                data = data[2:]
            op.pipe.write(data)
            n = ln
        elif op.fd is None:
            return INVALID_DEVICE_REQUEST, b'', b''
        elif not op.writable:
            return ACCESS_DENIED, b'', b''
        else:
            try:
                n = os.pwrite(op.fd, data, off)
            except OSError as e:
                return os_status(e), b'', b''
            op.written += n
        return SUCCESS, self.andx(struct.pack('<HHHH', n & 0xFFFF, 0xFFFF, n >> 16, 0)), b''

    def c_c0(self, rq):  # OPEN_PRINT_FILE
        t = self.tree(rq)
        if t.share.kind != 'print':
            return INVALID_DEVICE_REQUEST, b'', b''
        ident, _ = self.path_name(rq, rq.boff)
        rq.detail = '%s %r' % (t.share.name, ident)
        op = self.open_path(t, ident or 'job', CREATE)
        return SUCCESS, struct.pack('<H', self.add_fid(op)), b''

    def c_c1(self, rq):  # WRITE_PRINT_FILE
        op = self.fid(rq, u16(rq.words, 0))
        b = rq.bytes
        ln = u16(b, 1)
        os.write(op.fd, b[3:3 + ln])
        op.written += ln
        return SUCCESS, b'', b''

    def c_c2(self, rq):  # CLOSE_PRINT_FILE
        fid = u16(rq.words, 0)
        op = self.fid(rq, fid)
        op.close()
        del self.fids[fid]
        return SUCCESS, b'', b''

    def c_00(self, rq):  # CREATE_DIRECTORY
        t = self.disk_tree(rq)
        name, _ = self.path_name(rq, rq.boff)
        rq.detail = t.share.name + '\\' + name.lstrip('\\')
        self.open_path(t, name, CREATE, FILE_DIRECTORY_FILE).close()
        return SUCCESS, b'', b''

    def c_01(self, rq):  # DELETE_DIRECTORY
        t = self.disk_tree(rq)
        name, _ = self.path_name(rq, rq.boff)
        rq.detail = t.share.name + '\\' + name.lstrip('\\')
        op = self.open_path(t, name, OPEN, FILE_DIRECTORY_FILE, DELETE_ACCESS)
        st = set_disposition(op, True)
        op.close()
        return st, b'', b''

    def c_10(self, rq):  # CHECK_DIRECTORY
        t = self.disk_tree(rq)
        name, _ = self.path_name(rq, rq.boff)
        rq.detail = t.share.name + '\\' + name.lstrip('\\')
        st, path, exists, rel = resolve(t.share.path, name)
        if st:
            return st, b'', b''
        if not exists:
            return OBJECT_PATH_NOT_FOUND, b'', b''
        if not os.path.isdir(path):
            return NOT_A_DIRECTORY, b'', b''
        return SUCCESS, b'', b''

    def matching(self, t, name):
        """Files matched by a (possibly wildcard) path -> [(Open-less) host paths]."""
        d, _, pat = name.replace('/', '\\').rpartition('\\')
        if not set(pat) & WILDCARDS:
            return None
        op = self.open_path(t, d, OPEN, FILE_DIRECTORY_FILE)
        try:
            return [n for n, p in dir_entries(op, pat) if n not in ('.', '..')], d
        finally:
            op.close()

    def c_06(self, rq):  # DELETE
        t = self.disk_tree(rq)
        sattr = u16(rq.words, 0)
        name, _ = self.path_name(rq, rq.boff)
        rq.detail = t.share.name + '\\' + name.lstrip('\\')
        m = self.matching(t, name)
        names = [name] if m is None else [m[1] + '\\' + n for n in m[0]]
        if not names:
            return NO_SUCH_FILE, b'', b''
        for n in names:
            op = self.open_path(t, n, OPEN, 0, DELETE_ACCESS)
            if op.isdir:
                op.close()
                if m is None:
                    return FILE_IS_A_DIRECTORY, b'', b''
                continue
            fi = op.info()
            if fi.attrs & (ATTR_HIDDEN | ATTR_SYSTEM) & ~sattr and m is not None:
                op.close()
                continue
            st = set_disposition(op, True)
            op.close()
            if st:
                return st, b'', b''
        return SUCCESS, b'', b''

    def c_07(self, rq):  # RENAME
        t = self.disk_tree(rq)
        old, off = self.path_name(rq, rq.boff)
        new, _ = self.path_name(rq, off)
        rq.detail = '%s\\%s -> %s' % (t.share.name, old.lstrip('\\'), new)
        if set(old) & WILDCARDS or set(new) & WILDCARDS:
            return NOT_SUPPORTED, b'', b''
        op = self.open_path(t, old, OPEN, 0, DELETE_ACCESS)
        st = do_rename(op, new, False)
        op.close()
        return st, b'', b''

    def c_08(self, rq):  # QUERY_INFORMATION
        t = self.disk_tree(rq)
        name, _ = self.path_name(rq, rq.boff)
        rq.detail = t.share.name + '\\' + name.lstrip('\\')
        op = self.open_path(t, name)
        fi = op.info()
        op.close()
        return SUCCESS, struct.pack('<HII10s', fi.attrs & 0x3f, int(ft_to_unix(fi.wt)), min(fi.eof, 0xFFFFFFFF), b''), b''

    def c_09(self, rq):  # SET_INFORMATION
        t = self.disk_tree(rq)
        attrs, wt = struct.unpack_from('<HI', rq.words)
        name, _ = self.path_name(rq, rq.boff)
        rq.detail = t.share.name + '\\' + name.lstrip('\\')
        op = self.open_path(t, name, OPEN, 0, 0x100)
        st = set_basic(op, 0, 0, filetime(wt) if wt not in (0, 0xFFFFFFFF) else 0, 0, attrs or ATTR_NORMAL)
        op.close()
        return st, b'', b''

    def c_80(self, rq):  # QUERY_INFORMATION_DISK
        t = self.tree(rq)
        st, d = query_fs_info(t.share, 3)
        total, free, spu, bps = struct.unpack('<QQII', d)
        total *= spu
        free *= spu
        bpu = 1
        while total // bpu > 0xFFFF and bpu < 0x8000:
            bpu *= 2
        return SUCCESS, struct.pack('<HHHHH', min(total // bpu, 0xFFFF), bpu, bps, min(free // bpu, 0xFFFF), 0), b''

    # ------------------------------------------------------------ transactions

    def trans_collect(self, rq, cmd):
        """Collects a TRANSACTION/TRANSACTION2 (and its secondaries) -> dict or None (interim)."""
        w = rq.words
        if cmd in (0x25, 0x32):
            tpc, tdc, mpc, mdc, msc, _, flags, timeout, _, pc, po, dc, do, sc = struct.unpack_from(
                '<HHHHBBHIHHHHHB', w)
            setup = [u16(w, 28 + 2 * i) for i in range(sc)]
            t = dict(cmd=cmd, setup=setup, mpc=mpc, mdc=mdc, tpc=tpc, tdc=tdc,
                     params=bytearray(tpc), data=bytearray(tdc), gotp=0, gotd=0, name='')
            if cmd == 0x25:
                t['name'], _ = self.string(rq, rq.boff)
            self.trans[rq.mid] = t
            pd, dd = 0, 0
        else:
            t = self.trans.get(rq.mid)
            if t is None:
                raise SmbError(INVALID_PARAMETER)
            tpc, tdc, pc, po, pd, dc, do, dd = struct.unpack_from('<HHHHHHHH', w)
        t['params'][pd:pd + pc] = rq.data[po:po + pc]
        t['data'][dd:dd + dc] = rq.data[do:do + dc]
        t['gotp'] += pc
        t['gotd'] += dc
        if t['gotp'] < t['tpc'] or t['gotd'] < t['tdc']:
            return None
        del self.trans[rq.mid]
        t['params'], t['data'] = bytes(t['params']), bytes(t['data'])
        return t

    def trans_reply(self, rq, params, data, setup=b''):
        words_len = 20 + len(setup)
        bstart = rq.S + 1 + words_len + 2
        pad1 = (-bstart) % 4
        poff = bstart + pad1
        doff = poff + len(params)
        pad2 = (-doff) % 4
        doff += pad2
        words = struct.pack('<HHHHHHHHHBB', len(params), len(data), 0, len(params), poff if params else 0, 0,
                            len(data), doff if data else 0, 0, len(setup) // 2, 0) + setup
        return words, b'\0' * pad1 + params + b'\0' * pad2 + data

    def c_25(self, rq):  # TRANSACTION
        self.tree(rq)
        t = self.trans_collect(rq, 0x25)
        if t is None:
            return SUCCESS, b'', b''
        return self.transaction(rq, t)

    def c_26(self, rq):
        t = self.trans_collect(rq, 0x26)
        if t is None:
            return None
        rq.cmd_override = t['cmd']
        return self.transaction(rq, t)

    def c_32(self, rq):  # TRANSACTION2
        self.tree(rq)
        t = self.trans_collect(rq, 0x32)
        if t is None:
            return SUCCESS, b'', b''
        return self.trans2(rq, t)

    def c_33(self, rq):
        t = self.trans_collect(rq, 0x33)
        if t is None:
            return None
        return self.trans2(rq, t)

    def transaction(self, rq, t):
        name = t['name'].upper()
        setup = t['setup']
        rq.detail = name
        if name == '\\PIPE\\LANMAN':
            st, p, d = self.rap(rq, t['params'], t['data'], t['mdc'])
            w, b = self.trans_reply(rq, p, d)
            return st, w, b
        if not name.startswith('\\PIPE\\') or not setup:
            return NOT_SUPPORTED, b'', b''
        sub = setup[0]
        if sub == 0x26:  # TransactNmPipe
            op = self.fid(rq, setup[1])
            if op.kind != 'pipe':
                return INVALID_DEVICE_REQUEST, b'', b''
            rq.detail += ' TransactNmPipe ' + op.rel
            op.pipe.write(t['data'])
            out, st = op.pipe.read(t['mdc'])
            if st == PIPE_EMPTY:
                out, st = b'', SUCCESS
            w, b = self.trans_reply(rq, b'', out)
            return st, w, b
        if sub == 0x23:  # PeekNmPipe
            op = self.fid(rq, setup[1])
            avail = len(op.pipe.out[0]) if op.kind == 'pipe' and op.pipe.out else 0
            w, b = self.trans_reply(rq, struct.pack('<HHH', avail, avail, 3), b'')
            return SUCCESS, w, b
        if sub in (0x53, 0x01, 0x21, 0x22):  # WaitNmPipe, SetNmPHandState, Query*
            w, b = self.trans_reply(rq, struct.pack('<H', 0x05ff) if sub == 0x21 else b'', b'')
            return SUCCESS, w, b
        return NOT_SUPPORTED, b'', b''

    # RAP on \PIPE\LANMAN
    def rap(self, rq, p, d, maxdata):
        srv = self.srv
        api = u16(p, 0)
        e1 = p.find(b'\0', 2)
        pdesc = p[2:e1].decode('latin-1')
        e2 = p.find(b'\0', e1 + 1)
        ddesc = p[e1 + 1:e2].decode('latin-1')
        args = p[e2 + 1:]
        rq.detail += ' RAP %d %s %s' % (api, pdesc, ddesc)

        def strings_area(recs, _unused, bufsize):
            """recs: [[bytes or str, ...]] (a str is a pointer to that string) -> (data, count)."""
            n = fixed = strs = 0
            for rec in recs:
                f = sum(4 if isinstance(x, str) else len(x) for x in rec)
                sz = sum(len(x.encode('cp850', 'replace')[:255]) + 1 for x in rec if isinstance(x, str))
                if fixed + f + strs + sz > bufsize:
                    break
                fixed += f
                strs += sz
                n += 1
            out = bytearray()
            heap = bytearray()
            for rec in recs[:n]:
                for x in rec:
                    if isinstance(x, str):
                        out += struct.pack('<I', fixed + len(heap))
                        heap += x.encode('cp850', 'replace')[:255] + b'\0'
                    else:
                        out += x
            return bytes(out + heap), n

        if api == 0:  # NetShareEnum
            level, bufsize = struct.unpack_from('<HH', args)
            shares = [s for s in srv.shares.values() if len(s.name) <= 12]
            if level == 0:
                recs = [[s.name.encode('cp850')[:12].ljust(13, b'\0')] for s in shares]
            elif level == 1:
                recs = [[s.name.encode('cp850')[:12].ljust(14, b'\0') + struct.pack('<H', s.stype & 0xFFFF),
                         s.remark] for s in shares]
            else:
                return SUCCESS, struct.pack('<HHHH', 124, 0, 0, 0), b''
            data, n = strings_area(recs, recs, min(bufsize, maxdata))
            log('  rap: NetShareEnum level %d: %d of %d shares' % (level, n, len(shares)))
            return SUCCESS, struct.pack('<HHHH', 234 if n < len(shares) else 0, 0, n, len(shares)), data
        if api == 104:  # NetServerEnum2
            level, bufsize, stype = struct.unpack_from('<HHI', args)
            dom = args[8:args.find(b'\0', 8)].decode('latin-1') if len(args) > 8 else ''
            domains = stype != 0xFFFFFFFF and stype & 0x80000000
            if domains:
                ents = [(srv.workgroup, 0x80001000, srv.name)]
            elif stype == 0xFFFFFFFF or stype & srv.server_type:
                ents = [(srv.name, srv.server_type, srv.comment)]
            else:
                ents = []
            if dom and dom.upper() != srv.workgroup.upper() and not domains:
                ents = []
            if level == 0:
                recs = [[n.encode('cp850')[:15].ljust(16, b'\0')] for n, t, c in ents]
            elif level == 1:
                recs = [[n.encode('cp850')[:15].ljust(16, b'\0') + struct.pack('<BBI', 10, 0, t), c]
                        for n, t, c in ents]
            else:
                return SUCCESS, struct.pack('<HHHH', 124, 0, 0, 0), b''
            data, n = strings_area(recs, recs, min(bufsize, maxdata))
            log('  rap: NetServerEnum2 level %d type 0x%x domain %r: %d entries' % (level, stype, dom, n))
            return SUCCESS, struct.pack('<HHHH', 234 if n < len(ents) else 0, 0, n, len(ents)), data
        if api == 13:  # NetServerGetInfo
            level, bufsize = struct.unpack_from('<HH', args)
            if level == 0:
                recs = [[srv.name.encode('cp850')[:15].ljust(16, b'\0')]]
            elif level == 1:
                recs = [[srv.name.encode('cp850')[:15].ljust(16, b'\0') + struct.pack('<BBI', 10, 0, srv.server_type),
                         srv.comment]]
            else:
                return SUCCESS, struct.pack('<HHH', 124, 0, 0), b''
            data, n = strings_area(recs, recs, min(bufsize, maxdata))
            return SUCCESS, struct.pack('<HHH', 0 if n else 2123, 0, len(data)), data
        if api == 63:  # NetWkstaGetInfo
            level, bufsize = struct.unpack_from('<HH', args)
            if level != 10:
                return SUCCESS, struct.pack('<HHH', 124, 0, 0), b''
            sess = self.sessions.get(rq.uid)
            recs = [[srv.name, sess.user if sess and sess.user else '', srv.workgroup, struct.pack('<BB', 10, 0),
                     srv.workgroup, '']]
            data, n = strings_area(recs, recs, min(bufsize, maxdata))
            return SUCCESS, struct.pack('<HHH', 0 if n else 2123, 0, len(data)), data
        log('  rap: API %d not supported' % api)
        return SUCCESS, struct.pack('<HH', 50, 0), b''

    def trans2(self, rq, t):
        sub = t['setup'][0] if t['setup'] else -1
        p, d = t['params'], t['data']
        names = {1: 'FIND_FIRST2', 2: 'FIND_NEXT2', 3: 'QUERY_FS_INFORMATION', 5: 'QUERY_PATH_INFORMATION',
                 6: 'SET_PATH_INFORMATION', 7: 'QUERY_FILE_INFORMATION', 8: 'SET_FILE_INFORMATION',
                 0xd: 'CREATE_DIRECTORY'}
        rq.detail = names.get(sub, 'subcommand 0x%x' % sub)
        h = getattr(self, 't2_%x' % sub, None) if sub in names else None
        if h is None:
            return NOT_IMPLEMENTED, b'', b''
        st, rp, rd = h(rq, t, p, d)
        if is_error(st):
            return st, b'', b''
        w, b = self.trans_reply(rq, rp, rd)
        return st, w, b

    FIND_LEVELS = {0x101: 1, 0x102: 2, 0x103: 12, 0x104: 3, 0x105: 38, 0x106: 37}
    NAME_OFFSETS = {1: 64, 2: 68, 3: 94, 12: 12, 37: 104, 38: 80}

    def find_pack(self, rq, s, count, maxdata, flags):
        """Packs search s from s['pos'] -> (data, n, last name offset, eos)."""
        level = s['level']
        op = s['op']
        entries = s['entries']
        out = bytearray()
        last = None
        lastname = 0
        n = 0
        used = set()
        pos = s['pos']
        while pos < len(entries) and n < count:
            name, path = entries[pos]
            try:
                fi = file_info(path, op.root)
            except OSError:
                pos += 1
                continue
            if fi.attrs & (ATTR_HIDDEN | ATTR_SYSTEM | ATTR_DIRECTORY) & ~s['sattr']:
                pos += 1
                continue
            if level in (1, 2, 3):
                nb = self.enc(rq, name)
                e = struct.pack('<I', pos + 1) if flags & 4 else b''
                e += struct.pack('<6HII', *fi_dos(fi), min(fi.eof, 0xFFFFFFFF), min(fi.alloc, 0xFFFFFFFF))
                e += struct.pack('<H', fi.attrs & 0x37)
                if level == 2:
                    e += struct.pack('<I', fi.easize)
                nameoff = len(e) + 1
                e += bytes([len(nb)]) + nb + (b'\0\0' if rq.unicode else b'\0')
                if len(out) + len(e) > maxdata:
                    break
                lastname = len(out) + nameoff
                out += e
            else:
                cls = self.FIND_LEVELS[level]
                e = encode_dir_entry(cls, name, fi, pos + 1, rq.unicode, short_name(name, used))
                pad = (-len(out)) % 8 if last is not None else 0
                if len(out) + pad + len(e) > maxdata:
                    break
                if last is not None:
                    out += b'\0' * pad
                    struct.pack_into('<I', out, last, len(out) - last)
                last = len(out)
                lastname = last + self.NAME_OFFSETS[cls]
                out += e
            n += 1
            pos += 1
        s['pos'] = pos
        return bytes(out), n, lastname, pos >= len(entries)

    def find_limit(self, mdc):
        return max(min(mdc, self.client_maxbuf - 120), 512)

    def t2_1(self, rq, t, p, d):  # FIND_FIRST2
        tr = self.disk_tree(rq)
        sattr, count, flags, level, _ = struct.unpack_from('<HHHHI', p)
        name = self.pstring(rq, p, 12)
        rq.detail += ' %s\\%s level 0x%x' % (tr.share.name, name.lstrip('\\'), level)
        if level not in self.FIND_LEVELS and level not in (1, 2):
            return INVALID_INFO_CLASS, b'', b''
        dname, _, pat = name.rpartition('\\')
        try:
            op = self.open_path(tr, dname, OPEN, FILE_DIRECTORY_FILE)
        except SmbError as e:
            if e.status in (OBJECT_NAME_NOT_FOUND, NOT_A_DIRECTORY):
                raise SmbError(OBJECT_PATH_NOT_FOUND)
            raise
        entries = dir_entries(op, pat or '*')
        if not entries:
            return NO_SUCH_FILE, b'', b''
        sid = self.new_id(self.searches)
        s = dict(op=op, entries=entries, pos=0, level=level, sattr=sattr | 0x80 | 0x20 | 0x01)
        data, n, lastname, eos = self.find_pack(rq, s, count or 0xFFFF, self.find_limit(t['mdc']), flags)
        if n == 0:
            return NO_SUCH_FILE, b'', b''
        if not (flags & 1 or (flags & 2 and eos)):
            self.searches[sid] = s
        rq.detail += ' sid %d: %d entries%s' % (sid, n, ' (end)' if eos else '')
        return SUCCESS, struct.pack('<HHHHH', sid, n, 1 if eos else 0, 0, lastname), data

    def t2_2(self, rq, t, p, d):  # FIND_NEXT2
        self.disk_tree(rq)
        sid, count, level, rkey, flags = struct.unpack_from('<HHHIH', p)
        name = self.pstring(rq, p, 12)
        s = self.searches.get(sid)
        rq.detail += ' sid %d' % sid
        if s is None:
            return INVALID_HANDLE, b'', b''
        s['level'] = level if level in self.FIND_LEVELS or level in (1, 2) else s['level']
        if not flags & 8 and name:
            for i, (n, _) in enumerate(s['entries']):
                if n == name:
                    s['pos'] = i + 1
                    break
        data, n, lastname, eos = self.find_pack(rq, s, count or 0xFFFF, self.find_limit(t['mdc']), flags)
        if flags & 1 or (flags & 2 and eos):
            del self.searches[sid]
        rq.detail += ': %d entries%s' % (n, ' (end)' if eos else '')
        return SUCCESS, struct.pack('<HHHH', n, 1 if eos else 0, 0, lastname), data

    def c_34(self, rq):  # FIND_CLOSE2
        sid = u16(rq.words, 0)
        rq.detail = 'sid %d' % sid
        if self.searches.pop(sid, None) is None:
            return INVALID_HANDLE, b'', b''
        return SUCCESS, b'', b''

    def t2_3(self, rq, t, p, d):  # QUERY_FS_INFORMATION
        tr = self.tree(rq)
        level = u16(p, 0)
        rq.detail += ' level 0x%x' % level
        if level == 1:
            st, x = query_fs_info(tr.share, 3)
            total, free, spu, bps = struct.unpack('<QQII', x)
            while total > 0xFFFFFFFF:
                total //= 2
                free //= 2
                spu *= 2
            return SUCCESS, b'', struct.pack('<IIIIH', 0, spu, total, free, bps)
        if level == 2:
            label = self.enc(rq, tr.share.name)
            return SUCCESS, b'', struct.pack('<IB', zlib.crc32(tr.share.name.encode()) & 0xffffffff,
                                             len(label)) + label
        cls = {0x102: 1, 0x103: 3, 0x104: 4, 0x105: 5}.get(level, level - 1000 if level >= 1000 else None)
        if cls is None:
            return INVALID_INFO_CLASS, b'', b''
        st, x = query_fs_info(tr.share, cls)
        if level == 0x102:
            x = x[:16] + b'\0\0' + x[18:]
        return st, b'', x

    def query_info_level(self, rq, op, level, d):
        try:
            fi = op.info()
        except OSError as e:
            return os_status(e), b''
        if level in (1, 2):
            x = struct.pack('<6HII', *fi_dos(fi), min(fi.eof, 0xFFFFFFFF), min(fi.alloc, 0xFFFFFFFF))
            x += struct.pack('<H', fi.attrs & 0x37)
            if level == 2:
                x += struct.pack('<I', fi.easize)
            return SUCCESS, x
        if level == 3:
            want = parse_gealist(d)
            have = {n.upper(): (n, f, v) for n, f, v in fi.eas}
            return SUCCESS, fealist([have.get(w.upper(), (w.upper(), 0, b'')) for w in want])
        if level == 4:
            return SUCCESS, fealist(fi.eas)
        if level == 6:
            return SUCCESS, b''
        if level == 0x101:
            return query_file_info(op, 4, self.conn)
        if level == 0x102:
            return query_file_info(op, 5, self.conn)
        if level == 0x103:
            return SUCCESS, struct.pack('<I', fi.easize)
        if level == 0x104:
            nb = self.enc(rq, op.rel or '\\')
            return SUCCESS, struct.pack('<I', len(nb)) + nb
        if level == 0x107:
            nb = self.enc(rq, op.rel or '\\')
            return SUCCESS, (struct.pack('<QQQQII', fi.ct, fi.at, fi.wt, fi.chg, fi.attrs, 0)
                             + struct.pack('<QQIBBHI', fi.alloc, fi.eof, fi.nlink, 1 if op.delete_pending else 0,
                                           1 if fi.isdir else 0, 0, fi.easize)
                             + struct.pack('<I', len(nb)) + nb)
        if level == 0x108:
            sn = self.enc(rq, short_name(os.path.basename(op.path or ''), set()) or os.path.basename(op.path or ''))
            return SUCCESS, struct.pack('<I', len(sn)) + sn
        if level == 0x109:
            return query_file_info(op, 22, self.conn)
        if level == 0x10B:
            return query_file_info(op, 28, self.conn)
        if level >= 1000:
            if level - 1000 == 15:
                return query_full_eas(op, d)
            return query_file_info(op, level - 1000, self.conn)
        return INVALID_INFO_CLASS, b''

    def t2_5(self, rq, t, p, d):  # QUERY_PATH_INFORMATION
        tr = self.tree(rq)
        level = u16(p, 0)
        name = self.pstring(rq, p, 6)
        rq.detail += ' %s\\%s level 0x%x' % (tr.share.name, name.lstrip('\\'), level)
        op = self.open_path(tr, name)
        try:
            st, x = self.query_info_level(rq, op, level, d)
        finally:
            op.close()
        if is_error(st):
            return st, b'', b''
        return st, struct.pack('<H', 0), x

    def t2_7(self, rq, t, p, d):  # QUERY_FILE_INFORMATION
        self.tree(rq)
        fid, level = struct.unpack_from('<HH', p)
        op = self.fid(rq, fid)
        rq.detail = 'QUERY_FILE_INFORMATION %s level 0x%x' % (rq.detail, level)
        st, x = self.query_info_level(rq, op, level, d)
        if is_error(st):
            return st, b'', b''
        return st, struct.pack('<H', 0), x

    def set_info_level(self, rq, op, level, d):
        if level == 1:
            cd, ct, ad, at, wd, wt = struct.unpack_from('<6H', d)
            a, w = from_dos_datetime(ad, at), from_dos_datetime(wd, wt)
            return set_basic(op, 0, filetime(a) if a else 0, filetime(w) if w else 0, 0, 0)
        if level == 2:
            if op.kind not in ('file', 'dir'):
                return ACCESS_DENIED
            eas = {n.upper(): (n, f, v) for n, f, v in get_eas(op.path, op.root)}
            for n, f, v in parse_fealist(d):
                if v:
                    eas[n.upper()] = (n.upper(), f, v)
                else:
                    eas.pop(n.upper(), None)
            set_eas(op.path, op.root, list(eas.values()))
            return SUCCESS
        if level == 0x101:
            return set_file_info(op, 4, d)
        if level == 0x102:
            return set_disposition(op, d[0] & 1)
        if level == 0x103:
            return set_file_info(op, 19, d)
        if level == 0x104:
            return set_eof(op, u64(d, 0))
        if level == 1010:
            replace, nl = d[0], u32(d, 8)
            target = d[12:12 + nl].decode('utf-16-le' if rq.unicode else 'cp850')
            return do_rename(op, target, replace, share_relative=False)
        if level >= 1000:
            return set_file_info(op, level - 1000, d, share_relative=False)
        return INVALID_INFO_CLASS

    def t2_6(self, rq, t, p, d):  # SET_PATH_INFORMATION
        tr = self.disk_tree(rq)
        level = u16(p, 0)
        name = self.pstring(rq, p, 6)
        rq.detail += ' %s\\%s level 0x%x' % (tr.share.name, name.lstrip('\\'), level)
        op = self.open_path(tr, name, OPEN, 0, 0x100 | 0x10 | DELETE_ACCESS)
        try:
            st = self.set_info_level(rq, op, level, d)
        finally:
            op.close()
        return st, struct.pack('<H', 0), b''

    def t2_8(self, rq, t, p, d):  # SET_FILE_INFORMATION
        self.tree(rq)
        fid, level = struct.unpack_from('<HH', p)
        op = self.fid(rq, fid)
        rq.detail = 'SET_FILE_INFORMATION %s level 0x%x' % (rq.detail, level)
        st = self.set_info_level(rq, op, level, d)
        return st, struct.pack('<H', 0), b''

    def t2_d(self, rq, t, p, d):  # CREATE_DIRECTORY (with EAs)
        tr = self.disk_tree(rq)
        name = self.pstring(rq, p, 4)
        rq.detail += ' %s\\%s' % (tr.share.name, name.lstrip('\\'))
        op = self.open_path(tr, name, CREATE, FILE_DIRECTORY_FILE)
        if len(d) > 4:
            self.set_info_level(rq, op, 2, d)
        op.close()
        return SUCCESS, struct.pack('<H', 0), b''

    def c_a0(self, rq):  # NT_TRANSACT
        return NOT_SUPPORTED, b'', b''


# ---------------------------------------------------------------- server

class Server:
    def __init__(self, a):
        self.name = a.name.upper()
        self.workgroup = a.workgroup.upper()
        self.comment = a.comment
        self.ip = a.ip
        self.mac = bytes(int(x, 16) for x in a.mac.split(':'))
        self.guest = a.guest
        self.require_signing = a.require_signing
        self.smb1_only = a.smb1
        self.max_dialect = int(a.max_dialect, 16)
        self.max_io = a.max_io
        self.guid = uuid.uuid5(uuid.NAMESPACE_DNS, self.name).bytes_le
        self.start_ft = now_ft()
        self.lock = threading.Lock()
        self.jobno = 0
        self.server_type = 0x1 | 0x2 | 0x200 | 0x1000
        self.users = {}
        for u in a.user or []:
            n, _, p = u.partition(':')
            self.users[n.lower()] = p
        comments = dict(c.split('=', 1) for c in a.share_comment or [])
        comments = {k.upper(): v for k, v in comments.items()}
        self.shares = collections.OrderedDict()
        for kind, specs in (('disk', a.share or []), ('print', a.printer or [])):
            for spec in specs:
                n, _, d = spec.partition('=')
                if not d:
                    sys.exit('bad share spec %r (NAME=DIR)' % spec)
                d = os.path.abspath(d)
                os.makedirs(d, exist_ok=True)
                self.shares[n.upper()] = Share(n.upper(), kind, d, comments.get(n.upper(), 'Printer' if kind == 'print' else ''))
        self.shares['IPC$'] = Share('IPC$', 'ipc', None, 'Remote IPC')
        for sh in self.shares.values():
            if sh.kind == 'print':
                for n in os.listdir(sh.path):
                    m = re.match(r'job(\d+)-', n)
                    if m:
                        self.jobno = max(self.jobno, int(m.group(1)))


def tcp_socket(bind, port):
    s = socket.socket()
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    try:
        s.bind((bind, port))
    except OSError as e:
        sys.exit('cannot listen on tcp %s:%d: %s' % (bind, port, e))
    s.listen(16)
    return s


def listen(srv, s, nb):
    log('SMB on tcp %s:%d (%s)' % (s.getsockname()[0], s.getsockname()[1], 'NetBIOS session service' if nb else 'direct'))
    while True:
        c, addr = s.accept()
        c.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        threading.Thread(target=Conn(srv, c, addr, nb).run, daemon=True).start()


def main():
    ap = argparse.ArgumentParser(description='SMB test server for rs2 (standard library only)')
    ap.add_argument('--port', type=int, default=4445, help='direct-TCP SMB port (0: off)')
    ap.add_argument('--nbport', type=int, default=4139, help='NetBIOS session service port (0: off)')
    ap.add_argument('--nbns-port', type=int, default=4137, help='NBNS UDP port (0: off)')
    ap.add_argument('--bind', default='127.0.0.1', help='address to listen on')
    ap.add_argument('--ip', default='127.0.0.1', help='address NBNS answers with')
    ap.add_argument('--mac', default='52:54:00:12:34:56', help='MAC address in NODE STATUS answers')
    ap.add_argument('--name', default='SMBHOST', help='server (NetBIOS) name')
    ap.add_argument('--workgroup', default='WORKGROUP')
    ap.add_argument('--comment', default='rs2 SMB test server', help='server comment')
    ap.add_argument('--user', action='append', metavar='NAME:PASSWORD', help='an account (repeatable)')
    ap.add_argument('--guest', action='store_true', help='admit anonymous and unknown users as guests')
    ap.add_argument('--share', action='append', metavar='NAME=DIR', help='a disk share (repeatable)')
    ap.add_argument('--printer', action='append', metavar='NAME=SPOOLDIR', help='a print queue (repeatable)')
    ap.add_argument('--share-comment', action='append', metavar='NAME=TEXT', help='a share remark (repeatable)')
    ap.add_argument('--require-signing', action='store_true', help='require signed requests after logon')
    ap.add_argument('--smb1', action='store_true', help='speak only SMB1 NT LM 0.12')
    ap.add_argument('--max-dialect', default='0210', choices=['0202', '0210'])
    ap.add_argument('--max-io', type=int, default=1048576, help='MaxRead/MaxWrite for SMB 2.1')
    ap.add_argument('--tag', help=argparse.SUPPRESS)
    a = ap.parse_args()
    srv = Server(a)
    log('server %s workgroup %s, shares: %s; users: %s%s%s' % (
        srv.name, srv.workgroup, ', '.join('%s (%s%s)' % (s.name, s.kind, ' ' + s.path if s.path else '')
                                         for s in srv.shares.values()),
        ', '.join(srv.users) or 'none', ', guest' if srv.guest else '',
        ', SMB1 only' if srv.smb1_only else ', SMB2 up to %04x' % srv.max_dialect))
    threads = []
    if a.nbns_port:
        threads.append(threading.Thread(target=nbns_serve, args=(srv, nbns_socket(a.bind, a.nbns_port)), daemon=True))
    if a.port:
        threads.append(threading.Thread(target=listen, args=(srv, tcp_socket(a.bind, a.port), False), daemon=True))
    if a.nbport:
        threads.append(threading.Thread(target=listen, args=(srv, tcp_socket(a.bind, a.nbport), True), daemon=True))
    for t in threads:
        t.start()
    try:
        while True:
            time.sleep(3600)
    except KeyboardInterrupt:
        pass


if __name__ == '__main__':
    main()
