#!/bin/sh
# Makes work/ow2sys.img: an rs2 system disk (rs2's own files only) with Open
# Watcom 2.0 (the GitHub "Current-build" OS/2 package) in C:\WATCOM as its
# installer lays out an OS/2-hosted Full installation (the installer's ZIP
# unpacked on the host without the Linux, Windows NT and RDOS hosts' files;
# its CONFIG.SYS lines in tests/owatcom2/config.sys) and the test sources
# in C:\OWT: tests/owatcom's (the 1.9 tests) with tests/owatcom2's on top.
# The installer itself is tested separately (docs/COMPATIBILITY.md).
#
#   tests/owatcom2/mkdisk.sh [extra manifest lines file]
#   INIT="prog args" (default: OS2\CMD.EXE /C C:\OWT\RUN.CMD, the boot program)
#   OUT=work/x.img another image name
#   F77=1 also unpacks the F77 package over it (as installing both does)
#   cargo xtask run --boot-hdd --hdd work/ow2sys.img --headless --timeout N ...
#
# Note: replaces work/modules.txt (keep your own elsewhere).
set -e
root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
zip=${OWZIP:-work/media/open-watcom-2_0-c-os2.exe}
fzip=${OWFZIP:-work/media/open-watcom-2_0-f77-os2.exe}
ow=work/owatcom2
skip='^(binl|binl64|binnt|binnt64|rdos|lh|nlm)/'
if [ ! -f $ow/setup.inf ]; then
    mkdir -p $ow
    unzip -q -o "$zip" -d $ow || true
fi
if [ -n "$F77" ] && [ ! -f $ow/binp/wfc386.exe ]; then
    unzip -q -o "$fzip" -d $ow -x setup.inf || true
fi
m=work/modules.txt
grep -v '^#' dist/manifest.txt | grep -v '^dist/CONFIG.SYS' > $m
(cd $ow && find . -type f | sed 's#^\./##' | grep -Ev "$skip") | while read -r f; do
    printf '%s -> WATCOM/%s\n' "$ow/$f" "$f"
done >> $m
# the 1.9 tests, then this directory's (a file here replaces the one there)
for f in tests/owatcom/* tests/owatcom2/*; do
    case $f in *.sh|*/config.sys) continue ;; esac
    [ -f "$f" ] || continue
    printf '%s -> OWT/%s\n' "$f" "$(basename "$f")"
done | awk '{ n[$3] = $0 } END { for (k in n) print n[k] }' >> $m
echo "tests/owatcom2/config.sys -> CONFIG.SYS" >> $m
if [ -n "$1" ]; then cat "$1" >> $m; fi
cargo xtask disk --system --size ${SIZE:-1400} -o ${OUT:-work/ow2sys.img} --init "${INIT:-OS2\CMD.EXE /C C:\OWT\RUN.CMD}"
