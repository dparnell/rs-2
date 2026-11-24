//! Build a 64-bit rs2 program or DLL from C or C++ (docs/API64.md, "Building
//! 64-bit programs in C"; the C library: docs/LIBC64.md; C++: docs/CXX64.md):
//!
//! ```text
//! cc64 -o OUT.EXE [--pm] [--dll] [--static] [--rc FILE.RC] [CFLAGS...] FILE.c|.cpp|.o|.a|.DLL...
//! cc64 -c [--static] [CFLAGS...] FILE.c|.cpp [-o FILE.o]
//! cc64 --libc            (just build the C library)
//! cc64 --libcxx          (just build the C++ library)
//! cc64 --flags C|CXX     (print clang's flags: userland64/cmake)
//! ```
//!
//! userland64/c/cc64.sh runs it on Unix hosts, xtask on any (rs2 too: it
//! needs no shell). Only std, so that rustc alone can build it.
//!
//! clang (any recent one: Apple's will do) compiles for x86-64 System V
//! with userland64/include as the only system headers; rust-lld links with
//! userland64/link64.ld as the Rust programs are, against the C library,
//! built here from userland64/libc when its sources change: by default
//! LIBC64.DLL (programs and DLLs import it through libc64.a, and programs
//! get crt1.o, their own start-up code), with --static libc.a. Objects for
//! one way must not be linked the other way (-D_RS2_LIBC_DLL decides how
//! the headers declare the library's data).
//! C++ sources (.cpp .cc .cxx) get userland64/cxx: LLVM's libc++, libc++abi
//! and libunwind, in LIBCXX64.DLL (with --static, linked into the module);
//! --cxx uses them without C++ sources on the command line (objects only).
//! A 64-bit DLL on the command line is linked against: what it exports
//! resolves there (the program then needs that DLL, by its file name).
//! --pm marks a Presentation Manager program, --rc compiles an .RC script
//! into the module's resources (os2-rc's table64).
//!
//! Environment: CLANG, RUST_LLD, LLVM_AR (the programs, if not clang and
//! rustup's llvm-tools, or else rust-lld and llvm-ar on PATH), LIBC64_DIR
//! (where the libraries are built: target/libc64), CC64_ROOT.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, exit};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

const TARGET: &str = "x86_64-unknown-none-elf";
/// Where LIBC64.DLL and LIBCXX64.DLL are linked (the loader moves them if
/// that range is taken).
const LIBC64_BASE: &str = "0x18100000";
const LIBCXX64_BASE: &str = "0x18200000";

fn die(why: impl std::fmt::Display) -> ! {
    eprintln!("cc64: {why}");
    exit(1)
}

fn usage() -> ! {
    eprintln!("usage: cc64 -o OUT [--pm] [--dll] [--rc FILE.RC] [CFLAGS] FILE.c|.o|.a...");
    eprintln!("       cc64 -c [CFLAGS] FILE.c [-o FILE.o]");
    exit(2)
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn write(p: &Path, data: impl AsRef<[u8]>) {
    fs::write(p, data).unwrap_or_else(|e| die(format_args!("{}: {e}", p.display())))
}

fn make_dir(p: &Path) {
    fs::create_dir_all(p).unwrap_or_else(|e| die(format_args!("{}: {e}", p.display())))
}

fn copy(a: &Path, b: &Path) {
    let _ = fs::remove_file(b);
    fs::copy(a, b).unwrap_or_else(|e| die(format_args!("{} -> {}: {e}", a.display(), b.display())));
}

/// The files of a directory with one of these extensions, by name.
fn files(dir: &Path, exts: &[&str]) -> Vec<PathBuf> {
    let rd = fs::read_dir(dir).unwrap_or_else(|e| die(format_args!("{}: {e}", dir.display())));
    let mut v: Vec<PathBuf> =
        rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| exts.iter().any(|e| x == *e))).collect();
    v.sort();
    v
}

