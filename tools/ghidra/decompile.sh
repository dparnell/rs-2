#!/bin/sh
# decompile.sh BLOB BASE LANG ADDR[=NAME]...
#   LANG e.g. x86:LE:32:default or x86:LE:16:Protected Mode
set -e
blob=$1; base=$2; lang=$3; shift 3
here=$(cd "$(dirname "$0")" && pwd)
proj=${GHIDRA_PROJ:-${TMPDIR:-/tmp}/rs2-ghidra}
mkdir -p "$proj"
name=$(basename "$blob")
JAVA_HOME="${JAVA_HOME:-/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home}" /opt/homebrew/Cellar/ghidra/*/libexec/support/analyzeHeadless "$proj" rs2 \
  -import "$blob" -overwrite -loader BinaryLoader -loader-baseAddr "$base" \
  -processor "$lang" -noanalysis \
  -scriptPath "$here" -postScript Decompile.java "$@" 2>&1 \
  | sed -n '/^INFO  Decompile.java> ====/,$p' | sed -e 's/^INFO  Decompile.java> //' -e '/^INFO  /d'
