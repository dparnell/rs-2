"""Lay out Win-OS/2 on the host for tools/dosrun, as APPINST installs it.

usage: python3 -I winos2_tree.py MEDIA TREE [--printer]

MEDIA is the client CD (the directory that holds OS2IMAGE, e.g.
work/media/client); TREE gets OS2\\MDOS\\WINOS2 with the files, INI
settings and RS2DISP.DRV of APPINST's `winos2` entry
(userland/appinst/apps.txt). --printer also installs Windows' TTY
driver as the default printer on LPT1: ("Generic / Text Only"), so
that a Windows program's File > Print reaches dosrun's --lpt file (and
under rs2 the spooler: docs/PRINTING.md). Needs `cargo build --release
-p unpack` first. Then, e.g.:

  dosrun --c TREE --cwd 'OS2\\MDOS\\WINOS2' --lpt OUT \\
      --keys '{wait 20}{alt-f}{wait 2}p{wait 15}' \\
      'C:\\OS2\\MDOS\\WINOS2\\WINOS2.COM' 'NOTEPAD.EXE C:\\FILE.TXT'
"""
import os
import shutil
import subprocess
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
PACKED = b'\xa5\x96\xfd\xff'


def ci_path(base, rel):
    """A path looked up without regard to case (None if missing)."""
    cur = base
    for part in [p for p in rel.replace('\\', '/').split('/') if p]:
        if not os.path.isdir(cur):
            return None
        names = {n.upper(): n for n in os.listdir(cur)}
        if part.upper() not in names:
            return None
        cur = os.path.join(cur, names[part.upper()])
    return cur


def ini_set(path, section, key, value):
    """Set key=value in an INI file's section (made if missing)."""
    text = open(path, 'rb').read().decode('latin-1') if os.path.exists(path) else ''
    out, in_sec, done, found = [], False, False, False
    for line in text.replace('\r\n', '\n').split('\n'):
        s = line.strip()
        if s.startswith('['):
            if in_sec and not done:
                out.append(f'{key}={value}')
                done = True
            in_sec = s[1:-1].lower() == section.lower()
            found |= in_sec
        elif in_sec and '=' in s and s.split('=', 1)[0].strip().lower() == key.lower():
            if not done:
                out.append(f'{key}={value}')
                done = True
            continue
        out.append(line)
    if not done:
        if not found:
            out.append(f'[{section}]')
        out.append(f'{key}={value}')
    open(path, 'wb').write('\r\n'.join(out).encode('latin-1'))


def main():
    args = [a for a in sys.argv[1:] if not a.startswith('--')]
    if len(args) != 2:
        sys.exit(__doc__)
    media, tree = args
    printer = '--printer' in sys.argv
    stage = os.path.join(tree, '.stage')
    unpack = os.path.join(ROOT, 'target', 'release', 'unpack')
    lines = open(os.path.join(ROOT, 'userland', 'appinst', 'apps.txt')).read().splitlines()
    sec, on = [], False
    for line in lines:
        if line.startswith('app winos2'):
            on = True
            continue
        if on and line.startswith('app '):
            break
        if on:
            sec.append(line)
    done, staged = set(), {}
    for line in sec:
        if not line.startswith('file '):
            continue
        p = [x.strip() for x in line[5:].split('|')]
        target, src = p[0], p[1]
        stored = p[2] if len(p) > 2 and p[2] else target
        if target.upper() in done:
            continue
        srcp = ci_path(media, src)
        if srcp is None:
            print('missing', src)
            continue
        if open(srcp, 'rb').read(4) == PACKED:
            if src not in staged:
                d = os.path.join(stage, src.replace('\\', '_'))
                os.makedirs(d, exist_ok=True)
                subprocess.run([unpack, 'extract', srcp, d], check=True, stdout=subprocess.DEVNULL)
                staged[src] = d
            fp = ci_path(staged[src], stored)
        else:
            fp = srcp
        if fp is None:
            print('not in', src, stored)
            continue
        out = os.path.join(tree, target.lstrip('\\').replace('\\', '/'))
        os.makedirs(os.path.dirname(out), exist_ok=True)
        shutil.copy(fp, out)
        done.add(target.upper())
    shutil.rmtree(stage, ignore_errors=True)
    win = os.path.join(tree, 'OS2', 'MDOS', 'WINOS2')
    for line in sec:
        if line.startswith('make ') and 'rs2disp' in line:
            subprocess.run(['cargo', 'run', '-q', '--manifest-path', os.path.join(ROOT, 'Cargo.toml'), '-p', 'win16drv', '--bin', 'mkrs2disp',
                            os.path.join(win, 'SYSTEM', 'VGA.DRV'), os.path.join(win, 'SYSTEM', 'RS2DISP.DRV')], check=True)
        if line.startswith('ini '):
            p = [x.strip() for x in line[4:].split('|')]
            f = ci_path(win, p[0].split('\\')[-1]) or os.path.join(win, p[0].split('\\')[-1])
            ini_set(f, p[1], p[2], p[3] if len(p) > 3 else '')
    if printer:
        tty = ci_path(media, 'OS2IMAGE/DISK_W4/TTY.DRV')
        shutil.copy(tty, os.path.join(win, 'SYSTEM', 'TTY.DRV'))
        w = ci_path(win, 'WIN.INI')
        ini_set(w, 'windows', 'device', 'Generic / Text Only,TTY,LPT1:')
        ini_set(w, 'devices', 'Generic / Text Only', 'TTY,LPT1:')
        ini_set(w, 'PrinterPorts', 'Generic / Text Only', 'TTY,LPT1:,15,45')
    print(len(done), 'files in', win)


main()