fn modified(p: &Path) -> Option<SystemTime> {
    fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// Is a file at or below `p` newer than `t`?
fn newer(p: &Path, t: SystemTime) -> bool {
    match fs::read_dir(p) {
        Ok(rd) => rd.flatten().any(|e| newer(&e.path(), t)),
        Err(_) => modified(p).is_some_and(|m| m > t),
    }
}

fn is_cxx(f: &str) -> bool {
    [".cpp", ".cc", ".cxx"].iter().any(|x| f.ends_with(x))
}

fn is_dll(f: &str) -> bool {
    f.ends_with(".dll") || f.ends_with(".DLL")
}

/// A directory of our own, removed at the end.
struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Tools {
    root: PathBuf,
    ul: PathBuf,
    llvm: PathBuf,
    libdir: PathBuf,
    clang: Vec<String>,
    tmp: PathBuf,
    sysroot_bin: std::sync::OnceLock<Option<PathBuf>>,
}

/// Run a program; one that PATH only has as NAME.exe (rs2, Windows) too.
fn status(mut c: Command, what: &str) -> ExitStatus {
    match c.status() {
        Ok(s) => s,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            let mut name = c.get_program().to_os_string();
            name.push(".exe");
            let mut d = Command::new(name);
            d.args(c.get_args());
            if let Some(p) = c.get_current_dir() {
                d.current_dir(p);
            }
            d.status().unwrap_or_else(|_| die(format_args!("{what}: {e}")))
        }
        Err(e) => die(format_args!("{what}: {e}")),
    }
}

fn run(c: Command) {
    let what = c.get_program().to_string_lossy().into_owned();
    let s = status(c, &what);
    if !s.success() {
        exit(s.code().unwrap_or(1));
    }
}

/// Run jobs on as many threads as there are processors.
fn parallel<T: Sync>(jobs: &[T], f: impl Fn(&T) -> bool + Sync) {
    let next = AtomicUsize::new(0);
    let failed = AtomicUsize::new(0);
    let n = std::thread::available_parallelism().map_or(4, |n| n.get()).min(jobs.len());
    std::thread::scope(|sc| {
        for _ in 0..n {
            sc.spawn(|| {
                while let Some(j) = jobs.get(next.fetch_add(1, Ordering::Relaxed)) {
                    if failed.load(Ordering::Relaxed) != 0 || !f(j) {
                        failed.store(1, Ordering::Relaxed);
                        return;
                    }
                }
            });
        }
    });
    if failed.into_inner() != 0 {
        exit(1);
    }
}

impl Tools {
    /// rustup's llvm-tools directory.
    fn sysroot_bin(&self) -> Option<&Path> {
        self.sysroot_bin
            .get_or_init(|| {
                let rustc = std::env::var_os("RUSTC").unwrap_or("rustc".into());
                let o = Command::new(rustc).args(["--print", "sysroot"]).output().ok()?;
                let d = Path::new(String::from_utf8_lossy(&o.stdout).trim()).join("lib/rustlib");
                let mut v: Vec<PathBuf> = fs::read_dir(d).ok()?.flatten().map(|e| e.path().join("bin")).collect();
                v.sort();
                v.into_iter().find(|p| p.is_dir())
            })
            .as_deref()
    }

    /// $VAR, or one of rustup's llvm-tools, or the name for PATH.
    fn tool(&self, var: &str, name: &str) -> OsString {
        if let Some(v) = std::env::var_os(var).filter(|v| !v.is_empty()) {
            return v;
        }
        if let Some(b) = self.sysroot_bin() {
            for n in [name.to_string(), format!("{name}.exe")] {
                if b.join(&n).exists() {
                    return b.join(n).into();
                }
            }
        }
        name.into()
    }

    fn ar(&self, lib: &Path, objs: &[PathBuf]) {
        let _ = fs::remove_file(lib);
        let mut c = Command::new(self.tool("LLVM_AR", "llvm-ar"));
        c.arg("rcs").arg(lib);
        self.args(&mut c, objs.iter().map(|o| s(o)).collect());
        run(c);
    }

