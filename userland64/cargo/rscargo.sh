#!/bin/sh
# RSCARGO.DLL: the C libraries of cargo, on LIBC64.DLL: SQLite, and
# stand-ins for curl and libgit2.
#
#   rscargo.sh IMPORTS OUT      IMPORTS: the functions cargo imports
set -e
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
dir=${RUSTC_DIR:-$root/work/rustc}
cc=$root/userland64/c/cc64.sh
bin=$(ls -d "$(rustc --print sysroot)"/lib/rustlib/*/bin | head -1)
o=$dir/rscargo
mkdir -p "$o"
$cc -c -DSQLITE_OMIT_LOAD_EXTENSION -DSQLITE_THREADSAFE=1 -DSQLITE_OMIT_WAL -DSQLITE_ENABLE_COLUMN_METADATA \
    -DSQLITE_ENABLE_FTS3 -DSQLITE_MAX_MMAP_SIZE=0 -DSQLITE_ENABLE_API_ARMOR '-DSQLITE_API=__attribute__((visibility("default")))' \
    -w "$dir/cargo/libsqlite3-sys/sqlite3/sqlite3.c" -o "$o/sqlite3.o"
$cc -c -w "$here/stubs.c" -o "$o/stubs.o"
"$bin/llvm-nm" --defined-only "$o/sqlite3.o" "$o/stubs.o" | awk 'NF == 3 { print $3 }' | sort -u > "$o/have"
sort "$1" | comm -23 - "$o/have" | sed 's/.*/__attribute__((visibility("default"))) long &(void) { return -1; }/' > "$o/fail.c"
$cc -c -w "$o/fail.c" -o "$o/fail.o"
$cc --dll -o "$2" "$o/sqlite3.o" "$o/stubs.o" "$o/fail.o"
ls -l "$2"
