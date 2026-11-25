rs2 developer disk

Compilers that run on rs2: Rust (rustc, cargo), C and C++ (clang, lld), for
64-bit OS/2 programs and for building rs2 itself.

To install, with rs2 running from an HPFS or JFS disk:

  SETUP.EXE     on the desktop
  INSTALL.EXE   at a command prompt (INSTALL /? lists its options)

rs2's sources are optional: SETUP has a check box for them and INSTALL asks
(INSTALL /SOURCE, INSTALL /NOSOURCE).

Both unpack SDK.ZIP (and SOURCE.ZIP, the sources) into a directory, C:\RS2DEV
unless you choose another, and add it to PATH and LIBPATH in CONFIG.SYS, with
CARGO_HOME and TMPDIR. The old CONFIG.SYS is kept as CONFIG.DEV. Then read
README.TXT in that directory.

The computer needs 4 GB of memory or more, 8 GB to build rs2.

The compilers are LLVM's and the Rust project's, under their own licences
(the directory "licenses" after the installation).
