#!/bin/sh
# Build the Win32 test programs with Open Watcom v2 (wcl386 -bt=nt).
# WATCOM defaults to ~/opt/watcom (the GitHub snapshot); the programs go
# to bin/, which xtask puts on the image as C:\RS2\*.EXE when it exists
# (docs/WIN32.md).
set -e
dir=$(cd "$(dirname "$0")" && pwd)
: "${WATCOM:=$HOME/opt/watcom}"
[ -d "$WATCOM" ] || { echo "win32test: Open Watcom not found (set WATCOM)"; exit 0; }
case "$(uname -s)-$(uname -m)" in
Darwin-arm64) host=armo64 ;;
Darwin-*) host=bino64 ;;
Linux-aarch64) host=arml64 ;;
*) host=binl64 ;;
esac
export WATCOM PATH="$WATCOM/$host:$PATH" INCLUDE="$WATCOM/h:$WATCOM/h/nt"
mkdir -p "$dir/bin" "$dir/obj"
cd "$dir/obj"
# W32DLL.DLL first: W32TEST imports it (its import library from the DLL).
wcl386 -q -bt=nt -bd -l=nt_dll -fe=W32DLL.DLL "$dir/w32dll.c"
wlib -q -n w32dll.lib +W32DLL.DLL
mv W32DLL.DLL "$dir/bin/"
for src in "$dir"/*.c; do
    name=$(basename "$src" .c | tr a-z A-Z)
    imp=
    case "$name" in
    W32DLL) continue ;;
    W32TEST) imp=w32dll.lib ;;
    esac
    wcl386 -q -bt=nt -l=nt -fe="$name.EXE" "$src" $imp
    mv "$name.EXE" "$dir/bin/"
done
