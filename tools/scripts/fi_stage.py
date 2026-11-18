#!/usr/bin/env python3
"""Stage a Warp 4 Feature Installer package for an rs2 test disk.

    fi_stage.py INSTDATA.INI SOURCE_DIR OUT_DIR [--drive C:] [--setup CMDNAME]

Reads the package's INSTDATA.INI (e.g. the Warp 4 CD's
OS2IMAGE/FI/FEATURE_/OS2BONUS/BPWORKS for IBM Works) and copies its File
entries from SOURCE_DIR (the CD's directory with the files, e.g.
OS2IMAGE/FI/BONUSPAK/IBMWORKS) to OUT_DIR as the Feature Installer would
place them on the target drive ({TargetDrive}, {BootDrive}). The
package's extended attributes (the CD's 000000NN.EAS files, 16-bit
FEALISTs, which belong in order to the File entries that have an
EAMediaIndex) go into an EAS directory beside each file, the hold files
EAUTIL /J joins. Writes OUT_DIR/../modules.txt (lines for `cargo xtask
disk`'s work/modules.txt) and, with --setup, a CMD file that joins the
EAs on the running system (the user exits, objects, class registrations
and CONFIG.SYS edits of the package are left to the product's own setup
script, IBM Works' IBMWDESK.CMD).

The IBM files are test inputs: never commit or ship them.
"""
import argparse
import os
import re
import shutil


def parse(path):
    text = open(path, 'rb').read().decode('latin-1').replace('\r\n', '\n')
    blocks, cur = [], None
    for line in text.split('\n'):
        s = line.strip()
        if s.endswith('=('):
            cur = {'_type': s[:-2]}
            blocks.append(cur)
        elif '=' in s and cur is not None:
            k, v = s.split('=', 1)
            cur[k] = v
    return blocks


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('ini')
    ap.add_argument('src')
    ap.add_argument('out')
    ap.add_argument('--drive', default='C:')
    ap.add_argument('--setup')
    a = ap.parse_args()
    blocks = parse(a.ini)
    var = {'TargetDrive': a.drive, 'BootDrive': a.drive}
    for b in blocks:
        if b['_type'] == 'Variable' and b['Name'] not in var:
            var[b['Name']] = b['Value']

    def sub(v):
        for _ in range(5):
            v = re.sub(r'\{(\w+)\}', lambda m: var.get(m.group(1), m.group(0)), v)
        return v

    files = {f.upper(): f for f in os.listdir(a.src)}
    eas, n = [], 0
    for b in blocks:
        if b['_type'] != 'File':
            continue
        name = sub(b['MediaFileName'])
        real = files.get(name.upper())
        if real is None:
            print('missing', name)
            continue
        tp, tn = sub(b['TargetPath']), sub(b['TargetFileName'])
        d = os.path.join(a.out, *tp[3:].upper().split('\\'))
        os.makedirs(d, exist_ok=True)
        shutil.copyfile(os.path.join(a.src, real), os.path.join(d, tn.upper()))
        n += 1
        if b.get('EAMediaIndex', '0') != '0':
            eas.append((tp, tn))
    for i, (tp, tn) in enumerate(eas):
        d = os.path.join(a.out, *tp[3:].upper().split('\\'), 'EAS')
        os.makedirs(d, exist_ok=True)
        shutil.copyfile(os.path.join(a.src, '%08X.EAS' % i), os.path.join(d, tn.upper()))
    lines = []
    for root, _, fs in os.walk(a.out):
        for f in fs:
            p = os.path.abspath(os.path.join(root, f))
            lines.append('%s -> %s' % (p, os.path.relpath(p, os.path.abspath(a.out)).replace('/', '\\')))
    if a.setup:
        cmd = ['@echo off', 'rem Join the EAs the Feature Installer would have kept with the files']
        dirs = set()
        for tp, tn in eas:
            cmd.append('EAUTIL %s\\%s /J' % (tp.upper(), tn.upper()))
            dirs.add(tp.upper())
        cmd += ['RD %s\\EAS' % d for d in sorted(dirs)]
        path = os.path.join(a.out, a.setup.upper())
        open(path, 'w', newline='').write('\r\n'.join(cmd) + '\r\n')
        lines.append('%s -> %s' % (os.path.abspath(path), a.setup.upper()))
    open(os.path.join(a.out, '..', 'modules.txt'), 'w').write('\n'.join(lines) + '\n')
    print(n, 'files,', len(eas), 'with EAs')


if __name__ == '__main__':
    main()
