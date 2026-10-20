#!/bin/sh
# lxload.sh MODULE [SYMFILE]
#   Lays out an LX module (crates/os2-exe example lximage) and imports it,
#   fully analysed, into the Ghidra project $GHIDRA_PROJ (program name =
#   the module's file name). Then: lxdecomp.sh NAME FUNC|0xADDR|callers:FUNC...
set -e
here=$(cd "$(dirname "$0")" && pwd)
proj=${GHIDRA_PROJ:-${TMPDIR:-/tmp}/rs2-ghidra}
name=$(basename "$1")
out=$proj/lx/$name
mkdir -p "$out"
cargo run -q --manifest-path "$here/../../Cargo.toml" -p os2-exe --example lximage -- "$1" "$out" $2
python3 -I "$here/impnames.py" "$here/../.." "$out/symbols.txt"
first=$(head -1 "$out/layout.txt")
set -- $first
cp "$out/$1" "$out/$name"
JAVA_HOME="${JAVA_HOME:-/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home}" \
/opt/homebrew/Cellar/ghidra/*/libexec/support/analyzeHeadless "$proj" rs2lx \
  -import "$out/$name" -overwrite -loader BinaryLoader -loader-baseAddr "$2" \
  -processor x86:LE:32:default -scriptPath "$here" -preScript LxSetup.java "$out" 2>&1 \
  | grep -E 'ERROR|Import succeeded|ANALYZING' || true