    /// Arguments, in a response file if they are long (rs2's command lines
    /// are short).
    fn args(&self, c: &mut Command, a: Vec<String>) {
        if a.iter().map(|a| a.len() + 1).sum::<usize>() < 16000 {
            c.args(a);
            return;
        }
        static N: AtomicUsize = AtomicUsize::new(0);
        let f = self.tmp.join(format!("{}.rsp", N.fetch_add(1, Ordering::Relaxed)));
        let q = |a: &String| format!("\"{}\"\n", a.replace('\\', "\\\\").replace('"', "\\\""));
        write(&f, a.iter().map(q).collect::<String>());
        c.arg(format!("@{}", s(&f)));
    }

    /// Not the Linux target: __linux__ would mislead portable programs.
    /// Stacks grow over a guard page, so frames of a page or more must
    /// touch every page (clang's stack-clash protection).
    fn common(&self) -> Vec<String> {
        let t = format!("--target={TARGET}");
        let mut v = vec![t.as_str()];
        v.extend("-D__RS2__ -D__OS2__ -nostdinc -fno-stack-protector -fPIC -fvisibility=hidden -fno-plt".split(' '));
        v.extend("-femulated-tls -Xclang -fstack-clash-protection -O2".split(' '));
        v.into_iter().map(String::from).collect()
    }

    fn isystem(&self, v: &mut Vec<String>, dirs: &[PathBuf]) {
        for d in dirs {
            v.extend(["-isystem".into(), s(d)]);
        }
    }

    fn cxx_dirs(&self) -> [PathBuf; 4] {
        [self.ul.join("cxx/config"), self.llvm.join("libcxx/include"), self.llvm.join("libcxxabi/include"), self.ul.join("include")]
    }

    fn clang(&self) -> Command {
        let mut c = Command::new(&self.clang[0]);
        c.args(&self.clang[1..]);
        c
    }

    fn cc(&self, flags: &[String]) -> Command {
        let mut v = self.common();
        self.isystem(&mut v, &[self.ul.join("include")]);
        v.extend("-fexceptions -fasynchronous-unwind-tables -std=gnu11 -Wall -Wno-pointer-sign".split(' ').map(String::from));
        let mut c = self.clang();
        c.args(v).args(flags);
        c
    }

    fn cxxc(&self, flags: &[String]) -> Command {
        let mut v = self.common();
        v.push("-nostdinc++".into());
        self.isystem(&mut v, &self.cxx_dirs());
        v.extend(
            "-fexceptions -frtti -fasynchronous-unwind-tables -fvisibility-inlines-hidden -std=gnu++20".split(' ').map(String::from),
        );
        let mut c = self.clang();
        c.arg("--driver-mode=g++").args(v).args(flags);
        c
    }

    /// One source, by its extension.
    fn compile(&self, flags: &[String], src: &str, out: &Path) {
        let mut c = if is_cxx(src) {
            let mut f = vec!["-Wall".to_string()];
            f.extend_from_slice(flags);
            self.cxxc(&f)
        } else {
            self.cc(flags)
        };
        c.arg("-c").arg(src).arg("-o").arg(out);
        run(c);
    }

    fn assemble(&self, src: &Path, out: &Path) {
        let mut c = self.clang();
        c.arg(format!("--target={TARGET}")).arg("-c").arg(src).arg("-o").arg(out);
        run(c);
    }

    fn link(&self, rest: Vec<String>) {
        let mut c = Command::new(self.tool("RUST_LLD", "rust-lld"));
        let mut a = vec!["-shared".to_string(), "-T".into(), s(&self.ul.join("link64.ld"))];
        a.extend(
            "--no-rosegment -nostdlib -z now -z norelro -Bsymbolic --hash-style=sysv --gc-sections --eh-frame-hdr"
                .split(' ')
                .map(String::from),
        );
        a.extend(rest);
        c.args(["-flavor", "gnu"]);
        self.args(&mut c, a);
        run(c);
    }

