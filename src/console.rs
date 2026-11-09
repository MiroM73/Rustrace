//! Messages of the runs without a window (`--export`, `--help` / `--version` text, invalid
//! command line) on the console the program was started from (#62, port of MMPortLens#40).
//!
//! The release build is a GUI program: [`attach_console`] attaches it to the console of `cmd` /
//! PowerShell. Where the text goes decides its encoding ([`Target`]):
//! - a **console**: Rust's standard output already writes UTF-16 with `WriteConsoleW`, so every
//!   character is right whatever the code page is;
//! - a **pipe read by PowerShell** (`| …`, `2>&1`; the parent process is `powershell.exe` or
//!   `pwsh.exe`): PowerShell decodes the bytes of a native program with
//!   `[Console]::OutputEncoding`, which is the output code page of the console (OEM 852 / 437 by
//!   default, 65001 when the user switched to UTF-8). UTF-8 bytes would turn `–` into `ÔÇô`, so
//!   the text is written in that code page (characters it lacks get their closest look-alike,
//!   `–` → `-`);
//! - a **file**, or a pipe of another program (bash in Windows Terminal also has an OEM console,
//!   but `grep` expects UTF-8), or no console at all (a task): UTF-8 as before.
//!
//! The console's code page is never changed, so nothing has to be restored when the program ends
//! (also not after `process::exit` or Ctrl+C).

use std::io::Write;

// `Target`, `target` and `decodes_with_console_cp` are used only by the Windows branch of
// `handle_target` (and the tests), hence the `allow(dead_code)` on other platforms.

/// Where a standard handle writes, as far as the encoding is concerned.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    /// A console window: the standard library writes UTF-16.
    Console,
    /// Bytes in this (non-UTF-8) code page.
    CodePage(u32),
    /// UTF-8 bytes.
    Utf8,
}

#[cfg_attr(not(windows), allow(dead_code))]
const CP_UTF8: u32 = 65001;

/// The encoding of a handle: `console` = the handle is a console, `pipe` = it is a pipe,
/// `console_cp` = the output code page of the attached console (0 without one), `powershell` =
/// the parent process decodes with that code page ([`decodes_with_console_cp`]).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn target(
    console: bool,
    pipe: bool,
    console_cp: u32,
    powershell: impl FnOnce() -> bool,
) -> Target {
    if console {
        Target::Console
    } else if pipe && console_cp != 0 && console_cp != CP_UTF8 && powershell() {
        Target::CodePage(console_cp)
    } else {
        Target::Utf8
    }
}

/// Whether a parent process of this name decodes the bytes of a pipe with the console's code
/// page: Windows PowerShell and PowerShell 7 (`[Console]::OutputEncoding`).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn decodes_with_console_cp(parent_exe: &str) -> bool {
    let name = parent_exe.rsplit(['\\', '/']).next().unwrap_or(parent_exe);
    ["powershell.exe", "pwsh.exe", "powershell_ise.exe"]
        .iter()
        .any(|n| name.eq_ignore_ascii_case(n))
}

/// The executable of the parent of process `me` in a process list of (PID, parent PID,
/// executable); `None` when unknown.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn parent_of(list: &[(u32, u32, String)], me: u32) -> Option<String> {
    let parent = list.iter().find(|e| e.0 == me)?.1;
    list.iter().find(|e| e.0 == parent).map(|e| e.2.clone())
}

/// Lets a program without a console (the release build) print to the console it was started
/// from (`cmd`, PowerShell); does nothing when there is none.
pub fn attach_console() {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn AttachConsole(process: u32) -> i32;
        }
        const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
        // SAFETY: no pointers; failing (no parent console, or a console already) is fine.
        unsafe {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

/// `println!` for the console messages: the line in the encoding its target needs.
pub fn out(text: &str) {
    write_line(false, text);
}

/// `eprintln!` for the console messages.
pub fn err(text: &str) {
    write_line(true, text);
}

fn write_line(stderr: bool, text: &str) {
    let line = format!("{text}\n");
    let bytes = match handle_target(stderr) {
        Target::Console | Target::Utf8 => line.into_bytes(),
        Target::CodePage(cp) => encode(&line, cp).unwrap_or_else(|| line.into_bytes()),
    };
    // A closed pipe (`| select -First 1`) is not an error worth a panic.
    let _ = if stderr {
        std::io::stderr().lock().write_all(&bytes)
    } else {
        std::io::stdout().lock().write_all(&bytes)
    };
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn GetStdHandle(which: u32) -> *mut c_void;
        pub fn GetConsoleMode(handle: *mut c_void, mode: *mut u32) -> i32;
        pub fn GetFileType(handle: *mut c_void) -> u32;
        pub fn GetConsoleOutputCP() -> u32;
        pub fn GetCurrentProcessId() -> u32;
        pub fn WideCharToMultiByte(
            code_page: u32,
            flags: u32,
            wide: *const u16,
            wide_len: i32,
            out: *mut u8,
            out_len: i32,
            default_char: *const u8,
            used_default: *mut i32,
        ) -> i32;
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> *mut c_void;
        fn Process32FirstW(snapshot: *mut c_void, entry: *mut ProcessEntry) -> i32;
        fn Process32NextW(snapshot: *mut c_void, entry: *mut ProcessEntry) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    pub const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    pub const STD_ERROR_HANDLE: u32 = -12i32 as u32;
    pub const FILE_TYPE_PIPE: u32 = 3;
    const TH32CS_SNAPPROCESS: u32 = 0x2;

    /// `PROCESSENTRY32W`.
    #[repr(C)]
    struct ProcessEntry {
        size: u32,
        usage: u32,
        pid: u32,
        heap_id: usize,
        module_id: u32,
        threads: u32,
        parent_pid: u32,
        priority: i32,
        flags: u32,
        exe: [u16; 260],
    }

    /// (PID, parent PID, executable name) of every running process (Toolhelp snapshot).
    fn process_list() -> Vec<(u32, u32, String)> {
        let mut list = Vec::new();
        // SAFETY: the snapshot handle is checked and closed; `entry` is a local with its size set
        // as the API requires, `exe` is NUL-terminated by Windows.
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap.is_null() || snap as isize == -1 {
                return list;
            }
            let mut entry: ProcessEntry = std::mem::zeroed();
            entry.size = std::mem::size_of::<ProcessEntry>() as u32;
            let mut ok = Process32FirstW(snap, &mut entry);
            while ok != 0 {
                let len = entry.exe.iter().position(|&c| c == 0).unwrap_or(260);
                let exe = String::from_utf16_lossy(&entry.exe[..len]);
                list.push((entry.pid, entry.parent_pid, exe));
                ok = Process32NextW(snap, &mut entry);
            }
            CloseHandle(snap);
        }
        list
    }

    /// The executable name of the program that started this one; `None` when unknown.
    pub fn parent_exe() -> Option<String> {
        // SAFETY: no arguments.
        super::parent_of(&process_list(), unsafe { GetCurrentProcessId() })
    }
}

