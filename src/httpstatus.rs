//! HTTP status codes with IIS substatus codes (#28) – explanations of `sc-status` /
//! `sc-substatus` in IIS (W3C) logs and of `404.3` / `HTTP 500` in the error code lookup.
//!
//! Descriptions come from the language files: `http.<status>` (e.g. `http.404`),
//! `http.<status>.<substatus>` (IIS, e.g. `http.404.3`) and the class fallbacks `http.1xx` …
//! `http.5xx` for codes without an own description. New codes can be added in a `.lang` file
//! without recompiling. Which log columns hold the codes is set by the log definition
//! (`http_status`, `http_substatus`, `win32_status` – see `logdef.rs`).

use crate::{i18n, t};

/// Explanation of an HTTP status, optionally with an IIS substatus.
#[derive(Clone, Debug, PartialEq)]
pub struct HttpStatus {
    pub status: u16,
    /// IIS substatus (`None` = no substatus; `404.0` = `Some(0)`).
    pub substatus: Option<u32>,
    /// Description of the status, or of its class (`exact` = false), e.g. "4xx – client error".
    pub text: String,
    /// The status has an own description (not only the class fallback).
    pub exact: bool,
    /// Description of the substatus, when there is one.
    pub sub_text: Option<String>,
}

impl HttpStatus {
    /// The code as written in IIS documentation: "404" or "404.3".
    pub fn code(&self) -> String {
        match self.substatus {
            Some(sub) => format!("{}.{sub}", self.status),
            None => self.status.to_string(),
        }
    }

    /// One-line description, e.g. "HTTP 404.3 Not Found – … IIS substatus: MIME type restriction …".
    pub fn short(&self) -> String {
        match &self.sub_text {
            Some(sub) => t!(
                "http.short.sub",
                code = self.code(),
                status = self.text,
                substatus = sub
            ),
            None => t!("http.short", code = self.code(), status = self.text),
        }
    }
}

/// Describes a status code (with an optional IIS substatus) from the language files.
/// Substatus 0 counts only where it has an own description (`http.500.0`).
pub fn describe(status: u16, substatus: Option<u32>) -> HttpStatus {
    let (text, exact) = match i18n::tr_opt(&format!("http.{status}")) {
        Some(d) => (d, true),
        None => (
            i18n::tr_opt(&format!("http.{}xx", status / 100))
                .filter(|_| (100..=599).contains(&status))
                .unwrap_or_else(|| t!("http.unknown")),
            false,
        ),
    };
    let sub_text = substatus.and_then(|sub| i18n::tr_opt(&format!("http.{status}.{sub}")));
    // Substatus 0 is "no substatus" unless it has an own description (IIS 500.0, 503.0).
    let substatus = substatus.filter(|&sub| sub != 0 || sub_text.is_some());
    HttpStatus {
        status,
        substatus,
        text,
        exact,
        sub_text,
    }
}

/// Explains the values of the status / substatus cells of a log row; `None` when the status
/// cell holds no number.
pub fn from_cells(status: &str, substatus: Option<&str>) -> Option<HttpStatus> {
    let status: u16 = status.trim().parse().ok()?;
    let sub = substatus.and_then(|s| s.trim().parse::<u32>().ok());
    Some(describe(status, sub))
}

/// Value of a Win32 status cell (decimal, e.g. IIS `sc-win32-status` = `64`); `None` for an
/// empty or non-numeric value.
pub fn win32_from_cell(value: &str) -> Option<u32> {
    value
        .trim()
        .parse::<u64>()
        .ok()
        .and_then(|v| u32::try_from(v).ok())
}

/// An HTTP status entered in the error code lookup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HttpQuery {
    pub status: u16,
    pub substatus: Option<u32>,
    /// Only an HTTP status: written with `HTTP` or with a substatus (`404.3`). A plain number in
    /// 100–599 (`404`) is ambiguous – it is also a Win32 code, so the lookup shows both.
    pub explicit: bool,
}

/// Parses `404`, `404.3`, `HTTP 500`, `http 403.14`, `HTTP:500` in the error code lookup.
/// The status must have three digits in 100–599.
pub fn parse_query(input: &str) -> Option<HttpQuery> {
    let s = input.trim();
    let (prefixed, rest) = match s.get(..4) {
        Some(p) if p.eq_ignore_ascii_case("http") => (
            true,
            s[4..].trim_start_matches(|c: char| c.is_whitespace() || c == ':' || c == '/'),
        ),
        _ => (false, s),
    };
    let (st, sub) = match rest.split_once('.') {
        Some((a, b)) => (a, Some(b)),
        None => (rest, None),
    };
    let digits = |x: &str, max: usize| {
        !x.is_empty() && x.len() <= max && x.chars().all(|c| c.is_ascii_digit())
    };
    if st.len() != 3 || !digits(st, 3) {
        return None;
    }
    let status: u16 = st.parse().ok()?;
    if !(100..=599).contains(&status) {
        return None;
    }
    let substatus = match sub {
        Some(b) if digits(b, 5) => Some(b.parse().ok()?),
        Some(_) => return None,
        None => None,
    };
    Some(HttpQuery {
        status,
        substatus,
        explicit: prefixed || sub.is_some(),
    })
}

