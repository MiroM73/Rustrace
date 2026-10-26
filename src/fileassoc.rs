//! Windows file association: RusTrace as the program for `.log` and `.lo_` files.
//!
//! Everything is written per user under `HKEY_CURRENT_USER` (no administrator rights):
//! - `Software\Classes\RusTrace.LogFile` – the file type (icon, `"rustrace.exe" "%1"`),
//! - `Software\Classes\<ext>` – default type of the extension + `OpenWithProgids` entry,
//! - `Software\Classes\Applications\rustrace.exe` – "Open with" entry with the supported types,
//! - `Software\RusTrace\Capabilities` + `Software\RegisteredApplications` – RusTrace listed in
//!   Windows Settings → Default apps.
//!
//! Since Windows 8 a program cannot replace the user's own choice ("UserChoice", protected by a
//! hash): when another program was chosen for an extension, the user has to pick RusTrace in the
//! Windows settings. [`status`] tells which case applies.

/// File type (ProgID) of the log files opened by RusTrace.
pub const PROG_ID: &str = "RusTrace.LogFile";
/// Extensions RusTrace registers for.
pub const EXTENSIONS: [&str; 2] = [".log", ".lo_"];
/// Name under `RegisteredApplications` (also used by the Windows settings link).
pub const APP_NAME: &str = "RusTrace";
/// Root of the registration under `HKEY_CURRENT_USER`.
pub const ROOT: &str = "Software";

/// Which program opens an extension.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtStatus {
    /// RusTrace opens the files.
    RusTrace,
    /// Another program (its name, or the ProgID when Windows does not know a name) – chosen by
    /// the user or the system default.
    Other(String),
    /// No program is set.
    None,
}

/// One registry value of the registration: key under `HKEY_CURRENT_USER`, value name
/// (empty = default value) and text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegValue {
    pub key: String,
    pub name: String,
    pub data: String,
}

fn value(key: String, name: &str, data: &str) -> RegValue {
    RegValue {
        key,
        name: name.to_owned(),
        data: data.to_owned(),
    }
}

/// Command line Windows runs to open a file.
pub fn open_command(exe: &str) -> String {
    format!("\"{exe}\" \"%1\"")
}

/// All registry values written by [`register`] for the program `exe`, under `root`
/// ("Software"; the tests use their own key).
pub fn registration_values(root: &str, exe: &str, type_name: &str) -> Vec<RegValue> {
    let classes = format!("{root}\\Classes");
    let caps = format!("{root}\\{APP_NAME}\\Capabilities");
    let app = format!("{classes}\\Applications\\rustrace.exe");
    let mut v = vec![
        value(format!("{classes}\\{PROG_ID}"), "", type_name),
        value(format!("{classes}\\{PROG_ID}\\DefaultIcon"), "", &format!("\"{exe}\",0")),
        value(format!("{classes}\\{PROG_ID}\\shell\\open\\command"), "", &open_command(exe)),
        value(app.clone(), "FriendlyAppName", APP_NAME),
        value(format!("{app}\\shell\\open\\command"), "", &open_command(exe)),
        value(caps.clone(), "ApplicationName", APP_NAME),
        value(caps.clone(), "ApplicationDescription", type_name),
        value(format!("{root}\\RegisteredApplications"), APP_NAME, &caps),
    ];
    for ext in EXTENSIONS {
        v.push(value(format!("{classes}\\{ext}"), "", PROG_ID));
        v.push(value(format!("{classes}\\{ext}\\OpenWithProgids"), PROG_ID, ""));
        v.push(value(format!("{app}\\SupportedTypes"), ext, ""));
        v.push(value(format!("{caps}\\FileAssociations"), ext, PROG_ID));
    }
    v
}

