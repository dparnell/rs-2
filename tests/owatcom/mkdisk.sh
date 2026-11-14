#!/bin/sh
# Makes work/owsys.img: an rs2 system disk (rs2's own files only) with Open
# Watcom 1.9 installed in C:\WATCOM as its installer's Full installation
# lays it out (the installer's ZIP unpacked on the host, its CONFIG.SYS
# lines in tests/owatcom/config.sys) and the test sources in C:\OWT.
# The installer itself is tested separately (docs/COMPATIBILITY.md); this
# skips its 6 minutes of copying for every rebuilt rs2.
#
#   tests/owatcom/mkdisk.sh [extra manifest lines file]
#   INIT="prog args" (default: OS2\CMD.EXE /C C:\OWT\RUN.CMD, the boot program)
#   cargo xtask run --boot-hdd --hdd work/owsys.img --headless --timeout N ...
#
# Note: replaces work/modules.txt (keep your own elsewhere).
set -e
root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
zip=${OWZIP:-work/media/open-watcom-c-os2-1.9.exe}
ow=work/owatcom
if [ ! -f $ow/setup.inf ]; then
    mkdir -p $ow
    unzip -q -o "$zip" -d $ow || true
fi
m=work/modules.txt
grep -v '^#' dist/manifest.txt | grep -v '^dist/CONFIG.SYS' > $m
(cd $ow && find . -type f | sed 's#^\./##') | while read -r f; do
    printf '%s -> WATCOM/%s\n' "$ow/$f" "$f"
done >> $m
for f in tests/owatcom/*; do
    case $f in *.sh) continue ;; esac
    printf '%s -> OWT/%s\n' "$f" "$(basename "$f")"
done >> $m
echo "tests/owatcom/config.sys -> CONFIG.SYS" >> $m
[ -n "$1" ] && cat "$1" >> $m
cargo xtask disk --system --size 1200 -o work/owsys.img --init "${INIT:-OS2\CMD.EXE /C C:\OWT\RUN.CMD}"