    fn lib(&self, n: &str) -> PathBuf {
        self.libdir.join(n)
    }

    /// Are these built, and no source newer than the first?
    fn current(&self, built: &[PathBuf], sources: &[&str]) -> bool {
        let Some(t) = modified(&built[0]) else { return false };
        built.iter().all(|f| f.exists())
            && !sources.iter().any(|d| newer(&self.ul.join(d), t))
            && !newer(&self.root.join("tools/cc64/src"), t)
    }

    /// The C library: rebuilt when a source or header is newer than it.
    ///   libc.a        the library, static (with crt1.o)
    ///   LIBC64.DLL    the library as a DLL (C:\RS2\DLL64\LIBC64.DLL)
    ///   libc64.a      its import library: one stub per function,
    ///                 "jmp *LIBC64.name@GOTPCREL(%rip)"
    ///   crt1.o        a program's start-up code, for use with the DLL
    ///   crtdll.o      a DLL's start-up code (_DLL_InitTerm: constructors)
    fn build_libc(&self) {
        let (lib, implib, dll) = (self.lib("libc.a"), self.lib("libc64.a"), self.lib("LIBC64.DLL"));
        let built = [lib.clone(), implib.clone(), dll.clone(), self.lib("crt1.o"), self.lib("crtdll.o")];
        if self.current(&built, &["libc", "include", "link64.ld"]) {
            return;
        }
        let dirs = ["obj", "dll", "stubs"].map(|d| self.lib(d));
        for d in &dirs {
            let _ = fs::remove_dir_all(d);
            make_dir(d);
        }
        let [obj, dllobj, stubs] = dirs;
        let mut jobs = Vec::new();
        for f in files(&self.ul.join("libc"), &["c", "S"]) {
            let n = f.file_stem().unwrap_or_default().to_string_lossy().into_owned();
            let mut flags: Vec<String> =
                ["-ffreestanding", "-fno-builtin", "-D_RS2_BUILDING_LIBC"].map(String::from).to_vec();
            let out = if n == "crtdll" { self.lib("crtdll.o") } else { obj.join(format!("{n}.o")) };
            jobs.push((f.clone(), flags.clone(), out));
            // The DLL exports the library's functions and data (internal
            // ones are hidden in libc.h).
            if n != "crt1" && n != "crtdll" {
                flags.push("-fvisibility=default".into());
                jobs.push((f, flags, dllobj.join(format!("{n}.o"))));
            }
        }
        parallel(&jobs, |(f, flags, out)| {
            let mut c = self.cc(flags);
            c.arg("-Werror").arg("-c").arg(f).arg("-o").arg(out);
            status(c, "clang").success()
        });
        copy(&obj.join("crt1.o"), &self.lib("crt1.o"));
        let _ = fs::remove_file(&implib);
        let start = self.lib("dllstart.s");
        write(&start, ".text\n.globl _start\n_start: ret\n");
        self.assemble(&start, &self.lib("dllstart.o"));
        let mut a = vec!["-soname=LIBC64".to_string(), format!("--defsym=__image_base={LIBC64_BASE}"), s(&self.lib("dllstart.o"))];
        a.extend(files(&dllobj, &["o"]).iter().map(|p| s(p)));
        a.extend(["-o".into(), s(&dll)]);
        self.link(a);
        check_imports(&dll, &[]);
        // The import library: a stub for each exported function (in its own
        // member, so a program takes only what it calls). Data is imported
        // through the headers' asm labels instead.
        let mut names: Vec<String> = symbols(&dll)
            .into_iter()
            .filter(|y| y.global_code && !y.name.starts_with("_rs2_") && y.name != "_start")
            .map(|y| y.name)
            .collect();
        names.sort();
        write(&self.lib("exports.txt"), names.iter().map(|n| format!("{n}\n")).collect::<String>());
        // (Numbered: _exit and _Exit are one file to some file systems.)
        let mut srcs = Vec::new();
        for (i, f) in names.iter().enumerate() {
            let n = format!("./{}.s", i + 1);
            write(
                &stubs.join(&n[2..]),
                format!(
                    ".section .text.{f},\"ax\",@progbits\n.globl {f}\n.hidden {f}\n.type {f},@function\n{f}:\n    jmp *\"LIBC64.{f}\"@GOTPCREL(%rip)\n"
                ),
            );
            srcs.push(n);
        }
        let batches: Vec<&[String]> = srcs.chunks(50).collect();
        parallel(&batches, |b| {
            let mut c = self.clang();
            c.current_dir(&stubs).arg(format!("--target={TARGET}")).arg("-c").args(b.iter());
            status(c, "clang").success()
        });
        self.ar(&implib, &files(&stubs, &["o"]));
        // Last: what `current` looks at.
        self.ar(&lib, &files(&obj, &["o"]));
    }