/// Which program opens an extension: the user's choice wins; without it the default file type
/// of the extension.
pub fn classify(user_choice: Option<&str>, class_default: Option<&str>) -> ExtStatus {
    let is_ours = |p: &str| {
        p.eq_ignore_ascii_case(PROG_ID) || p.eq_ignore_ascii_case("Applications\\rustrace.exe")
    };
    match user_choice.or(class_default).map(str::trim) {
        Some("") => ExtStatus::None,
        Some(p) if is_ours(p) => ExtStatus::RusTrace,
        Some(p) => ExtStatus::Other(p.to_owned()),
        None => ExtStatus::None,
    }
}

#[cfg(windows)]
pub use os::*;

#[cfg(windows)]
mod os {
    use super::*;
    use std::ffi::c_void;
    use std::io;

    type Hkey = isize;
    const HKEY_CURRENT_USER: Hkey = 0x8000_0001_u32 as i32 as isize;
    const HKEY_LOCAL_MACHINE: Hkey = 0x8000_0002_u32 as i32 as isize;
    const KEY_WRITE: u32 = 0x2_0006;
    const KEY_READ: u32 = 0x2_0019;
    const REG_SZ: u32 = 1;
    const RRF_RT_REG_SZ: u32 = 0x2;
    const ERROR_FILE_NOT_FOUND: i32 = 2;
    const SHCNE_ASSOCCHANGED: i32 = 0x0800_0000;
    const ASSOCSTR_FRIENDLYAPPNAME: i32 = 4;

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegCreateKeyExW(
            key: Hkey,
            subkey: *const u16,
            reserved: u32,
            class: *const u16,
            options: u32,
            sam: u32,
            security: *const c_void,
            result: *mut Hkey,
            disposition: *mut u32,
        ) -> i32;
        fn RegSetValueExW(
            key: Hkey,
            name: *const u16,
            reserved: u32,
            kind: u32,
            data: *const u8,
            size: u32,
        ) -> i32;
        fn RegGetValueW(
            key: Hkey,
            subkey: *const u16,
            name: *const u16,
            flags: u32,
            kind: *mut u32,
            data: *mut c_void,
            size: *mut u32,
        ) -> i32;
        fn RegDeleteTreeW(key: Hkey, subkey: *const u16) -> i32;
        fn RegDeleteKeyValueW(key: Hkey, subkey: *const u16, name: *const u16) -> i32;
        fn RegCloseKey(key: Hkey) -> i32;
        fn RegOpenKeyExW(key: Hkey, subkey: *const u16, options: u32, sam: u32, result: *mut Hkey) -> i32;
        fn RegQueryInfoKeyW(
            key: Hkey,
            class: *mut u16,
            class_len: *mut u32,
            reserved: *const u32,
            subkeys: *mut u32,
            max_subkey_len: *mut u32,
            max_class_len: *mut u32,
            values: *mut u32,
            max_value_name_len: *mut u32,
            max_value_len: *mut u32,
            security_len: *mut u32,
            last_write: *mut u64,
        ) -> i32;
        fn RegDeleteKeyW(key: Hkey, subkey: *const u16) -> i32;
    }

    #[link(name = "shlwapi")]
    unsafe extern "system" {
        fn AssocQueryStringW(
            flags: u32,
            what: i32,
            assoc: *const u16,
            extra: *const u16,
            out: *mut u16,
            size: *mut u32,
        ) -> i32;
    }

    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SHChangeNotify(event: i32, flags: u32, item1: *const c_void, item2: *const c_void);
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn check(code: i32) -> io::Result<()> {
        if code == 0 {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(code))
        }
    }

    fn set_value(v: &RegValue) -> io::Result<()> {
        let mut key: Hkey = 0;
        // SAFETY: NUL-terminated strings, `key` receives the opened handle.
        check(unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                wide(&v.key).as_ptr(),
                0,
                std::ptr::null(),
                0,
                KEY_WRITE,
                std::ptr::null(),
                &mut key,
                std::ptr::null_mut(),
            )
        })?;
        let data = wide(&v.data);
        let name = wide(&v.name);
        // SAFETY: `data` is a NUL-terminated UTF-16 string, its size is given in bytes.
        let res = check(unsafe {
            RegSetValueExW(
                key,
                if v.name.is_empty() { std::ptr::null() } else { name.as_ptr() },
                0,
                REG_SZ,
                data.as_ptr().cast(),
                (data.len() * 2) as u32,
            )
        });
        // SAFETY: the handle was opened above.
        unsafe { RegCloseKey(key) };
        res
    }

    fn get_string(root: Hkey, key: &str, name: &str) -> Option<String> {
        let mut buf = vec![0u16; 1024];
        let mut size = (buf.len() * 2) as u32;
        let name_w = wide(name);
        // SAFETY: the buffer size is given in bytes; RRF_RT_REG_SZ guarantees a NUL-terminated string.
        let res = unsafe {
            RegGetValueW(
                root,
                wide(key).as_ptr(),
                if name.is_empty() { std::ptr::null() } else { name_w.as_ptr() },
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if res != 0 {
            return None;
        }
        let len = (size as usize / 2).saturating_sub(1);
        Some(String::from_utf16_lossy(&buf[..len]))
    }

    fn delete_tree(key: &str) -> io::Result<()> {
        // SAFETY: NUL-terminated string.
        match unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, wide(key).as_ptr()) } {
            0 | ERROR_FILE_NOT_FOUND => Ok(()),
            e => check(e),
        }
    }

    /// Deletes a key that has no values and no subkeys left (an extension key that only
    /// RusTrace created); a key in use or a missing key is left alone.
    fn delete_if_empty(key: &str) {
        let key_w = wide(key);
        let mut h: Hkey = 0;
        // SAFETY: NUL-terminated string, `h` receives the opened handle.
        if unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, key_w.as_ptr(), 0, KEY_READ, &mut h) } != 0 {
            return;
        }
        let (mut subkeys, mut values) = (0u32, 0u32);
        use std::ptr::{null, null_mut};
        // SAFETY: only the two counts are requested, all other outputs are null.
        let res = unsafe {
            RegQueryInfoKeyW(
                h,
                null_mut(),
                null_mut(),
                null(),
                &mut subkeys,
                null_mut(),
                null_mut(),
                &mut values,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
            )
        };
        // SAFETY: the handle was opened above.
        unsafe { RegCloseKey(h) };
        if res == 0 && subkeys == 0 && values == 0 {
            // SAFETY: NUL-terminated string.
            unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, key_w.as_ptr()) };
        }
    }

    fn delete_value(key: &str, name: &str) -> io::Result<()> {
        let name_w = wide(name);
        // SAFETY: NUL-terminated strings.
        match unsafe {
            RegDeleteKeyValueW(
                HKEY_CURRENT_USER,
                wide(key).as_ptr(),
                if name.is_empty() { std::ptr::null() } else { name_w.as_ptr() },
            )
        } {
            0 | ERROR_FILE_NOT_FOUND => Ok(()),
            e => check(e),
        }
    }

    fn notify_shell() {
        // SAFETY: SHCNE_ASSOCCHANGED takes no items.
        unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, 0, std::ptr::null(), std::ptr::null()) };
    }

    /// Registers `exe` for the log extensions under `HKEY_CURRENT_USER\<root>`.
    pub fn register_under(root: &str, exe: &str, type_name: &str) -> io::Result<()> {
        for v in registration_values(root, exe, type_name) {
            set_value(&v)?;
        }
        Ok(())
    }

    /// Removes what [`register_under`] wrote; the default type of an extension is removed only
    /// when it is still RusTrace's.
    pub fn unregister_under(root: &str) -> io::Result<()> {
        let classes = format!("{root}\\Classes");
        for ext in EXTENSIONS {
            let key = format!("{classes}\\{ext}");
            if get_string(HKEY_CURRENT_USER, &key, "").as_deref() == Some(PROG_ID) {
                delete_value(&key, "")?;
            }
            delete_value(&format!("{key}\\OpenWithProgids"), PROG_ID)?;
            delete_if_empty(&format!("{key}\\OpenWithProgids"));
            delete_if_empty(&key);
        }
        delete_tree(&format!("{classes}\\{PROG_ID}"))?;
        delete_tree(&format!("{classes}\\Applications\\rustrace.exe"))?;
        delete_tree(&format!("{root}\\{APP_NAME}"))?;
        delete_value(&format!("{root}\\RegisteredApplications"), APP_NAME)?;
        Ok(())
    }

    /// Registers the running program for `.log` and `.lo_` files.
    pub fn register(type_name: &str) -> io::Result<()> {
        let exe = std::env::current_exe()?;
        register_under(ROOT, &exe.to_string_lossy(), type_name)?;
        notify_shell();
        Ok(())
    }

    /// Removes the registration.
    pub fn unregister() -> io::Result<()> {
        unregister_under(ROOT)?;
        notify_shell();
        Ok(())
    }

    /// The program the registration starts (None = not registered).
    pub fn registered_command() -> Option<String> {
        get_string(
            HKEY_CURRENT_USER,
            &format!("{ROOT}\\Classes\\{PROG_ID}\\shell\\open\\command"),
            "",
        )
    }

    /// True when the registration starts the running program (not a copy somewhere else).
    pub fn registered_for_this_exe() -> bool {
        let Ok(exe) = std::env::current_exe() else {
            return false;
        };
        registered_command()
            .is_some_and(|c| c.eq_ignore_ascii_case(&open_command(&exe.to_string_lossy())))
    }

    /// Which program opens files with the extension `ext` (".log").
    pub fn status(ext: &str) -> ExtStatus {
        let exts = format!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts\\{ext}");
        // Newer Windows 11 builds keep the choice in "UserChoiceLatest\ProgId".
        let choice = [r"UserChoiceLatest\ProgId", "UserChoiceLatest", "UserChoice"]
            .iter()
            .find_map(|k| get_string(HKEY_CURRENT_USER, &format!("{exts}\\{k}"), "ProgId"));
        let class = get_string(HKEY_CURRENT_USER, &format!("Software\\Classes\\{ext}"), "")
            .or_else(|| get_string(HKEY_LOCAL_MACHINE, &format!("Software\\Classes\\{ext}"), ""));
        match classify(choice.as_deref(), class.as_deref()) {
            ExtStatus::Other(prog) => ExtStatus::Other(friendly_app_name(ext).unwrap_or(prog)),
            s => s,
        }
    }

    /// Name of the program Windows opens files with the extension with ("Notepad").
    fn friendly_app_name(ext: &str) -> Option<String> {
        let mut buf = vec![0u16; 512];
        let mut size = buf.len() as u32;
        let open = wide("open");
        // SAFETY: NUL-terminated strings; `size` is the buffer length in characters.
        let hr = unsafe {
            AssocQueryStringW(
                0,
                ASSOCSTR_FRIENDLYAPPNAME,
                wide(ext).as_ptr(),
                open.as_ptr(),
                buf.as_mut_ptr(),
                &mut size,
            )
        };
        if hr != 0 {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let name = String::from_utf16_lossy(&buf[..len]);
        (!name.trim().is_empty()).then_some(name)
    }

    /// State of the registration shown in the default app window.
    pub struct Info {
        pub exts: Vec<(&'static str, ExtStatus)>,
        /// Command of the registration (None = not registered).
        pub command: Option<String>,
        /// The registration starts the running program.
        pub this_exe: bool,
    }

    pub fn info() -> Info {
        Info {
            exts: EXTENSIONS.iter().map(|&e| (e, status(e))).collect(),
            command: registered_command(),
            this_exe: registered_for_this_exe(),
        }
    }

    /// Opens Windows Settings → Default apps (on Windows 11 directly the page of RusTrace).
    pub fn open_default_apps_settings() -> io::Result<()> {
        open::that_detached(format!("ms-settings:defaultapps?registeredAppUser={APP_NAME}"))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn key_exists(key: &str) -> bool {
            let mut h: Hkey = 0;
            // SAFETY: NUL-terminated string; the handle is closed right away.
            let ok = unsafe {
                RegOpenKeyExW(HKEY_CURRENT_USER, wide(key).as_ptr(), 0, KEY_READ, &mut h)
            } == 0;
            if ok {
                // SAFETY: the handle was opened above.
                unsafe { RegCloseKey(h) };
            }
            ok
        }

        /// Registration and removal in a separate test key under HKEY_CURRENT_USER.
        #[test]
        fn register_and_unregister() {
            let root = format!("Software\\RusTraceTest-{}", std::process::id());
            let exe = r"C:\Tools\RusTrace\rustrace.exe";
            register_under(&root, exe, "Log file").unwrap();
            let hk = HKEY_CURRENT_USER;
            assert_eq!(
                get_string(hk, &format!("{root}\\Classes\\{PROG_ID}\\shell\\open\\command"), "")
                    .as_deref(),
                Some(r#""C:\Tools\RusTrace\rustrace.exe" "%1""#)
            );
            assert_eq!(
                get_string(hk, &format!("{root}\\Classes\\.lo_"), "").as_deref(),
                Some(PROG_ID)
            );
            assert_eq!(
                get_string(hk, &format!("{root}\\RegisteredApplications"), APP_NAME),
                Some(format!("{root}\\{APP_NAME}\\Capabilities"))
            );
            // An extension taken over by another program keeps it after removal.
            set_value(&RegValue {
                key: format!("{root}\\Classes\\.log"),
                name: String::new(),
                data: "txtfile".into(),
            })
            .unwrap();
            unregister_under(&root).unwrap();
            // The .lo_ key existed only for RusTrace, so it is gone completely.
            assert!(!key_exists(&format!("{root}\\Classes\\.lo_")));
            assert!(key_exists(&format!("{root}\\Classes\\.log")));
            assert_eq!(
                get_string(hk, &format!("{root}\\Classes\\.log"), "").as_deref(),
                Some("txtfile")
            );
            assert_eq!(get_string(hk, &format!("{root}\\Classes\\{PROG_ID}"), ""), None);
            assert_eq!(get_string(hk, &format!("{root}\\RegisteredApplications"), APP_NAME), None);
            delete_tree(&root).unwrap();
            assert_eq!(get_string(hk, &format!("{root}\\Classes\\.log"), ""), None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_cover_both_extensions() {
        let v = registration_values("Software", r"C:\x\rustrace.exe", "Log file");
        let find = |key: &str, name: &str| {
            v.iter()
                .find(|r| r.key == key && r.name == name)
                .map(|r| r.data.as_str())
        };
        assert_eq!(
            find(r"Software\Classes\RusTrace.LogFile\shell\open\command", ""),
            Some(r#""C:\x\rustrace.exe" "%1""#)
        );
        for ext in EXTENSIONS {
            assert_eq!(find(&format!(r"Software\Classes\{ext}"), ""), Some(PROG_ID));
            assert_eq!(find(&format!(r"Software\Classes\{ext}\OpenWithProgids"), PROG_ID), Some(""));
            assert_eq!(
                find(r"Software\RusTrace\Capabilities\FileAssociations", ext),
                Some(PROG_ID)
            );
        }
        assert_eq!(
            find(r"Software\RegisteredApplications", APP_NAME),
            Some(r"Software\RusTrace\Capabilities")
        );
    }

    #[test]
    fn classify_status() {
        // The user's choice wins over the default type.
        assert_eq!(classify(Some("txtfilelegacy"), Some(PROG_ID)), ExtStatus::Other("txtfilelegacy".into()));
        assert_eq!(classify(Some("rustrace.logfile"), None), ExtStatus::RusTrace);
        assert_eq!(classify(Some(r"Applications\RusTrace.exe"), None), ExtStatus::RusTrace);
        assert_eq!(classify(None, Some(PROG_ID)), ExtStatus::RusTrace);
        assert_eq!(classify(None, Some("txtfile")), ExtStatus::Other("txtfile".into()));
        assert_eq!(classify(None, None), ExtStatus::None);
        assert_eq!(classify(None, Some("")), ExtStatus::None);
    }
}
