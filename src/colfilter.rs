//! Filters of single table columns (the filter row under the column headers).
//!
//! Syntax of a filter text (always case-insensitive):
//! * `text` – the value contains `text`,
//! * `=text` – the value is exactly `text`,
//! * `a|b|c` – any of the alternatives (each one may start with `=`),
//! * `!…` at the start – negation: the value matches none of the alternatives.
//!
//! Spaces around alternatives are ignored; an empty filter (or only `!`) filters nothing.

use regex::{Regex, RegexBuilder};

pub struct ColumnFilter {
    re: Regex,
    negate: bool,
}

impl ColumnFilter {
    /// Compiles a filter text; `None` when it filters nothing.
    pub fn parse(input: &str) -> Option<ColumnFilter> {
        let input = input.trim();
        let (negate, body) = match input.strip_prefix('!') {
            Some(rest) => (true, rest),
            None => (false, input),
        };
        let alternatives: Vec<String> = body
            .split('|')
            .map(str::trim)
            .filter(|a| !a.is_empty() && *a != "=")
            .map(|a| match a.strip_prefix('=') {
                Some(exact) => format!("^{}$", regex::escape(exact.trim())),
                None => regex::escape(a),
            })
            .collect();
        if alternatives.is_empty() {
            return None;
        }
        let re = RegexBuilder::new(&format!("(?:{})", alternatives.join("|")))
            .case_insensitive(true)
            .build()
            .ok()?;
        Some(ColumnFilter { re, negate })
    }

    pub fn matches(&self, value: &str) -> bool {
        self.re.is_match(value) != self.negate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(filter: &str, value: &str) -> bool {
        ColumnFilter::parse(filter).unwrap().matches(value)
    }

    #[test]
    fn syntax() {
        assert!(ColumnFilter::parse("").is_none());
        assert!(ColumnFilter::parse("  ! ").is_none());
        assert!(ColumnFilter::parse("| =").is_none());
        // contains, case-insensitive
        assert!(m("pack", "DISM Package Manager"));
        assert!(!m("pack", "DISM Provider Store"));
        // alternatives with spaces around them
        assert!(m("error | warn", "Warning"));
        assert!(!m("error|warn", "Info"));
        // exact match
        assert!(m("=info", "Info"));
        assert!(!m("=info", "Information"));
        assert!(m("=500|=404", "404"));
        assert!(!m("=500|=404", "4040"));
        // negation applies to all alternatives
        assert!(m("!dism", "CBS"));
        assert!(!m("!dism|cbs", "CBS"));
        assert!(m("!=info", "Information"));
        // special characters are literal
        assert!(m("a.b(", "xa.b(y"));
        assert!(!m("a.b", "axb"));
    }
}
