#!/bin/sh
# disasm.sh BLOB BASE LANG START END
set -e
here=$(cd "$(dirname "$0")" && pwd)
proj=${GHIDRA_PROJ:-${TMPDIR:-/tmp}/rs2-ghidra}
mkdir -p "$proj"
JAVA_HOME="${JAVA_HOME:-/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home}" \
/opt/homebrew/Cellar/ghidra/*/libexec/support/analyzeHeadless "$proj" rs2d \
  -import "$1" -overwrite -loader BinaryLoader -loader-baseAddr "$2" \
  -processor "$3" -noanalysis -scriptPath "$here" -postScript Disasm.java "$4" "$5" 2>&1 \
  | grep 'Disasm.java> ' | sed -e 's/^INFO  Disasm.java> //' -e 's/ (GhidraScript)  *$//'
