//! Minimal HTTPS GET for the update check (#15): WinHTTP through raw FFI on Windows, so the
//! system proxy settings and certificate store are used and no TLS library is compiled in.
//! Failures are typed ([`HttpError`]) and transient ones retried ([`retry`], #60, port of
//! MMPortLens#26).
//! Redirects (GitHub release downloads go to another host) are followed by WinHTTP.

// The pure helpers are only called by the Windows code (and tested on every platform).
#![cfg_attr(not(windows), allow(dead_code))]

use std::time::Duration;

/// Downloads `url` (https only). `progress(received, total)` is called after every block;
/// returning `false` cancels the download.
#[cfg(windows)]
pub fn get(
    url: &str,
    accept: &str,
    progress: impl FnMut(u64, Option<u64>) -> bool,
) -> Result<Vec<u8>, HttpError> {
    imp::get(url, accept, progress)
}

#[cfg(not(windows))]
pub fn get(
    _url: &str,
    _accept: &str,
    _progress: impl FnMut(u64, Option<u64>) -> bool,
) -> Result<Vec<u8>, HttpError> {
    Err(HttpError::Other("not supported on this platform".into()))
}

/// Why a request failed (#60).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpError {
    /// The server answered with a status other than 200.
    Status { code: u32, limits: Limits },
    /// No answer: a WinHTTP error (`code`, e.g. 12002 = timeout) with its text.
    Network { code: u32, text: String },
    /// Anything else (bad URL, cancelled, platform).
    Other(String),
}

/// The headers of a refused request that say when to come back.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Limits {
    /// `Retry-After` in seconds (the HTTP-date form is ignored).
    pub retry_after: Option<u64>,
    /// `X-RateLimit-Remaining` (GitHub).
    pub rate_remaining: Option<u64>,
    /// `X-RateLimit-Reset`: Unix time when the rate limit is reset (GitHub).
    pub rate_reset: Option<i64>,
}

impl Limits {
    /// Reads the limits through `header(name)` (pure, so it is testable without a server).
    pub fn from_headers(header: impl Fn(&str) -> Option<String>) -> Self {
        let num = |name: &str| header(name).and_then(|v| v.trim().parse::<u64>().ok());
        Limits {
            retry_after: num("Retry-After"),
            rate_remaining: num("X-RateLimit-Remaining"),
            rate_reset: num("X-RateLimit-Reset").map(|v| v as i64),
        }
    }
}

/// What a failure means for the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// HTTP 5xx: the server has a (usually passing) problem.
    ServerUnavailable,
    /// HTTP 429, or 403 with no requests left in the rate limit.
    RateLimited,
    NotFound,
    /// No answer from the server (offline, timeout, connection refused / reset).
    Network,
    Other,
}

/// The waits before the retries of a transient failure, in seconds; their count is the number
/// of retries.
const RETRY_DELAYS: [u64; 3] = [2, 5, 10];
/// The longest `Retry-After` that is honoured, in seconds.
const MAX_RETRY_AFTER: u64 = 30;

impl HttpError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            HttpError::Status {
                code: 500..=599, ..
            } => ErrorKind::ServerUnavailable,
            HttpError::Status { code: 429, .. } => ErrorKind::RateLimited,
            HttpError::Status { code: 403, limits } if limits.rate_remaining == Some(0) => {
                ErrorKind::RateLimited
            }
            HttpError::Status { code: 404, .. } => ErrorKind::NotFound,
            HttpError::Network { .. } => ErrorKind::Network,
            _ => ErrorKind::Other,
        }
    }

    /// The wait before retry number `attempt` (0 = the first retry), or `None` when the request
    /// should not be repeated: only HTTP 502 / 503 / 504 and WinHTTP timeouts / broken
    /// connections (12002, 12029, 12030) are retried; never 4xx.
    pub fn retry_delay(&self, attempt: usize) -> Option<Duration> {
        let base = *RETRY_DELAYS.get(attempt)?;
        let after = match self {
            HttpError::Status {
                code: 502..=504,
                limits,
            } => limits.retry_after.unwrap_or(0).min(MAX_RETRY_AFTER),
            HttpError::Network {
                code: 12002 | 12029 | 12030,
                ..
            } => 0,
            _ => return None,
        };
        Some(Duration::from_secs(base.max(after)))
    }
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpError::Status { code, .. } => write!(f, "HTTP {code}"),
            HttpError::Network { text, .. } | HttpError::Other(text) => f.write_str(text),
        }
    }
}

