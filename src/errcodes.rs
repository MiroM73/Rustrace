//! Error code lookup (similar to "Error Lookup" in CMTrace).
//!
//! Description sources:
//! 1. the language files – keys `code.0xXXXXXXXX` (common ConfigMgr, Windows Update,
//!    Windows Installer, WMI and Win32 codes; new codes can be added in a `.lang` file
//!    without recompiling),
//! 2. on Windows the system message via `FormatMessageW` (in the OS language),
//!    for WinINet codes (12xxx) also from `wininet.dll` / `winhttp.dll`.
//!
//! A code can be entered as hex (`0x80070005`, `80070005`), a negative decimal number
//! (`-2147024891`) or a Win32/MSI code (`1603`, `3010`).

use crate::{i18n, t};
use regex::Regex;
use std::sync::OnceLock;

#[derive(Clone, Debug)]
pub struct CodeInfo {
    /// The code as HRESULT (Win32 codes are converted to 0x8007xxxx).
    pub hresult: u32,
    /// The original value, if it was entered as a Win32/MSI code.
    pub win32: Option<u32>,
    pub facility: String,
    /// (source, description)
    pub descriptions: Vec<(String, String)>,
}

impl CodeInfo {
    pub fn hex(&self) -> String {
        format!("0x{:08X}", self.hresult)
    }

    pub fn signed(&self) -> i32 {
        self.hresult as i32
    }

    /// Short one-line description.
    pub fn short(&self) -> String {
        self.descriptions
            .first()
            .map(|(_, d)| d.clone())
            .unwrap_or_else(|| t!("code.unknown", facility = self.facility))
    }

    /// All forms of the code, e.g. "0x80070643 = -2147023293 = Win32 1603".
    pub fn forms(&self) -> String {
        let mut s = format!("{} = {}", self.hex(), self.signed());
        if let Some(w) = self.win32_code() {
            s.push_str(&format!(" = Win32 {w}"));
        }
        s
    }

    pub fn win32_code(&self) -> Option<u32> {
        self.win32.or(if self.hresult & 0xFFFF_0000 == 0x8007_0000 {
            Some(self.hresult & 0xFFFF)
        } else {
            None
        })
    }
}

/// Converts user input to a code number.
pub fn parse_code(input: &str) -> Option<u32> {
    let s = input.trim().trim_end_matches(['.', ',', ')', ';']);
    if s.is_empty() {
        return None;
    }
    if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return u32::from_str_radix(h, 16).ok();
    }
    if let Some(neg) = s.strip_prefix('-') {
        let v: i64 = neg.parse().ok()?;
        return i32::try_from(-v).ok().map(|x| x as u32);
    }
    // 8 hex digits starting with 8 (e.g. 80070005) = HRESULT without "0x".
    if s.len() == 8 && s.starts_with('8') && s.chars().all(|c| c.is_ascii_hexdigit()) {
        return u32::from_str_radix(s, 16).ok();
    }
    if s.chars().all(|c| c.is_ascii_hexdigit()) && !s.chars().all(|c| c.is_ascii_digit()) {
        return u32::from_str_radix(s, 16).ok();
    }
    s.parse::<u64>().ok().and_then(|v| u32::try_from(v).ok())
}

/// Finds error codes in a message text (max. 6, without duplicates).
pub fn find_codes(text: &str) -> Vec<u32> {
    static HEX: OnceLock<Regex> = OnceLock::new();
    static NEG: OnceLock<Regex> = OnceLock::new();
    static NAMED: OnceLock<Regex> = OnceLock::new();
    let hex = HEX.get_or_init(|| Regex::new(r"\b0[xX]([0-9A-Fa-f]{8})\b").unwrap());
    let neg = NEG.get_or_init(|| Regex::new(r"(?:^|[^\w-])(-\d{10})\b").unwrap());
    let named = NAMED.get_or_init(|| {
        Regex::new(
            r"(?i)\b(?:(?:exit|error|return|result|install(?:ation)?\s+(?:result|exit))\s*code|error\s+status)\s*(?:is|was|of|=|:)?\s*\(?\s*(-?\d{1,10})\b",
        )
        .unwrap()
    });

    let mut found: Vec<(usize, u32)> = Vec::new();
    for c in hex.captures_iter(text) {
        let m = c.get(1).unwrap();
        // Only failure HRESULTs (severity bit set) – skips addresses, content IDs etc.
        if let Ok(v) = u32::from_str_radix(m.as_str(), 16)
            && v & 0x8000_0000 != 0
        {
            found.push((m.start(), v));
        }
    }
    for c in neg.captures_iter(text) {
        let m = c.get(1).unwrap();
        if let Some(v) = parse_code(m.as_str())
            && v & 0x8000_0000 != 0
        {
            found.push((m.start(), v));
        }
    }
    for c in named.captures_iter(text) {
        let m = c.get(1).unwrap();
        if let Some(v) = parse_code(m.as_str())
            && v != 0
        {
            found.push((m.start(), v));
        }
    }
    found.sort_by_key(|(pos, _)| *pos);
    let mut out: Vec<u32> = Vec::new();
    for (_, v) in found {
        let norm = normalize(v);
        if !out.iter().any(|&o| normalize(o) == norm) {
            out.push(v);
        }
    }
    out.truncate(6);
    out
}

/// Converts Win32 codes (< 0x10000) to HRESULT 0x8007xxxx.
fn normalize(v: u32) -> u32 {
    if v != 0 && v < 0x1_0000 {
        0x8007_0000 | v
    } else {
        v
    }
}

