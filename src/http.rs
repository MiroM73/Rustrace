//! Minimal HTTPS GET for the update check (#15): WinHTTP through raw FFI on Windows, so the
//! system proxy settings and certificate store are used and no TLS library is compiled in.
//! Redirects (GitHub release downloads go to another host) are followed by WinHTTP.

/// Downloads `url` (https only). `progress(received, total)` is called after every block;
/// returning `false` cancels the download.
#[cfg(windows)]
pub fn get(
    url: &str,
    accept: &str,
    progress: impl FnMut(u64, Option<u64>) -> bool,
) -> Result<Vec<u8>, String> {
    imp::get(url, accept, progress)
}

#[cfg(not(windows))]
pub fn get(
    _url: &str,
    _accept: &str,
    _progress: impl FnMut(u64, Option<u64>) -> bool,
) -> Result<Vec<u8>, String> {
    Err("not supported on this platform".into())
}

/// Splits `https://host/path?query` into host and path (with the query).
pub fn split_url(url: &str) -> Option<(&str, &str)> {
    let rest = url.strip_prefix("https://")?;
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    (!host.is_empty()).then_some((host, path))
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;

    type Handle = *mut c_void;

    #[link(name = "winhttp")]
    unsafe extern "system" {
        fn WinHttpOpen(
            agent: *const u16,
            access: u32,
            proxy: *const u16,
            bypass: *const u16,
            flags: u32,
        ) -> Handle;
        fn WinHttpSetTimeouts(
            h: Handle,
            resolve: i32,
            connect: i32,
            send: i32,
            receive: i32,
        ) -> i32;
        fn WinHttpConnect(session: Handle, server: *const u16, port: u16, reserved: u32) -> Handle;
        fn WinHttpOpenRequest(
            connect: Handle,
            verb: *const u16,
            object: *const u16,
            version: *const u16,
            referrer: *const u16,
            accept_types: *const *const u16,
            flags: u32,
        ) -> Handle;
        fn WinHttpSendRequest(
            request: Handle,
            headers: *const u16,
            headers_len: u32,
            optional: *const c_void,
            optional_len: u32,
            total_len: u32,
            context: usize,
        ) -> i32;
        fn WinHttpReceiveResponse(request: Handle, reserved: *mut c_void) -> i32;
        fn WinHttpQueryHeaders(
            request: Handle,
            info_level: u32,
            name: *const u16,
            buffer: *mut c_void,
            len: *mut u32,
            index: *mut u32,
        ) -> i32;
        fn WinHttpReadData(
            request: Handle,
            buffer: *mut c_void,
            to_read: u32,
            read: *mut u32,
        ) -> i32;
        fn WinHttpCloseHandle(h: Handle) -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetLastError() -> u32;
    }

    const WINHTTP_ACCESS_TYPE_DEFAULT_PROXY: u32 = 0;
    const WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY: u32 = 4;
    const WINHTTP_FLAG_SECURE: u32 = 0x0080_0000;
    const WINHTTP_QUERY_STATUS_CODE: u32 = 19;
    const WINHTTP_QUERY_CONTENT_LENGTH: u32 = 5;
    const WINHTTP_QUERY_FLAG_NUMBER: u32 = 0x2000_0000;
    const WINHTTP_QUERY_FLAG_NUMBER64: u32 = 0x0800_0000;

    /// Closes a WinHTTP handle when dropped.
    struct Guard(Handle);

    impl Drop for Guard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: a handle returned by WinHTTP, closed once.
                unsafe { WinHttpCloseHandle(self.0) };
            }
        }
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Text of the last WinHTTP / Windows error.
    fn last_error(what: &str) -> String {
        // SAFETY: no arguments.
        let code = unsafe { GetLastError() };
        let info = crate::errcodes::lookup(code);
        match info.descriptions.last() {
            Some((_, d)) => format!("{what}: {d} ({code})"),
            None => format!("{what}: error {code}"),
        }
    }

    fn check(ok: bool, what: &str) -> Result<(), String> {
        if ok { Ok(()) } else { Err(last_error(what)) }
    }

    fn open(h: Handle, what: &str) -> Result<Guard, String> {
        if h.is_null() {
            Err(last_error(what))
        } else {
            Ok(Guard(h))
        }
    }

    pub fn get(
        url: &str,
        accept: &str,
        mut progress: impl FnMut(u64, Option<u64>) -> bool,
    ) -> Result<Vec<u8>, String> {
        let (host, path) = super::split_url(url).ok_or_else(|| format!("unsupported URL {url}"))?;
        let agent = wide(&format!("RusTrace/{}", crate::app::VERSION));
        let (host, path, verb) = (wide(host), wide(path), wide("GET"));
        let headers = wide(&format!("Accept: {accept}\r\n"));
        // SAFETY: all strings are NUL-terminated and outlive the calls; handles are checked and
        // closed by the guards (request before connection before session).
        unsafe {
            // Automatic proxy (Windows 8.1+), else the WinHTTP default.
            let mut session = WinHttpOpen(
                agent.as_ptr(),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                std::ptr::null(),
                std::ptr::null(),
                0,
            );
            if session.is_null() {
                session = WinHttpOpen(
                    agent.as_ptr(),
                    WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                );
            }
            let session = open(session, "WinHttpOpen")?;
            WinHttpSetTimeouts(session.0, 15_000, 15_000, 30_000, 30_000);
            let connect = open(
                WinHttpConnect(session.0, host.as_ptr(), 443, 0),
                "WinHttpConnect",
            )?;
            let request = open(
                WinHttpOpenRequest(
                    connect.0,
                    verb.as_ptr(),
                    path.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    WINHTTP_FLAG_SECURE,
                ),
                "WinHttpOpenRequest",
            )?;
            check(
                WinHttpSendRequest(
                    request.0,
                    headers.as_ptr(),
                    u32::MAX,
                    std::ptr::null(),
                    0,
                    0,
                    0,
                ) != 0,
                "WinHttpSendRequest",
            )?;
            check(
                WinHttpReceiveResponse(request.0, std::ptr::null_mut()) != 0,
                "WinHttpReceiveResponse",
            )?;

            let mut status = 0u32;
            let mut len = 4u32;
            check(
                WinHttpQueryHeaders(
                    request.0,
                    WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                    std::ptr::null(),
                    (&raw mut status).cast(),
                    &mut len,
                    std::ptr::null_mut(),
                ) != 0,
                "WinHttpQueryHeaders",
            )?;
            if status != 200 {
                return Err(format!("HTTP {status}"));
            }
            let mut total = 0u64;
            let mut len = 8u32;
            let total = (WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_CONTENT_LENGTH | WINHTTP_QUERY_FLAG_NUMBER64,
                std::ptr::null(),
                (&raw mut total).cast(),
                &mut len,
                std::ptr::null_mut(),
            ) != 0)
                .then_some(total);

            let mut body = Vec::with_capacity(total.unwrap_or(0).min(256 << 20) as usize);
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                let mut read = 0u32;
                check(
                    WinHttpReadData(
                        request.0,
                        buf.as_mut_ptr().cast(),
                        buf.len() as u32,
                        &mut read,
                    ) != 0,
                    "WinHttpReadData",
                )?;
                if read == 0 {
                    break;
                }
                body.extend_from_slice(&buf[..read as usize]);
                if !progress(body.len() as u64, total) {
                    return Err("cancelled".into());
                }
            }
            Ok(body)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_parts() {
        assert_eq!(
            split_url("https://api.github.com/repos/a/b/releases/latest"),
            Some(("api.github.com", "/repos/a/b/releases/latest"))
        );
        assert_eq!(split_url("https://example.com"), Some(("example.com", "/")));
        assert_eq!(split_url("http://example.com/x"), None);
        assert_eq!(split_url("https:///x"), None);
    }
}