    /// The C++ library, for programs that use LIBC64.DLL or (static) libc.a:
    /// the two differ in how the C library's data is declared.
    ///   LIBCXX64.DLL    libc++, libc++abi and libunwind (C:\RS2\DLL64)
    ///   libcxx64s.a     the same, to link in
    fn build_libcxx(&self, is_static: bool) -> PathBuf {
        let out = self.lib(if is_static { "libcxx64s.a" } else { "LIBCXX64.DLL" });
        if self.current(std::slice::from_ref(&out), &["cxx", "include", "link64.ld"]) {
            return out;
        }
        let o = self.lib(if is_static { "cxxobjs" } else { "cxxobj" });
        let _ = fs::remove_dir_all(&o);
        make_dir(&o);
        // In the DLL the annotated interface is exported.
        let d = if is_static {
            "-D_LIBCXXABI_DISABLE_VISIBILITY_ANNOTATIONS -D_LIBUNWIND_DISABLE_VISIBILITY_ANNOTATIONS"
        } else {
            "-D_RS2_LIBC_DLL"
        };
        let cf = format!("-ffunction-sections -fdata-sections -DNDEBUG -w {d}");
        let l = &self.llvm;
        let inc = |dirs: &[&str]| -> Vec<String> { dirs.iter().map(|d| format!("-I{}", s(&l.join(d)))).collect() };
        let flags = |f: &str, dirs: &[&str]| -> Vec<String> {
            cf.split(' ').chain(f.split(' ')).map(String::from).chain(inc(dirs)).collect()
        };
        let mut jobs: Vec<(PathBuf, String, Vec<String>)> = Vec::new();
        let cxx = flags(
            "-std=c++23 -D_LIBCPP_BUILDING_LIBRARY -DLIBCXX_BUILDING_LIBCXXABI -D_LIBCPP_HAS_CLOCK_GETTIME \
             -DLIBC_NAMESPACE=__llvm_libc_common_utils",
            &["libcxx/src", "libc"],
        );
        let abi = flags(
            "-std=c++23 -D_LIBCXXABI_BUILDING_LIBRARY -D_LIBCPP_BUILDING_LIBRARY -DLIBCXXABI_USE_LLVM_UNWINDER",
            &["libcxxabi/src", "libcxx/src", "libunwind/include"],
        );
        let (u, ud) = ("-D_LIBUNWIND_IS_BAREMETAL", ["libunwind/include", "libunwind/src"]);
        let stem = |f: &Path| f.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        for dir in ["libcxx/src", "libcxx/src/ryu", "libcxx/src/filesystem"] {
            for f in files(&l.join(dir), &["cpp"]) {
                if dir != "libcxx/src" || stem(&f) != "new" {
                    jobs.push((f.clone(), format!("cxx-{}.o", stem(&f)), cxx.clone()));
                }
            }
        }
        for f in files(&l.join("libcxxabi/src"), &["cpp"]) {
            if !["cxa_noexception", "cxa_thread_atexit"].contains(&stem(&f).as_str()) {
                jobs.push((f.clone(), format!("abi-{}.o", stem(&f)), abi.clone()));
            }
        }
        let mut f = flags(u, &ud);
        f.extend("-std=c++17 -fno-exceptions -fno-rtti".split(' ').map(String::from));
        jobs.push((l.join("libunwind/src/libunwind.cpp"), "unw-libunwind.o".into(), f));
        let mut f = flags(u, &ud);
        f.extend("-fexceptions -fasynchronous-unwind-tables".split(' ').map(String::from));
        for n in ["UnwindLevel1.c", "UnwindLevel1-gcc-ext.c", "UnwindRegistersRestore.S", "UnwindRegistersSave.S"] {
            let p = l.join("libunwind/src").join(n);
            jobs.push((p.clone(), format!("unw-{}.o", stem(&p)), f.clone()));
        }
        parallel(&jobs, |(src, name, flags)| {
            let mut c = if is_cxx(&s(src)) { self.cxxc(flags) } else { self.cc(flags) };
            c.arg("-c").arg(src).arg("-o").arg(o.join(name));
            status(c, "clang").success()
        });
        let objs = files(&o, &["o"]);
        if is_static {
            self.ar(&out, &objs);
        } else {
            let _ = fs::remove_file(&out);
            let mut a = vec!["-soname=LIBCXX64".to_string(), format!("--defsym=__image_base={LIBCXX64_BASE}")];
            a.extend([s(&self.lib("dllstart.o")), s(&self.lib("crtdll.o"))]);
            a.extend(objs.iter().map(|p| s(p)));
            a.extend([s(&self.lib("libc64.a")), "-o".into(), s(&out)]);
            self.link(a);
            check_imports(&out, &[]);
        }
        out
    }
}

