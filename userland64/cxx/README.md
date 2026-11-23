# C++ for rs2's 64-bit programs

See docs/CXX64.md.

- `llvm/`: libc++, libc++abi and libunwind from LLVM 21.1.8
  (https://github.com/llvm/llvm-project, tag llvmorg-21.1.8, commit
  2078da43e25a4623cab2d0d60decddf709aaea28), with the parts of LLVM's libc
  that libc++'s `from_chars` uses. Apache License 2.0 with LLVM
  Exceptions: each directory's LICENSE.TXT. Left out: libc++'s frozen
  C++03 headers, experimental parts and the Windows and z/OS files.
- `config/`: libc++'s configuration for rs2.

rs2's changes are marked `__RS2__`: `grep -rn __RS2__ llvm`. They are the
paths with drive letters and backslashes (`__filesystem/path.h`,
`path_parser.h`, `path.cpp`, `operations.cpp`, `posix_compat.h`),
`atomic::wait` on `__rs2_wait` (`atomic.cpp`), `getentropy` for
`random_device` (`__config`) and the "C" locale functions
(`locale_base_api.h`), type comparison (`typeinfo`) and finding unwind
tables (libunwind's `AddressSpace.hpp`).
