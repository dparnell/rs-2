#!/usr/bin/env python3
"""Write dist/PKGLIST.TXT, what PKGSETUP downloads: the RPMs of the set
docs/PACKAGES.md tests (work/pk/rpms), with the SHA-256 of each from the
repository's metadata, checked against the file here.
pkglist.py [PK_DIR], from the repository root.
"""
import hashlib, os, sqlite3, sys
pk = sys.argv[1] if len(sys.argv) > 1 else 'work/pk'
db = sqlite3.connect(f'{pk}/meta/primary.sqlite')
out = ['# PKGSETUP: SHA-256, size, path below the repository. Made by tools/scripts/pkglist.py.']
for f in sorted(os.listdir(f'{pk}/rpms')):
    if not f.endswith('.rpm'):
        continue
    (sha, size, loc), = db.execute('select pkgId, size_package, location_href from packages where location_href like ?', ('%/' + f,)).fetchall()
    data = open(f'{pk}/rpms/{f}', 'rb').read()
    assert hashlib.sha256(data).hexdigest() == sha and len(data) == size, f
    out.append(f'{sha} {size} {loc}')
open('dist/PKGLIST.TXT', 'w').write('\n'.join(out) + '\n')
print(len(out) - 1, 'packages')
