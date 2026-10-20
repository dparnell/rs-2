#!/bin/sh
# lxdecomp.sh NAME FUNC|0xADDR|callers:FUNC ...   (after lxload.sh)
set -e
here=$(cd "$(dirname "$0")" && pwd)
proj=${GHIDRA_PROJ:-${TMPDIR:-/tmp}/rs2-ghidra}
name=$1; shift
JAVA_HOME="${JAVA_HOME:-/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home}" \
/opt/homebrew/Cellar/ghidra/*/libexec/support/analyzeHeadless "$proj" rs2lx \
  -process "$name" -noanalysis -readOnly -scriptPath "$here" -postScript DecompileNamed.java "$@" 2>&1 \
  | sed -n '/^INFO  DecompileNamed.java> ====/,$p' | sed -e 's/^INFO  DecompileNamed.java> //' -e 's/ (GhidraScript)  *$//' -e '/^INFO  /d'
