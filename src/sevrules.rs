//! Severity set by highlight rules (#50).
//!
//! A highlight rule can give the rows it matches a severity (Info / Warning / Error). It is
//! applied to the store after parsing (`LogStore::set_severity`; the parsed one stays in
//! `base_severity`), so everything that reads the severity – the Info / Warnings / Errors toggles,
//! the overview strip, the status bar counts, `--errors` / `--warnings`, the exports – follows.
//! The first enabled rule (in the order of the list) with a severity that matches decides; rows
//! no such rule matches keep the parsed severity, whatever the log format is.

use regex::Regex;

use crate::config::{HighlightRule, HighlightScope, RuleSet, RuleSeverity, active_rules_of};
use crate::filter::other_columns_match;
use crate::merge::ReadResult;
use crate::parser::Severity;
use crate::store::{EntryRef, LogStore};

struct SevRule {
    regex: Regex,
    /// Also the other columns (like the *Also component* option of a whole-row rule).
    other_columns: bool,
    severity: Severity,
}

/// The enabled, valid rules that set a severity, in priority order.
#[derive(Default)]
pub struct SevRules {
    rules: Vec<SevRule>,
    /// What the rules were built from – to tell whether a changed rule list changes anything.
    key: Vec<(String, bool, bool, bool, RuleSeverity)>,
}

impl PartialEq for SevRules {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl SevRules {
    pub fn from_config(rules: &[HighlightRule]) -> SevRules {
        let mut out = SevRules::default();
        for r in rules {
            let Some(sev) = r.severity.filter(|_| r.enabled && !r.pattern.is_empty()) else {
                continue;
            };
            let Ok(regex) = crate::app::compile_pattern(&r.pattern, r.is_regex, r.case_sensitive)
            else {
                continue;
            };
            let other_columns = r.scope == HighlightScope::Row && r.match_component;
            out.key.push((
                r.pattern.clone(),
                r.is_regex,
                r.case_sensitive,
                other_columns,
                sev,
            ));
            out.rules.push(SevRule {
                regex,
                other_columns,
                severity: match sev {
                    RuleSeverity::Info => Severity::Info,
                    RuleSeverity::Warning => Severity::Warning,
                    RuleSeverity::Error => Severity::Error,
                },
            });
        }
        out
    }