struct Symbol {
    name: String,
    defined: bool,
    weak: bool,
    /// A defined, global (not weak) symbol in code.
    global_code: bool,
}

/// An ELF64 file's dynamic symbols.
fn symbols(f: &Path) -> Vec<Symbol> {
    let d = fs::read(f).unwrap_or_else(|e| die(format_args!("{}: {e}", f.display())));
    symbols_of(&d).unwrap_or_else(|| die(format_args!("{}: not a 64-bit ELF module", f.display())))
}

fn symbols_of(d: &[u8]) -> Option<Vec<Symbol>> {
    let u16_at = |o: usize| Some(u16::from_le_bytes(d.get(o..o + 2)?.try_into().ok()?) as usize);
    let u32_at = |o: usize| Some(u32::from_le_bytes(d.get(o..o + 4)?.try_into().ok()?) as usize);
    let u64_at = |o: usize| Some(u64::from_le_bytes(d.get(o..o + 8)?.try_into().ok()?) as usize);
    if d.get(..5)? != b"\x7fELF\x02" {
        return None;
    }
    let (shoff, shentsize, shnum) = (u64_at(0x28)?, u16_at(0x3a)?, u16_at(0x3c)?);
    let mut v = Vec::new();
    for i in 0..shnum {
        let sh = shoff + i * shentsize;
        const SHT_DYNSYM: usize = 11;
        if u32_at(sh + 4)? != SHT_DYNSYM {
            continue;
        }
        let (off, size) = (u64_at(sh + 0x18)?, u64_at(sh + 0x20)?);
        let strings = u64_at(shoff + u32_at(sh + 0x28)? * shentsize + 0x18)?;
        for y in (off + 24..off + size).step_by(24) {
            let at = strings + u32_at(y)?;
            let n = d.get(at..)?.iter().position(|&c| c == 0)?;
            let (bind, shndx) = (d.get(y + 4)? >> 4, u16_at(y + 6)?);
            const SHF_EXECINSTR: usize = 4;
            let code = shndx != 0 && shndx < shnum && u64_at(shoff + shndx * shentsize + 8)? & SHF_EXECINSTR != 0;
            v.push(Symbol {
                name: String::from_utf8_lossy(&d[at..at + n]).into_owned(),
                defined: shndx != 0,
                weak: bind == 2,
                global_code: code && bind == 1,
            });
        }
    }
    Some(v)
}

