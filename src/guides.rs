//! Guides (#39) – the step-by-step troubleshooting guides in `docs/guides/` of the repository,
//! opened on GitHub from Help → Guides, the About window and the log set window.
//!
//! Adding a guide = one line in `GUIDES` + its title key in the language files.

/// The guides in menu order: (language key of the title, file in `docs/guides/`).
pub const GUIDES: &[(&str, &str)] = &[
    ("help.guide.failed_app_install", "failed-app-install.md"),
    ("help.guide.software_updates", "software-updates.md"),
    ("help.guide.client_install", "client-install.md"),
    ("help.guide.iis_management_point", "iis-management-point.md"),
    ("help.guide.client_msi_1603", "client-msi-1603.md"),
];

/// The index of all guides.
pub const INDEX: &str = "README.md";

/// URL of a file in `docs/guides/` on GitHub.
pub fn url(repository: &str, file: &str) -> String {
    format!(
        "{}/blob/master/docs/guides/{file}",
        repository.trim_end_matches('/')
    )
}

/// What a `guide = …` line of a log set opens: a URL or a path stays as it is, a plain file
/// name (`failed-app-install.md`) is a guide of the repository.
pub fn target(repository: &str, value: &str) -> String {
    let v = value.trim();
    // A URL (`://`), an absolute path (`C:`) or any path with a folder.
    if v.contains(['\\', '/', ':']) {
        v.to_owned()
    } else {
        url(repository, v)
    }
}

/// Language key of the title of a built-in guide (`None` = not one of `GUIDES`).
pub fn title_key(file: &str) -> Option<&'static str> {
    GUIDES
        .iter()
        .find(|(_, f)| f.eq_ignore_ascii_case(file.trim()))
        .map(|(k, _)| *k)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPO: &str = "https://github.com/MiroM73/Rustrace";

    #[test]
    fn urls_and_targets() {
        assert_eq!(
            url(REPO, "software-updates.md"),
            "https://github.com/MiroM73/Rustrace/blob/master/docs/guides/software-updates.md"
        );
        assert_eq!(url(&format!("{REPO}/"), INDEX), url(REPO, "README.md"));
        assert_eq!(
            target(REPO, " client-install.md "),
            url(REPO, "client-install.md")
        );
        assert_eq!(
            target(REPO, "https://intranet/wiki/Logs"),
            "https://intranet/wiki/Logs"
        );
        assert_eq!(target(REPO, r"C:\Guides\mine.md"), r"C:\Guides\mine.md");
        assert_eq!(target(REPO, r"\\srv\share\g.html"), r"\\srv\share\g.html");
        assert_eq!(
            title_key("Client-Install.md"),
            Some("help.guide.client_install")
        );
        assert_eq!(title_key("mine.md"), None);
    }

    /// Every guide of the table exists and is listed in the index; every guide file is in the
    /// table (a new guide must not be forgotten in the menu).
    #[test]
    fn table_matches_the_guide_files() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/guides");
        let index = std::fs::read_to_string(dir.join(INDEX)).unwrap();
        for (_, f) in GUIDES {
            assert!(dir.join(f).is_file(), "{f} missing");
            assert!(index.contains(&format!("]({f})")), "{f} not in the index");
        }
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.ends_with(".md") && name != INDEX {
                assert!(title_key(&name).is_some(), "{name} not in GUIDES");
            }
        }
    }
}
