#!/bin/sh
# Build the Win16 test programs with Open Watcom v2 (wcl -bt=windows).
# WATCOM defaults to ~/opt/watcom (the GitHub snapshot); the programs go
# to bin/, which xtask puts on the image as C:\RS2\*.EXE when it exists.
# A program's resources are in the .rc of the same name.
set -e
dir=$(cd "$(dirname "$0")" && pwd)
: "${WATCOM:=$HOME/opt/watcom}"
[ -d "$WATCOM" ] || { echo "win16test: Open Watcom not found (set WATCOM)"; exit 0; }
case "$(uname -s)-$(uname -m)" in
Darwin-arm64) host=armo64 ;;
Darwin-*) host=bino64 ;;
Linux-aarch64) host=arml64 ;;
*) host=binl64 ;;
esac
export WATCOM PATH="$WATCOM/$host:$PATH" INCLUDE="$WATCOM/h:$WATCOM/h/win"
mkdir -p "$dir/bin" "$dir/obj"
cd "$dir/obj"
for src in "$dir"/*.c; do
    name=$(basename "$src" .c | tr a-z A-Z)
    wcl -q -bt=windows -l=windows -zW -fe="$name.EXE" "$src" lzexpand.lib ver.lib
    # Resources (NAME.rc) are bound into the program.
    rc="$dir/$(basename "$src" .c).rc"
    [ -f "$rc" ] && wrc -q -bt=windows -i="$dir" "$rc" "$name.EXE"
    mv "$name.EXE" "$dir/bin/"
done