/// Facility (area) of an HRESULT – technical names, not translated.
pub fn facility_name(hr: u32) -> String {
    if hr == 0 {
        return "S_OK".into();
    }
    let fac = (hr >> 16) & 0x7FF;
    let name = match fac {
        0 => "NULL (COM)",
        1 => "RPC",
        2 => "DISPATCH",
        3 => "STORAGE",
        4 => "ITF (COM / WMI)",
        7 => "WIN32",
        8 => "WINDOWS",
        9 => "SECURITY / SSPI",
        10 => "CONTROL",
        11 => "CERT",
        16 => "SCARD",
        23 => "SETUPAPI",
        25 => "HTTP",
        36 => "WINDOWSUPDATE",
        2000 => "ConfigMgr (SCCM)",
        _ => "",
    };
    if name.is_empty() {
        format!("facility {fac}")
    } else {
        format!("{name} (facility {fac})")
    }
}

pub fn lookup(value: u32) -> CodeInfo {
    let win32 = (value != 0 && value < 0x1_0000).then_some(value);
    let hr = normalize(value);
    let mut descriptions: Vec<(String, String)> = Vec::new();

    if value == 0 {
        descriptions.push((t!("code.source.description"), t!("code.success")));
    } else if let Some(d) = i18n::tr_opt(&format!("code.0x{hr:08X}")) {
        descriptions.push((t!("code.source.description"), d));
    }
    #[cfg(windows)]
    if let Some(m) = os::message(hr, win32) {
        descriptions.push((t!("code.source.windows"), m));
    }

    CodeInfo {
        hresult: hr,
        win32,
        facility: facility_name(hr),
        descriptions,
    }
}

#[cfg(windows)]
mod os {
    use std::ffi::c_void;

    const FORMAT_MESSAGE_IGNORE_INSERTS: u32 = 0x0000_0200;
    const FORMAT_MESSAGE_FROM_HMODULE: u32 = 0x0000_0800;
    const FORMAT_MESSAGE_FROM_SYSTEM: u32 = 0x0000_1000;
    const LOAD_LIBRARY_AS_DATAFILE: u32 = 0x0000_0002;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn FormatMessageW(
            flags: u32,
            source: *const c_void,
            message_id: u32,
            language_id: u32,
            buffer: *mut u16,
            size: u32,
            arguments: *const c_void,
        ) -> u32;
        fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    }

    fn format(code: u32, module: *const c_void) -> Option<String> {
        let mut buf = [0u16; 2048];
        let flags = FORMAT_MESSAGE_IGNORE_INSERTS
            | if module.is_null() {
                FORMAT_MESSAGE_FROM_SYSTEM
            } else {
                FORMAT_MESSAGE_FROM_HMODULE
            };
        // SAFETY: the buffer holds `buf.len()` characters, arguments are not used (IGNORE_INSERTS).
        let n = unsafe {
            FormatMessageW(
                flags,
                module,
                code,
                0,
                buf.as_mut_ptr(),
                buf.len() as u32,
                std::ptr::null(),
            )
        };
        if n == 0 {
            return None;
        }
        let s = String::from_utf16_lossy(&buf[..n as usize]);
        let s = s.trim().to_owned();
        (!s.is_empty()).then_some(s)
    }

    fn module(name: &str) -> *const c_void {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: the name is NUL-terminated; the library is loaded only as a data file (message resources).
        unsafe {
            LoadLibraryExW(
                wide.as_ptr(),
                std::ptr::null_mut(),
                LOAD_LIBRARY_AS_DATAFILE,
            )
        }
    }

    pub fn message(hr: u32, win32: Option<u32>) -> Option<String> {
        let w = win32.or((hr & 0xFFFF_0000 == 0x8007_0000).then_some(hr & 0xFFFF));
        if let Some(m) = format(hr, std::ptr::null()) {
            return Some(m);
        }
        if let Some(w) = w {
            if let Some(m) = format(w, std::ptr::null()) {
                return Some(m);
            }
            if (12000..=12200).contains(&w) {
                for dll in ["wininet.dll", "winhttp.dll"] {
                    let h = module(dll);
                    if !h.is_null()
                        && let Some(m) = format(w, h)
                    {
                        return Some(m);
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_forms() {
        assert_eq!(parse_code("0x80070005"), Some(0x8007_0005));
        assert_eq!(parse_code("80070005"), Some(0x8007_0005));
        assert_eq!(parse_code("-2147024891"), Some(0x8007_0005));
        assert_eq!(parse_code("1603"), Some(1603));
        assert_eq!(parse_code("87D00324"), Some(0x87D0_0324));
        assert_eq!(parse_code("abc"), Some(0xABC));
        assert_eq!(parse_code("xyz"), None);
    }

    #[test]
    fn lookup_and_find() {
        let i = lookup(1603);
        assert_eq!(i.hresult, 0x8007_0643);
        assert!(i.short().contains("1603"));
        assert_eq!(i.forms(), "0x80070643 = -2147023293 = Win32 1603");
        assert!(lookup(0x87D0_0324).facility.contains("ConfigMgr"));

        let c = find_codes(
            "Failed, error 0x80070005, content 0x0000ABCD, exit code 1603, hr=-2147024891 (dup)",
        );
        assert_eq!(c, vec![0x8007_0005, 1603]);
        assert_eq!(
            find_codes("Process terminated with exit code 0"),
            Vec::<u32>::new()
        );
        // The final status of a Windows Installer log (#40).
        assert_eq!(
            find_codes("Installation success or error status: 1603."),
            vec![1603]
        );
        assert!(find_codes("Installation success or error status: 0.").is_empty());
    }
}
