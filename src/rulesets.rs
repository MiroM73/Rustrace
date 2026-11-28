//! Highlight rule sets as files (#70): the file format of an exported set, the built-in sets
//! and the import of a file (a set file, or the older bare list of rules).

use crate::config::{HighlightRule, RuleSet, RuleSetFile};

/// Built-in sets (file name, content): embedded in the program, added with *Add built-in set*.
pub const BUILTIN: &[(&str, &str)] = &[
    ("cbs-dism.rules", include_str!("../rulesets/cbs-dism.rules")),
    ("msi.rules", include_str!("../rulesets/msi.rules")),
    (
        "sccm-client.rules",
        include_str!("../rulesets/sccm-client.rules"),
    ),
];

/// What a rules file contains.
#[derive(Debug, PartialEq)]
pub enum RulesFile {
    /// A set with a name and a binding (the format of *Export…*).
    Set(RuleSetFile),
    /// A bare list of rules (the format before rule sets, #51).
    Rules(Vec<HighlightRule>),
}

/// Reads a rules file: a set file first, else a list of rules.
pub fn parse_rules_file(text: &str) -> Result<RulesFile, String> {
    match serde_json::from_str::<RuleSetFile>(text) {
        Ok(f) => Ok(RulesFile::Set(f)),
        Err(set_err) => serde_json::from_str::<Vec<HighlightRule>>(text)
            .map(RulesFile::Rules)
            // The message of the set format is the more useful one for a broken set file.
            .map_err(|list_err| {
                if text.trim_start().starts_with('{') {
                    set_err.to_string()
                } else {
                    list_err.to_string()
                }
            }),
    }
}

/// The built-in sets, in the order of `BUILTIN`.
pub fn builtin_sets() -> Vec<RuleSetFile> {
    BUILTIN
        .iter()
        .map(|(name, text)| {
            serde_json::from_str(text).unwrap_or_else(|e| panic!("built-in rule set {name}: {e}"))
        })
        .collect()
}

/// The content of the set for the settings: switched on, with the binding of the file.
pub fn to_rule_set(f: RuleSetFile) -> RuleSet {
    RuleSet {
        name: f.name,
        enabled: true,
        logdefs: f.logdefs,
        rules: f.rules,
    }
}

/// Adds a set file to the sets (#70): a set of the same name gets the rules without duplicates
/// (`import_rules`), else it is appended. Returns (index of the set, what happened to the rules).
pub fn import_set(sets: &mut Vec<RuleSet>, f: RuleSetFile) -> (usize, crate::config::RuleImport) {
    match sets.iter().position(|s| s.name == f.name) {
        Some(i) => {
            let res = crate::config::import_rules(&mut sets[i].rules, f.rules);
            (i, res)
        }
        None => {
            let res = crate::config::RuleImport {
                added: f.rules.len(),
                ..Default::default()
            };
            let mut set = to_rule_set(f);
            for r in &mut set.rules {
                r.fill_missing_dark();
            }
            sets.push(set);
            (sets.len() - 1, res)
        }
    }
}

/// The sets as they were when the rules window was opened (#78), found by name. A set that
/// appears later (new, duplicate, import) is remembered as it was first seen.
#[derive(Default)]
pub struct Baseline(Vec<RuleSet>);

