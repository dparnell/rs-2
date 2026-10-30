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
    # W16DLL.C is a DLL, linked a second time as W16D43.DLL (module name
    # W16DLL, another value). W16CYC1 and W16CYC2 import each other.
    case "$name" in
    W16DLL)
        wcl -q -bt=windows -bd -l=windows_dll -zW -fe="$name.DLL" "$src"
        mv "$name.DLL" "$dir/bin/"
        wcl -q -bt=windows -bd -zW -DW16DLL_VALUE=43 -c -fo=W16D43.OBJ "$src"
        wlink op q system windows_dll name W16D43.DLL op modname=W16DLL file W16D43.OBJ
        mv W16D43.DLL "$dir/bin/"
        continue ;;
    W16CYC1|W16CYC2)
        other=W16CYC2; imp=W16CYC2VALUE
        [ "$name" = W16CYC2 ] && other=W16CYC1 && imp=W16CYC1BASE
        wcl -q -bt=windows -bd -zW -c -fo="$name.OBJ" "$src"
        wlink op q system windows_dll name "$name.DLL" file "$name.OBJ" import "$imp" "$other.$imp"
        mv "$name.DLL" "$dir/bin/"
        continue ;;
    esac
    wcl -q -bt=windows -l=windows -zW -fe="$name.EXE" "$src" lzexpand.lib ver.lib
    # Resources (NAME.rc) are bound into the program.
    rc="$dir/$(basename "$src" .c).rc"
    [ -f "$rc" ] && wrc -q -bt=windows -i="$dir" "$rc" "$name.EXE"
    mv "$name.EXE" "$dir/bin/"
done
