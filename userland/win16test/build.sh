#!/bin/sh
# Build the Win16 test programs with Open Watcom v2 (wcl -bt=windows).
# WATCOM defaults to ~/opt/watcom (the GitHub snapshot); the programs go
# to bin/, which xtask puts on the image as C:\RS2\*.EXE when it exists.
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
    wcl -q -bt=windows -l=windows -zW -fe="$name.EXE" "$src"
    mv "$name.EXE" "$dir/bin/"
done
