#!/bin/sh
# Copies C:\OWT's logs and results (*.LOG *.OK *.OUT *.DMP) from
# work/ow2sys.img (or $IMG) to DIR (macOS: hdiutil + mount_msdos). QEMU must
# be done with the image.
#   tests/owatcom2/getlogs.sh DIR [more file patterns]
set -e
root=$(cd "$(dirname "$0")/../.." && pwd)
img=${IMG:-$root/work/ow2sys.img}
out=$1; shift
mkdir -p "$out"
dev=$(hdiutil attach -imagekey diskimage-class=CRawDiskImage -nomount "$img" | head -1 | awk '{print $1}')
mnt=$(mktemp -d)
mount -t msdos "${dev}s1" "$mnt"
for p in '*.LOG' '*.OK' '*.OUT' '*.DMP' "$@"; do
    for f in "$mnt"/OWT/$p; do
        [ -f "$f" ] && cp "$f" "$out/"
    done
done
umount "$mnt"
hdiutil detach "$dev" > /dev/null
rmdir "$mnt"
ls "$out"