/// Imports are named MODULE.name or exported by a DLL linked against:
/// anything else undefined cannot load.
fn check_imports(f: &Path, dlls: &[String]) {
    let has: Vec<String> =
        dlls.iter().flat_map(|d| symbols(Path::new(d))).filter(|y| y.defined).map(|y| y.name).collect();
    let bad: Vec<String> = symbols(f)
        .into_iter()
        .filter(|y| !y.defined && !y.weak && !has.contains(&y.name) && (!y.name.contains('.') || y.name.starts_with("__emutls_")))
        .map(|y| y.name)
        .collect();
    if !bad.is_empty() {
        eprintln!("cc64: {}: undefined:", f.display());
        for b in bad {
            eprintln!("    {b}");
        }
        let _ = fs::remove_file(f);
        exit(1);
    }
}

/// A DLL's name for DT_NEEDED: its file name without .DLL, in upper case.
fn soname(f: &str) -> String {
    let n = Path::new(f).file_name().unwrap_or(OsStr::new("")).to_string_lossy().to_ascii_uppercase();
    n.strip_suffix(".DLL").unwrap_or(&n).to_string()
}

fn main() {
    // main's own frame must end for the temporary directory to go.
    exit(build())
}

fn build() -> i32 {
    let mut args = std::env::args().skip(1);
    let (mut flags, mut out, mut rc) = (None, None, None);
    let (mut pm, mut dll, mut compile_only, mut libc_only, mut libcxx_only, mut is_static, mut cxx) = (false, false, false, false, false, false, false);
    let (mut cflags, mut inputs): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    while let Some(a) = args.next() {
        let mut value = || Some(args.next().unwrap_or_else(|| usage()));
        match a.as_str() {
            "-o" => out = value(),
            "-c" => compile_only = true,
            "--pm" => pm = true,
            "--dll" => dll = true,
            "--rc" => rc = value(),
            "--flags" => flags = value(),
            "--libc" => libc_only = true,
            "--libcxx" => libcxx_only = true,
            "--cxx" => cxx = true,
            "--static" => is_static = true,
            f if is_cxx(f) => {
                cxx = true;
                inputs.push(a);
            }
            f if [".c", ".o", ".a", ".s", ".S"].iter().any(|x| f.ends_with(x)) || is_dll(f) => inputs.push(a),
            _ => cflags.push(a),
        }
    }
    let root = match std::env::var_os("CC64_ROOT") {
        Some(r) => PathBuf::from(r),
        None => match option_env!("CARGO_MANIFEST_DIR") {
            Some(d) => Path::new(d).parent().and_then(Path::parent).unwrap_or(Path::new(".")).to_path_buf(),
            None => die("CC64_ROOT is not set"),
        },
    };
    let ul = root.join("userland64");
    let env = |v: &str| std::env::var(v).ok().filter(|v| !v.is_empty());
    let tmp = Temp(std::env::temp_dir().join(format!("cc64.{}", std::process::id())));
    let t = Tools {
        llvm: ul.join("cxx/llvm"),
        ul,
        libdir: env("LIBC64_DIR").map_or_else(|| root.join("target/libc64"), PathBuf::from),
        root,
        clang: env("CLANG").unwrap_or("clang".into()).split_whitespace().map(String::from).collect(),
        tmp: tmp.0.clone(),
        sysroot_bin: Default::default(),
    };
    if let Some(f) = flags {
        let mut v = t.common();
        if f == "C" {
            v.extend(["-isystem".into(), s(&t.ul.join("include"))]);
        } else {
            v.push("-nostdinc++".into());
            t.isystem(&mut v, &t.cxx_dirs());
        }
        v.extend("-D_RS2_LIBC_DLL -fexceptions -fasynchronous-unwind-tables".split(' ').map(String::from));
        if f != "C" {
            v.push("-fvisibility-inlines-hidden".into());
        }
        println!("{}", v.join(" "));
        return 0;
    }
    make_dir(&tmp.0);
    t.build_libc();
    if libc_only {
        return 0;
    }
    if libcxx_only {
        t.build_libcxx(is_static);
        return 0;
    }
    if !is_static {
        cflags.insert(0, "-D_RS2_LIBC_DLL".into());
    }
    if compile_only {
        if inputs.is_empty() || (out.is_some() && inputs.len() > 1) {
            usage();
        }
        for f in &inputs {
            let o = match &out {
                Some(o) => PathBuf::from(o),
                None => Path::new(Path::new(f).file_stem().unwrap_or_default()).with_extension("o"),
            };
            t.compile(&cflags, f, &o);
        }
        return 0;
    }
    let Some(out) = out else { usage() };
    if inputs.is_empty() {
        usage();
    }
    let (mut objs, mut dlls) = (Vec::new(), Vec::new());
    for (i, f) in inputs.iter().enumerate() {
        if f.ends_with(".o") || f.ends_with(".a") {
            objs.push(f.clone());
        } else if is_dll(f) {
            objs.push(f.clone());
            dlls.push(f.clone());
        } else {
            let o = t.tmp.join(format!("{}.o", i + 1));
            t.compile(&cflags, f, &o);
            objs.push(s(&o));
        }
    }
    // The program type and the resources: data symbols the loader reads.
    let mut meta = String::new();
    if pm {
        meta += ".section .rodata\n.globl RS2_APPTYPE\n.type RS2_APPTYPE,@object\nRS2_APPTYPE: .long 3\n";
    }
    if let Some(rc) = rc {
        let res = t.tmp.join("res.bin");
        let rc = std::env::current_dir().map_or(PathBuf::from(&rc), |d| d.join(&rc));
        let mut c = Command::new(std::env::var_os("CARGO").unwrap_or("cargo".into()));
        c.current_dir(&t.root).args("run -q --release -p os2-rc --features host --example rc -- table64".split(' ')).arg(rc).arg(&res);
        run(c);
        meta += ".section .rodata\n.globl RS2_RESOURCES\n.type RS2_RESOURCES,@object\n.p2align 2\n";
        meta += &format!("RS2_RESOURCES: .incbin \"{}\"\n", s(&res).replace('\\', "/"));
    }
    if dll {
        // The ELF entry point, unused for a DLL (crtdll.o's _DLL_InitTerm
        // is its initialisation).
        meta += ".text\n.globl _start\n_start: ret\n";
    }
    let (ms, mo) = (t.tmp.join("meta.s"), t.tmp.join("meta.o"));
    write(&ms, meta);
    t.assemble(&ms, &mo);
    let mut libs = vec![s(&t.lib(if is_static { "libc.a" } else { "libc64.a" }))];
    if !is_static && !dll {
        // A program's own start-up code.
        objs.insert(0, s(&t.lib("crt1.o")));
    }
    if cxx {
        let l = s(&t.build_libcxx(is_static));
        if !is_static {
            dlls.push(l.clone());
        }
        libs.insert(0, l);
    }
    // What is linked in from an archive is not exported.
    let mut a: Vec<String> = ["--exclude-libs=ALL", "--as-needed"].map(String::from).to_vec();
    if dll {
        a.push(format!("-soname={}", soname(&out)));
        // A DLL's: _DLL_InitTerm runs its constructors.
        objs.insert(0, s(&t.lib("crtdll.o")));
    }
    a.extend(["--export-dynamic-symbol=RS2_APPTYPE", "--export-dynamic-symbol=RS2_RESOURCES"].map(String::from));
    a.push(s(&mo));
    a.extend(objs);
    a.extend(libs);
    a.extend(["-o".into(), out.clone()]);
    t.link(a);
    check_imports(Path::new(&out), &dlls);
    0
}