impl Baseline {
    /// Remembers the sets not known yet; a known set keeps its remembered state.
    pub fn observe(&mut self, sets: &[RuleSet]) {
        for s in sets {
            if self.find(&s.name).is_none() {
                self.0.push(s.clone());
            }
        }
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// The rules of the set differ from the remembered ones.
    pub fn changed(&self, set: &RuleSet) -> bool {
        self.find(&set.name).is_some_and(|b| b.rules != set.rules)
    }

    /// A renamed set keeps its remembered state.
    pub fn rename(&mut self, from: &str, to: &str) {
        if from == to {
            return;
        }
        self.forget(to);
        if let Some(b) = self.0.iter_mut().find(|b| b.name == from) {
            b.name = to.to_owned();
        }
    }

    /// Drops a deleted set: a later set of the same name is a new one.
    pub fn forget(&mut self, name: &str) {
        self.0.retain(|b| b.name != name);
    }

    /// *Save as new set* (#78): set `i` as it is now becomes a new set `name`, inserted before
    /// it (so its rules keep the priority they had), and set `i` gets its remembered rules and
    /// binding back; its name and On state stay. Returns `false` (nothing done) for a set that
    /// was never observed.
    pub fn save_as_new(&self, sets: &mut Vec<RuleSet>, i: usize, name: String) -> bool {
        let Some(base) = self.find(&sets[i].name) else {
            return false;
        };
        let mut new = sets[i].clone();
        new.name = name;
        sets[i].rules = base.rules.clone();
        sets[i].logdefs = base.logdefs.clone();
        sets.insert(i, new);
        true
    }

    fn find(&self, name: &str) -> Option<&RuleSet> {
        self.0.iter().find(|b| b.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_sets_parse_and_use_known_log_definitions() {
        let sets = builtin_sets();
        assert_eq!(sets.len(), BUILTIN.len());
        for s in &sets {
            assert!(!s.name.is_empty() && !s.rules.is_empty(), "{}", s.name);
            for r in &s.rules {
                assert!(
                    crate::app::compile_pattern(&r.pattern, r.is_regex, r.case_sensitive).is_ok(),
                    "{}: {}",
                    s.name,
                    r.name
                );
                assert!(
                    r.fg_dark[3] > 0 && r.bg_dark[3] > 0,
                    "{}: dark colors",
                    r.name
                );
            }
            for d in &s.logdefs {
                assert!(
                    crate::logdef::BUILTIN.iter().any(|(id, _)| id == d),
                    "{}: unknown log definition {d}",
                    s.name
                );
            }
        }
        let mut names: Vec<_> = sets.iter().map(|s| &s.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), sets.len());
    }

    #[test]
    fn rules_files_of_both_formats_are_read() {
        let set = RuleSetFile {
            name: "S".into(),
            logdefs: vec!["cbs.log.def".into()],
            rules: vec![HighlightRule {
                name: "r".into(),
                pattern: "x".into(),
                ..Default::default()
            }],
        };
        let json = serde_json::to_string_pretty(&set).unwrap();
        assert_eq!(parse_rules_file(&json), Ok(RulesFile::Set(set)));
        // The shared list of the MSI guide (#51) is a bare list.
        let list = include_str!("../docs/guides/rules/msi-verbose-log.json");
        match parse_rules_file(list) {
            Ok(RulesFile::Rules(r)) => assert_eq!(r.len(), 3),
            other => panic!("{other:?}"),
        }
        assert!(parse_rules_file("{\"name\": 1}").is_err());
        assert!(parse_rules_file("nonsense").is_err());
    }

    fn rule(name: &str) -> HighlightRule {
        HighlightRule {
            name: name.into(),
            pattern: name.into(),
            ..Default::default()
        }
    }

    #[test]
    fn saving_as_a_new_set_restores_the_original() {
        let mut sets = vec![
            RuleSet::new("Other".into(), Vec::new()),
            RuleSet::new("A".into(), vec![rule("old")]),
        ];
        let mut base = Baseline::default();
        // Never observed: nothing to go back to.
        assert!(!base.save_as_new(&mut sets, 1, "B".into()));
        base.observe(&sets);
        assert!(!base.changed(&sets[1]));
        sets[1].rules.insert(0, rule("added"));
        sets[1].logdefs = vec!["cbs.log.def".into()];
        sets[1].enabled = false;
        assert!(base.changed(&sets[1]) && !base.changed(&sets[0]));
        // Observing again keeps the first state.
        base.observe(&sets);
        assert!(base.changed(&sets[1]));

        assert!(base.save_as_new(&mut sets, 1, "B".into()));
        let names: Vec<_> = sets.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Other", "B", "A"]);
        assert_eq!(sets[1].rules, [rule("added"), rule("old")]);
        assert_eq!(sets[1].logdefs, ["cbs.log.def"]);
        assert_eq!(sets[2].rules, [rule("old")]);
        assert!(sets[2].logdefs.is_empty());
        assert!(!sets[1].enabled && !sets[2].enabled);
        base.observe(&sets);
        assert!(!base.changed(&sets[1]) && !base.changed(&sets[2]));
    }

    #[test]
    fn baseline_follows_rename_and_delete() {
        let mut sets = vec![RuleSet::new("A".into(), vec![rule("old")])];
        let mut base = Baseline::default();
        base.observe(&sets);
        sets[0].rules.push(rule("added"));
        base.rename("A", "A");
        assert!(base.changed(&sets[0]));
        base.rename("A", "C");
        sets[0].name = "C".into();
        base.observe(&sets);
        assert!(base.changed(&sets[0]));
        // Undoing the change by hand: unchanged again.
        sets[0].rules.pop();
        assert!(!base.changed(&sets[0]));
        base.forget("C");
        sets[0].rules.clear();
        base.observe(&sets);
        assert!(!base.changed(&sets[0]));
    }

    #[test]
    fn importing_a_set_twice_merges_by_name() {
        let file = || builtin_sets().remove(0);
        let mut sets = vec![RuleSet::new("Mine".into(), Vec::new())];
        let (i, first) = import_set(&mut sets, file());
        assert_eq!((i, first.added), (1, 3));
        assert_eq!(sets[1].logdefs, ["cbs.log.def", "dism.log.def"]);
        let (j, second) = import_set(&mut sets, file());
        assert_eq!((j, sets.len()), (1, 2));
        assert_eq!((second.added, second.updated, second.skipped), (0, 0, 3));
    }
}