#[cfg(windows)]
fn handle_target(stderr: bool) -> Target {
    let which = if stderr {
        win::STD_ERROR_HANDLE
    } else {
        win::STD_OUTPUT_HANDLE
    };
    // SAFETY: GetStdHandle takes no pointers; the handle is only queried, `mode` is a local.
    unsafe {
        let handle = win::GetStdHandle(which);
        if handle.is_null() || handle as isize == -1 {
            return Target::Utf8;
        }
        let mut mode = 0u32;
        let console = win::GetConsoleMode(handle, &mut mode) != 0;
        let pipe = win::GetFileType(handle) == win::FILE_TYPE_PIPE;
        target(console, pipe, win::GetConsoleOutputCP(), || {
            win::parent_exe().is_some_and(|p| decodes_with_console_cp(&p))
        })
    }
}

#[cfg(not(windows))]
fn handle_target(_stderr: bool) -> Target {
    Target::Utf8
}

/// `text` in the code page `cp` (best fit for missing characters); `None` when Windows cannot.
#[cfg(windows)]
pub fn encode(text: &str, cp: u32) -> Option<Vec<u8>> {
    let wide: Vec<u16> = text.encode_utf16().collect();
    if wide.is_empty() {
        return Some(Vec::new());
    }
    let len = i32::try_from(wide.len()).ok()?;
    // SAFETY: `wide` holds `len` UTF-16 units; the first call only measures (null output), the
    // second writes at most `need` bytes into a buffer of that size.
    unsafe {
        let need = win::WideCharToMultiByte(
            cp,
            0,
            wide.as_ptr(),
            len,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
            std::ptr::null_mut(),
        );
        if need <= 0 {
            return None;
        }
        let mut out = vec![0u8; need as usize];
        let written = win::WideCharToMultiByte(
            cp,
            0,
            wide.as_ptr(),
            len,
            out.as_mut_ptr(),
            need,
            std::ptr::null(),
            std::ptr::null_mut(),
        );
        (written > 0).then(|| {
            out.truncate(written as usize);
            out
        })
    }
}

#[cfg(not(windows))]
pub fn encode(_text: &str, _cp: u32) -> Option<Vec<u8>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_by_handle_kind() {
        let ps = || true;
        // A console wins whatever the code page (WriteConsoleW).
        assert_eq!(target(true, false, 852, ps), Target::Console);
        // A pipe read by PowerShell: the console's code page.
        assert_eq!(target(false, true, 852, ps), Target::CodePage(852));
        assert_eq!(target(false, true, 437, ps), Target::CodePage(437));
        // A pipe of another program (bash, grep), console switched to UTF-8, no console (task),
        // or a file: UTF-8.
        assert_eq!(target(false, true, 852, || false), Target::Utf8);
        assert_eq!(target(false, true, CP_UTF8, ps), Target::Utf8);
        assert_eq!(target(false, true, 0, ps), Target::Utf8);
        assert_eq!(target(false, false, 852, ps), Target::Utf8);
    }

    #[test]
    fn parent_in_the_process_list() {
        let list = vec![
            (10, 1, "powershell.exe".to_string()),
            (30, 10, "rustrace.exe".to_string()),
        ];
        assert_eq!(parent_of(&list, 30).as_deref(), Some("powershell.exe"));
        assert_eq!(parent_of(&list, 10), None);
        assert_eq!(parent_of(&list, 99), None);
    }

    #[test]
    fn powershell_parents() {
        assert!(decodes_with_console_cp("powershell.exe"));
        assert!(decodes_with_console_cp("PWSH.EXE"));
        assert!(decodes_with_console_cp(
            r"C:\Program Files\PowerShell\pwsh.exe"
        ));
        assert!(!decodes_with_console_cp("bash.exe"));
        assert!(!decodes_with_console_cp("cmd.exe"));
        assert!(!decodes_with_console_cp("mypowershell.exe.bak"));
    }

    #[cfg(windows)]
    #[test]
    fn encodes_in_the_oem_code_page() {
        // Slovak letters exist in 852; the en dash does not and becomes a hyphen.
        assert_eq!(
            encode("á č ž – ô", 852).unwrap(),
            vec![0xA0, b' ', 0x9F, b' ', 0xA7, b' ', b'-', b' ', 0x93]
        );
        // German umlauts in 437.
        assert_eq!(encode("ü", 437).unwrap(), vec![0x81]);
        assert_eq!(encode("", 852).unwrap(), Vec::<u8>::new());
    }
}
