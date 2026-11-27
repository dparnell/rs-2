//! W32KERN's calls (KERNEL32) the C runtime is built on.

#![allow(dead_code)]

unsafe extern "stdcall" {
    #[link_name = "W32KERN.CreateFileA"]
    pub fn CreateFileA(name: *const u8, access: u32, share: u32, sa: *const u32, disp: u32, flags: u32, tmpl: u32) -> u32;
    #[link_name = "W32KERN.ReadFile"]
    pub fn ReadFile(h: u32, buf: *mut u8, n: u32, read: *mut u32, ov: u32) -> i32;
    #[link_name = "W32KERN.WriteFile"]
    pub fn WriteFile(h: u32, buf: *const u8, n: u32, written: *mut u32, ov: u32) -> i32;
    #[link_name = "W32KERN.CloseHandle"]
    pub fn CloseHandle(h: u32) -> i32;
    #[link_name = "W32KERN.SetFilePointer"]
    pub fn SetFilePointer(h: u32, lo: i32, hi: *mut i32, method: u32) -> u32;
    #[link_name = "W32KERN.GetFileType"]
    pub fn GetFileType(h: u32) -> u32;
    #[link_name = "W32KERN.GetFileSize"]
    pub fn GetFileSize(h: u32, hi: *mut u32) -> u32;
    #[link_name = "W32KERN.SetEndOfFile"]
    pub fn SetEndOfFile(h: u32) -> i32;
    #[link_name = "W32KERN.FlushFileBuffers"]
    pub fn FlushFileBuffers(h: u32) -> i32;
    #[link_name = "W32KERN.GetStdHandle"]
    pub fn GetStdHandle(n: u32) -> u32;
    #[link_name = "W32KERN.DuplicateHandle"]
    pub fn DuplicateHandle(sp: u32, h: u32, tp: u32, out: *mut u32, access: u32, inherit: i32, options: u32) -> i32;
    #[link_name = "W32KERN.GetCurrentProcess"]
    pub fn GetCurrentProcess() -> u32;
    #[link_name = "W32KERN.GetCommandLineA"]
    pub fn GetCommandLineA() -> *const u8;
    #[link_name = "W32KERN.GetCommandLineW"]
    pub fn GetCommandLineW() -> *const u16;
    #[link_name = "W32KERN.GetEnvironmentStringsA"]
    pub fn GetEnvironmentStringsA() -> *const u8;
    #[link_name = "W32KERN.SetEnvironmentVariableA"]
    pub fn SetEnvironmentVariableA(name: *const u8, value: *const u8) -> i32;
    #[link_name = "W32KERN.ExitProcess"]
    pub fn ExitProcess(code: u32) -> !;
    #[link_name = "W32KERN.CreateThread"]
    pub fn CreateThread(sa: u32, stack: u32, f: u32, arg: u32, flags: u32, tid: *mut u32) -> u32;
    #[link_name = "W32KERN.ExitThread"]
    pub fn ExitThread(code: u32) -> !;
    #[link_name = "W32KERN.GetLastError"]
    pub fn GetLastError() -> u32;
    #[link_name = "W32KERN.SetLastError"]
    pub fn SetLastError(e: u32);
    #[link_name = "W32KERN.GetFileAttributesA"]
    pub fn GetFileAttributesA(name: *const u8) -> u32;
    #[link_name = "W32KERN.GetFileAttributesExA"]
    pub fn GetFileAttributesExA(name: *const u8, level: u32, info: *mut u32) -> i32;
    #[link_name = "W32KERN.SetFileAttributesA"]
    pub fn SetFileAttributesA(name: *const u8, attr: u32) -> i32;
    #[link_name = "W32KERN.GetFileInformationByHandle"]
    pub fn GetFileInformationByHandle(h: u32, info: *mut u32) -> i32;
    #[link_name = "W32KERN.DeleteFileA"]
    pub fn DeleteFileA(name: *const u8) -> i32;
    #[link_name = "W32KERN.MoveFileA"]
    pub fn MoveFileA(from: *const u8, to: *const u8) -> i32;
    #[link_name = "W32KERN.CreateDirectoryA"]
    pub fn CreateDirectoryA(name: *const u8, sa: u32) -> i32;
    #[link_name = "W32KERN.RemoveDirectoryA"]
    pub fn RemoveDirectoryA(name: *const u8) -> i32;
    #[link_name = "W32KERN.GetCurrentDirectoryA"]
    pub fn GetCurrentDirectoryA(n: u32, buf: *mut u8) -> u32;
    #[link_name = "W32KERN.SetCurrentDirectoryA"]
    pub fn SetCurrentDirectoryA(path: *const u8) -> i32;
    #[link_name = "W32KERN.GetFullPathNameA"]
    pub fn GetFullPathNameA(name: *const u8, n: u32, buf: *mut u8, part: *mut *mut u8) -> u32;
    #[link_name = "W32KERN.FindFirstFileA"]
    pub fn FindFirstFileA(spec: *const u8, fd: *mut u8) -> u32;
    #[link_name = "W32KERN.FindNextFileA"]
    pub fn FindNextFileA(h: u32, fd: *mut u8) -> i32;
    #[link_name = "W32KERN.FindClose"]
    pub fn FindClose(h: u32) -> i32;
    #[link_name = "W32KERN.GetSystemTimeAsFileTime"]
    pub fn GetSystemTimeAsFileTime(ft: *mut u64);
    #[link_name = "W32KERN.GetTimeZoneInformation"]
    pub fn GetTimeZoneInformation(tz: *mut u8) -> u32;
    #[link_name = "W32KERN.GetTickCount"]
    pub fn GetTickCount() -> u32;
    #[link_name = "W32KERN.CreateProcessA"]
    pub fn CreateProcessA(app: *const u8, cl: *const u8, psa: u32, tsa: u32, inherit: i32, flags: u32, env: *const u8, cwd: *const u8, si: *const u32, pi: *mut u32) -> i32;
    #[link_name = "W32KERN.WaitForSingleObject"]
    pub fn WaitForSingleObject(h: u32, ms: u32) -> u32;
    #[link_name = "W32KERN.GetExitCodeProcess"]
    pub fn GetExitCodeProcess(h: u32, code: *mut u32) -> i32;
    #[link_name = "W32KERN.Sleep"]
    pub fn Sleep(ms: u32);
    #[link_name = "W32KERN.MultiByteToWideChar"]
    pub fn MultiByteToWideChar(cp: u32, flags: u32, s: *const u8, n: i32, d: *mut u16, size: i32) -> i32;
    #[link_name = "W32KERN.WideCharToMultiByte"]
    pub fn WideCharToMultiByte(cp: u32, flags: u32, s: *const u16, n: i32, d: *mut u8, size: i32, dflt: *const u8, used: *mut i32) -> i32;
    #[link_name = "W32KERN.GetACP"]
    pub fn GetACP() -> u32;
    #[link_name = "W32KERN.ReadConsoleInputA"]
    pub fn ReadConsoleInputA(h: u32, rec: *mut u8, n: u32, read: *mut u32) -> i32;
    #[link_name = "W32KERN.GetNumberOfConsoleInputEvents"]
    pub fn GetNumberOfConsoleInputEvents(h: u32, n: *mut u32) -> i32;
    #[link_name = "W32KERN.GetCurrentThreadId"]
    pub fn GetCurrentThreadId() -> u32;
    #[link_name = "W32KERN.GetCurrentProcessId"]
    pub fn GetCurrentProcessId() -> u32;
    #[link_name = "W32KERN.FileTimeToLocalFileTime"]
    pub fn FileTimeToLocalFileTime(ft: *const u64, local: *mut u64) -> i32;
    #[link_name = "W32KERN.GetTempPathA"]
    pub fn GetTempPathA(n: u32, buf: *mut u8) -> u32;
    #[link_name = "W32KERN.GetModuleFileNameA"]
    pub fn GetModuleFileNameA(h: u32, buf: *mut u8, n: u32) -> u32;
    #[link_name = "W32KERN.TerminateProcess"]
    pub fn TerminateProcess(h: u32, code: u32) -> i32;
    #[link_name = "W32KERN.OutputDebugStringA"]
    pub fn OutputDebugStringA(s: *const u8);
    #[link_name = "W32KERN.GetLocalTime"]
    pub fn GetLocalTime(st: *mut u16);
}