impl From<HttpError> for String {
    fn from(e: HttpError) -> Self {
        e.to_string()
    }
}

/// Runs `op` and repeats it after `sleep(delay)` while it fails with a transient error
/// ([`HttpError::retry_delay`]); returns the first success or the last error.
pub fn retry<T>(
    mut op: impl FnMut() -> Result<T, HttpError>,
    mut sleep: impl FnMut(Duration),
) -> Result<T, HttpError> {
    let mut attempt = 0;
    loop {
        match op() {
            Err(e) => match e.retry_delay(attempt) {
                Some(delay) => {
                    sleep(delay);
                    attempt += 1;
                }
                None => return Err(e),
            },
            ok => return ok,
        }
    }
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
    use super::HttpError;
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
    const WINHTTP_QUERY_CUSTOM: u32 = 65535;

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

    /// The last WinHTTP / Windows error with its text.
    fn last_error(what: &str) -> HttpError {
        // SAFETY: no arguments.
        let code = unsafe { GetLastError() };
        let info = crate::errcodes::lookup(code);
        let text = match info.descriptions.last() {
            Some((_, d)) => format!("{what}: {d} ({code})"),
            None => format!("{what}: error {code}"),
        };
        HttpError::Network { code, text }
    }

    fn check(ok: bool, what: &str) -> Result<(), HttpError> {
        if ok { Ok(()) } else { Err(last_error(what)) }
    }

    fn open(h: Handle, what: &str) -> Result<Guard, HttpError> {
        if h.is_null() {
            Err(last_error(what))
        } else {
            Ok(Guard(h))
        }
    }

    /// The value of the response header `name`, if present (short values only).
    fn header(request: Handle, name: &str) -> Option<String> {
        let name = wide(name);
        let mut buf = [0u16; 128];
        let mut len = (buf.len() * 2) as u32;
        // SAFETY: `name` is NUL-terminated and `buf` holds `len` bytes; WinHTTP writes the
        // length in bytes (without the terminating NUL) back into `len`.
        let ok = unsafe {
            WinHttpQueryHeaders(
                request,
                WINHTTP_QUERY_CUSTOM,
                name.as_ptr(),
                buf.as_mut_ptr().cast(),
                &mut len,
                std::ptr::null_mut(),
            )
        } != 0;
        ok.then(|| String::from_utf16_lossy(&buf[..(len as usize / 2).min(buf.len())]))
    }

    pub fn get(
        url: &str,
        accept: &str,
        mut progress: impl FnMut(u64, Option<u64>) -> bool,
    ) -> Result<Vec<u8>, HttpError> {
        let (host, path) = super::split_url(url)
            .ok_or_else(|| HttpError::Other(format!("unsupported URL {url}")))?;
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
                let limits = super::Limits::from_headers(|name| header(request.0, name));
                return Err(HttpError::Status {
                    code: status,
                    limits,
                });
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
                    return Err(HttpError::Other("cancelled".into()));
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

    fn status(code: u32) -> HttpError {
        HttpError::Status {
            code,
            limits: Limits::default(),
        }
    }

    fn network(code: u32) -> HttpError {
        HttpError::Network {
            code,
            text: format!("WinHttpSendRequest: x ({code})"),
        }
    }

    #[test]
    fn error_kinds() {
        assert_eq!(status(503).kind(), ErrorKind::ServerUnavailable);
        assert_eq!(status(500).kind(), ErrorKind::ServerUnavailable);
        assert_eq!(status(429).kind(), ErrorKind::RateLimited);
        assert_eq!(status(403).kind(), ErrorKind::Other);
        let limited = HttpError::Status {
            code: 403,
            limits: Limits {
                rate_remaining: Some(0),
                ..Limits::default()
            },
        };
        assert_eq!(limited.kind(), ErrorKind::RateLimited);
        assert_eq!(status(404).kind(), ErrorKind::NotFound);
        assert_eq!(network(12002).kind(), ErrorKind::Network);
        assert_eq!(HttpError::Other("x".into()).kind(), ErrorKind::Other);
        assert_eq!(status(503).to_string(), "HTTP 503");
        assert_eq!(
            String::from(network(12002)),
            "WinHttpSendRequest: x (12002)"
        );
    }

    #[test]
    fn retried_errors_and_delays() {
        for code in [502, 503, 504] {
            assert_eq!(status(code).retry_delay(0), Some(Duration::from_secs(2)));
        }
        assert_eq!(status(503).retry_delay(2), Some(Duration::from_secs(10)));
        assert_eq!(status(503).retry_delay(3), None);
        for code in [12002, 12029, 12030] {
            assert_eq!(network(code).retry_delay(1), Some(Duration::from_secs(5)));
        }
        // Not retried: 4xx, other 5xx, a name that cannot be resolved (offline), cancel.
        for e in [
            status(403),
            status(404),
            status(429),
            status(500),
            network(12007),
        ] {
            assert_eq!(e.retry_delay(0), None, "{e:?}");
        }
        assert_eq!(HttpError::Other("cancelled".into()).retry_delay(0), None);
        // Retry-After is honoured up to 30 s.
        let after = |secs| HttpError::Status {
            code: 503,
            limits: Limits {
                retry_after: Some(secs),
                ..Limits::default()
            },
        };
        assert_eq!(after(7).retry_delay(0), Some(Duration::from_secs(7)));
        assert_eq!(after(1).retry_delay(1), Some(Duration::from_secs(5)));
        assert_eq!(after(3600).retry_delay(0), Some(Duration::from_secs(30)));
    }

    #[test]
    fn limits_from_headers() {
        let l = Limits::from_headers(|name| match name {
            "Retry-After" => Some(" 12 ".into()),
            "X-RateLimit-Remaining" => Some("0".into()),
            "X-RateLimit-Reset" => Some("1794150000".into()),
            _ => None,
        });
        assert_eq!(
            l,
            Limits {
                retry_after: Some(12),
                rate_remaining: Some(0),
                rate_reset: Some(1_794_150_000),
            }
        );
        // An HTTP date in Retry-After is ignored.
        let l = Limits::from_headers(|name| {
            (name == "Retry-After").then(|| "Wed, 21 Oct 2015 07:28:00 GMT".to_owned())
        });
        assert_eq!(l, Limits::default());
    }

    #[test]
    fn retry_repeats_only_transient_failures() {
        // Two 503s, then success: two waits.
        let mut answers = vec![Ok(7), Err(status(503)), Err(status(503))];
        let mut waits = Vec::new();
        let r = retry(|| answers.pop().unwrap(), |d| waits.push(d.as_secs()));
        assert_eq!((r, waits), (Ok(7), vec![2, 5]));
        // A 404 is returned at once.
        let mut calls = 0;
        let r: Result<(), _> = retry(
            || {
                calls += 1;
                Err(status(404))
            },
            |_| panic!("no wait"),
        );
        assert_eq!((r, calls), (Err(status(404)), 1));
        // A lasting 503 gives up after the last retry with the last error.
        let mut calls = 0;
        let mut waits = Vec::new();
        let r: Result<(), _> = retry(
            || {
                calls += 1;
                Err(status(503))
            },
            |d| waits.push(d.as_secs()),
        );
        assert_eq!((r, calls, waits), (Err(status(503)), 4, vec![2, 5, 10]));
    }

    /// Real failures (network, httpbin.org): `cargo test real_http_errors -- --ignored`.
    #[test]
    #[ignore]
    #[cfg(windows)]
    fn real_http_errors() {
        let get = |url: &str| get(url, "*/*", |_, _| true);
        let e = get("https://httpbin.org/status/404").unwrap_err();
        assert_eq!(
            (e.kind(), e.to_string()),
            (ErrorKind::NotFound, "HTTP 404".into())
        );
        let e = get("https://httpbin.org/status/503").unwrap_err();
        assert_eq!(e.kind(), ErrorKind::ServerUnavailable);
        // Retried three times (2 + 5 + 10 s, not slept here).
        let mut waits = Vec::new();
        let r = retry(|| get("https://httpbin.org/status/503"), |d| waits.push(d));
        assert_eq!(r.unwrap_err().kind(), ErrorKind::ServerUnavailable);
        assert_eq!(waits.len(), 3);
        // The headers of a refused request are read: GitHub sends its rate limit with a 404.
        let e = get("https://api.github.com/repos/MiroM73/Rustrace-does-not-exist").unwrap_err();
        let HttpError::Status { code: 404, limits } = &e else {
            panic!("{e:?}")
        };
        assert!(
            limits.rate_remaining.is_some() && limits.rate_reset.is_some(),
            "{limits:?}"
        );
        let e = get("https://name.invalid/").unwrap_err();
        assert_eq!(e.kind(), ErrorKind::Network, "{e}");
    }
}