    /// The rules after the log definitions in use changed from `old` to `new` (#70): `None` when
    /// they did not, or no set is bound to a definition (so nothing can change).
    pub fn for_new_defs(sets: &[RuleSet], old: &[String], new: &[String]) -> Option<SevRules> {
        if old == new || sets.iter().all(|s| s.logdefs.is_empty()) {
            return None;
        }
        let rules: Vec<HighlightRule> = active_rules_of(sets, new).into_iter().cloned().collect();
        Some(SevRules::from_config(&rules))
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// The severity of the first matching rule.
    fn of(&self, e: &EntryRef) -> Option<Severity> {
        self.rules
            .iter()
            .find(|r| {
                r.regex.is_match(e.message) || (r.other_columns && other_columns_match(&r.regex, e))
            })
            .map(|r| r.severity)
    }

    /// Sets the severity of the entries from `from` on: the rule's, else the parsed one.
    pub fn apply(&self, store: &mut LogStore, from: usize) {
        for i in from..store.len() {
            let sev = self
                .of(&store.at(i))
                .unwrap_or_else(|| store.base_severity(i));
            store.set_severity(i, sev);
        }
    }

    /// Applies the rules to the new entries of a read result.
    pub fn apply_to_result(&self, res: &mut ReadResult) {
        if self.is_empty() {
            return;
        }
        match res {
            ReadResult::Reset {
                committed,
                provisional,
            }
            | ReadResult::Appended {
                committed,
                provisional,
                ..
            } => {
                self.apply(committed, 0);
                self.apply(provisional, 0);
            }
            ReadResult::Unchanged => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::LogEntry;

    #[test]
    fn bound_sets_follow_the_definitions_of_the_log() {
        let set = |defs: &[&str]| RuleSet {
            name: "s".into(),
            logdefs: defs.iter().map(|d| d.to_string()).collect(),
            rules: vec![rule("boom", Some(RuleSeverity::Error))],
            ..RuleSet::default()
        };
        let ids = |v: &[&str]| -> Vec<String> { v.iter().map(|s| s.to_string()).collect() };
        // Nothing bound: the definitions do not matter.
        assert!(SevRules::for_new_defs(&[set(&[])], &ids(&[]), &ids(&["cbs.log.def"])).is_none());
        let bound = [set(&["cbs.log.def"])];
        // Same definitions: nothing to change.
        assert!(SevRules::for_new_defs(&bound, &ids(&["a"]), &ids(&["a"])).is_none());
        // The bound definition appears / disappears.
        let on = SevRules::for_new_defs(&bound, &ids(&[]), &ids(&["cbs.log.def"])).unwrap();
        assert!(!on.is_empty());
        let off = SevRules::for_new_defs(&bound, &ids(&["cbs.log.def"]), &ids(&["x"])).unwrap();
        assert!(off.is_empty());
    }

    fn store(lines: &[(&str, Severity)]) -> LogStore {
        let mut s = LogStore::new();
        for (i, (m, sev)) in lines.iter().enumerate() {
            s.push(
                LogEntry {
                    message: (*m).into(),
                    severity: *sev,
                    line_no: i + 1,
                    component: String::new(),
                    datetime: String::new(),
                    thread: String::new(),
                    extra: Vec::new(),
                    raw: (0, 0),
                },
                0,
            );
        }
        s
    }

    fn rule(pattern: &str, sev: Option<RuleSeverity>) -> HighlightRule {
        HighlightRule {
            pattern: pattern.into(),
            is_regex: true,
            severity: sev,
            ..Default::default()
        }
    }

    #[test]
    fn first_rule_with_a_severity_wins_and_unmatched_rows_keep_theirs() {
        let mut s = store(&[
            ("0 errors", Severity::Error),
            ("exit code 1603", Severity::Info),
            ("fine", Severity::Warning),
            ("exit code 0 errors", Severity::Info),
        ]);
        let rules = SevRules::from_config(&[
            rule("colour only", None),
            rule(r"\b0 errors\b", Some(RuleSeverity::Info)),
            rule(r"exit code [1-9]\d*", Some(RuleSeverity::Error)),
        ]);
        rules.apply(&mut s, 0);
        let sev: Vec<_> = (0..4).map(|i| s.severity(i)).collect();
        assert_eq!(
            sev,
            [
                Severity::Info,
                Severity::Error,
                Severity::Warning,
                Severity::Info
            ]
        );
        // Without rules the parsed severity returns.
        SevRules::default().apply(&mut s, 0);
        assert_eq!(s.severity(0), Severity::Error);
        assert_eq!(s.severity(1), Severity::Info);
        // The base survives copying / appending.
        rules.apply(&mut s, 0);
        let mut t = LogStore::new();
        t.append(s);
        assert_eq!(t.severity(1), Severity::Error);
        assert_eq!(t.base_severity(1), Severity::Info);
    }

    #[test]
    fn disabled_invalid_and_empty_rules_are_ignored_and_equality_follows_the_rules() {
        let mut off = rule("x", Some(RuleSeverity::Error));
        off.enabled = false;
        let bad = rule("(", Some(RuleSeverity::Error));
        let empty = rule("", Some(RuleSeverity::Error));
        assert!(SevRules::from_config(&[off, bad, empty]).is_empty());
        let a = SevRules::from_config(&[rule("a", Some(RuleSeverity::Error))]);
        let colour_changed = {
            let mut r = rule("a", Some(RuleSeverity::Error));
            r.use_bg = false;
            SevRules::from_config(&[r])
        };
        assert!(a == colour_changed);
        assert!(a != SevRules::from_config(&[rule("a", Some(RuleSeverity::Info))]));
    }

    #[test]
    fn rule_severity_in_json_is_optional() {
        let old: HighlightRule = serde_json::from_str(r#"{"pattern":"x"}"#).unwrap();
        assert_eq!(old.severity, None);
        let new: HighlightRule =
            serde_json::from_str(r#"{"pattern":"x","severity":"warning"}"#).unwrap();
        assert_eq!(new.severity, Some(RuleSeverity::Warning));
        assert!(!serde_json::to_string(&old).unwrap().contains("severity"));
    }
}