/// What the error code lookup shows for an input: the HTTP status and / or the error code
/// (`errcodes::parse_code`). An explicit HTTP input (`HTTP 500`, `404.3`) is only an HTTP
/// status; a plain `404` is both; anything else is only an error code.
pub fn lookup_input(input: &str) -> (Option<HttpQuery>, Option<u32>) {
    let http = parse_query(input);
    let code = match http {
        Some(q) if q.explicit => None,
        _ => crate::errcodes::parse_code(input),
    };
    (http, code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_http_queries() {
        let q = |status, substatus, explicit| {
            Some(HttpQuery {
                status,
                substatus,
                explicit,
            })
        };
        assert_eq!(parse_query("404"), q(404, None, false));
        assert_eq!(parse_query(" 404.3 "), q(404, Some(3), true));
        assert_eq!(parse_query("500.0"), q(500, Some(0), true));
        assert_eq!(parse_query("HTTP 500"), q(500, None, true));
        assert_eq!(parse_query("http500.19"), q(500, Some(19), true));
        assert_eq!(parse_query("HTTP: 403.14"), q(403, Some(14), true));
        assert_eq!(parse_query("HTTP/503.2"), q(503, Some(2), true));
        assert_eq!(parse_query("1603"), None);
        assert_eq!(parse_query("99"), None);
        assert_eq!(parse_query("600"), None);
        assert_eq!(parse_query("HTTP 1603"), None);
        assert_eq!(parse_query("404."), None);
        assert_eq!(parse_query("404.x"), None);
        assert_eq!(parse_query("0x80070005"), None);
        assert_eq!(parse_query("HTTP"), None);
    }

    #[test]
    fn lookup_input_ambiguity() {
        // Plain number in the HTTP range: both meanings.
        let (h, c) = lookup_input("404");
        assert_eq!(h.map(|q| q.status), Some(404));
        assert_eq!(c, Some(404));
        // Explicit HTTP: no error code.
        assert_eq!(lookup_input("HTTP 500").1, None);
        assert_eq!(lookup_input("404.3").1, None);
        // Error codes stay as before.
        assert_eq!(lookup_input("1603"), (None, Some(1603)));
        assert_eq!(lookup_input("0x80070005"), (None, Some(0x8007_0005)));
        assert_eq!(lookup_input("80070005"), (None, Some(0x8007_0005)));
        assert_eq!(lookup_input("-2147024891"), (None, Some(0x8007_0005)));
        assert_eq!(lookup_input("64"), (None, Some(64)));
        assert_eq!(lookup_input("xyz"), (None, None));
    }

    #[test]
    fn describe_with_fallbacks() {
        let d = describe(404, Some(3));
        assert!(d.exact);
        assert_eq!(d.code(), "404.3");
        assert_eq!(Some(d.text.clone()), i18n::tr_opt("http.404"));
        assert_eq!(d.sub_text, i18n::tr_opt("http.404.3"));
        assert!(d.sub_text.is_some());
        assert!(d.short().contains("404.3"));
        // Unknown status: class description.
        let d = describe(499, None);
        assert!(!d.exact);
        assert_eq!(Some(d.text), i18n::tr_opt("http.4xx"));
        // Unknown substatus: only the status.
        let d = describe(404, Some(999));
        assert_eq!(d.sub_text, None);
        assert_eq!(d.code(), "404.999");
        // Out of range: unknown.
        assert_eq!(describe(999, None).text, t!("http.unknown"));
        // Substatus 0 has a description only where IIS documents one (500.0, not 404.0).
        assert!(describe(500, Some(0)).sub_text.is_some());
        assert_eq!(describe(404, Some(0)).sub_text, None);
        assert_eq!(describe(404, Some(0)).code(), "404");
        assert_eq!(describe(500, Some(0)).code(), "500.0");
        // Every class has a description.
        for c in 1..=5 {
            assert!(i18n::tr_opt(&format!("http.{c}xx")).is_some());
        }
    }

    #[test]
    fn cells() {
        let d = from_cells("404", Some("3")).unwrap();
        assert_eq!((d.status, d.substatus), (404, Some(3)));
        assert_eq!(from_cells("200", Some("")).unwrap().substatus, None);
        assert_eq!(from_cells("200", None).unwrap().substatus, None);
        assert_eq!(from_cells("", Some("3")), None);
        assert_eq!(win32_from_cell("64"), Some(64));
        assert_eq!(win32_from_cell("2148074254"), Some(0x8009_030E));
        assert_eq!(win32_from_cell(""), None);
        assert_eq!(win32_from_cell("-"), None);
    }
}
