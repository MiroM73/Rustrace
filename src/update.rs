//! New versions (#15, #31): the published GitHub releases newer than this program, download of its ZIP (checked against the
//! `.sha256` file published next to it), installation over the running program and the state
//! handed to the restarted program.
//!
//! Installing on Windows: the running `rustrace.exe` cannot be overwritten, but it can be
//! renamed – it becomes `rustrace.exe.old` (deleted by the next start, `cleanup_old`) and the new
//! exe takes its name; the language files, log definitions and LICENSE next to it are replaced
//! by the ones from the archive (other files there are left alone).

use crate::logdef::ColumnKind;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// The release archive this program installs from (`RusTrace-v<version>-windows-x64.zip`).
const ZIP_SUFFIX: &str = "-windows-x64.zip";
/// Upper limit for downloads (the ZIP has a few MB).
const MAX_DOWNLOAD: u64 = 200 << 20;

#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub name: String,
    pub url: String,
}

/// A published release.
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    /// Version without the leading "v", e.g. "0.7.0".
    pub version: String,
    /// Publication date "YYYY-MM-DD" (empty when unknown).
    pub date: String,
    /// Release notes (Markdown as written on GitHub).
    pub notes: String,
    /// Web page of the release.
    pub page: String,
    pub zip: Option<Asset>,
    pub sha256: Option<Asset>,
}

/// `major.minor.patch` of "v0.7.0" / "0.7.0" (a suffix like "-beta" is ignored).
pub fn parse_version(s: &str) -> Option<[u64; 3]> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let s = s.split(['-', '+']).next()?;
    let mut parts = s.split('.').map(|p| p.parse::<u64>().ok());
    let v = [
        parts.next()??,
        parts.next()??,
        parts.next().unwrap_or(Some(0))?,
    ];
    parts.next().is_none().then_some(v)
}

/// `latest` is a higher version than `current`.
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

/// The version this program compares with: its own, unless `RUSTRACE_UPDATE_AS_VERSION` pretends
/// an older one (for testing the update).
pub fn current_version() -> String {
    std::env::var("RUSTRACE_UPDATE_AS_VERSION").unwrap_or_else(|_| crate::app::VERSION.to_owned())
}

/// GitHub API URL of the releases (newest first, up to 100) of a `https://github.com/owner/repo`
/// project.
pub fn releases_url(repository: &str) -> Option<String> {
    let rest = repository
        .trim_end_matches('/')
        .strip_prefix("https://github.com/")?;
    let mut it = rest.split('/');
    let (owner, repo) = (it.next()?, it.next()?);
    (it.next().is_none() && !owner.is_empty() && !repo.is_empty())
        .then(|| format!("https://api.github.com/repos/{owner}/{repo}/releases?per_page=100"))
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    html_url: String,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

impl GhRelease {
    fn into_release(self) -> Result<Release, String> {
        if parse_version(&self.tag_name).is_none() {
            return Err(format!("unexpected version {}", self.tag_name));
        }
        let asset = |pred: &dyn Fn(&str) -> bool| {
            self.assets.iter().find(|a| pred(&a.name)).map(|a| Asset {
                name: a.name.clone(),
                url: a.browser_download_url.clone(),
            })
        };
        let zip = asset(&|n: &str| n.ends_with(ZIP_SUFFIX));
        let sha256 = zip
            .as_ref()
            .and_then(|z| asset(&|n: &str| n.eq_ignore_ascii_case(&format!("{}.sha256", z.name))));
        Ok(Release {
            version: self.tag_name.trim_start_matches(['v', 'V']).to_owned(),
            date: self
                .published_at
                .and_then(|d| d.get(..10).map(str::to_owned))
                .unwrap_or_default(),
            notes: self.body.unwrap_or_default().replace("\r\n", "\n"),
            page: self.html_url,
            zip,
            sha256,
        })
    }
}

/// Reads one release from the GitHub API answer (`releases/latest`; the program reads the list).
#[cfg(test)]
pub fn parse_release(json: &str) -> Result<Release, String> {
    serde_json::from_str::<GhRelease>(json)
        .map_err(|e| e.to_string())?
        .into_release()
}

/// Reads the list of releases from the GitHub API answer: drafts, pre-releases and tags that are
/// not a version are left out; newest version first.
pub fn parse_releases(json: &str) -> Result<Vec<Release>, String> {
    let list: Vec<GhRelease> = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let mut releases: Vec<Release> = list
        .into_iter()
        .filter(|r| !r.draft && !r.prerelease)
        .filter_map(|r| r.into_release().ok())
        .collect();
    releases.sort_by_key(|r| std::cmp::Reverse(parse_version(&r.version)));
    Ok(releases)
}

/// The releases newer than `current` (the ones the user has missed), newest first.
pub fn missed_releases(all: &[Release], current: &str) -> Vec<Release> {
    let mut missed: Vec<Release> = all
        .iter()
        .filter(|r| is_newer(&r.version, current))
        .cloned()
        .collect();
    missed.sort_by_key(|r| std::cmp::Reverse(parse_version(&r.version)));
    missed
}

/// Result of a check: the latest release (installed from) and the missed ones.
#[derive(Clone, Debug, PartialEq)]
pub struct Updates {
    pub latest: Release,
    /// Releases newer than this program, newest first (empty = up to date).
    pub missed: Vec<Release>,
}

impl Updates {
    /// `None` when there is no published release at all.
    pub fn new(all: &[Release], current: &str) -> Option<Self> {
        let latest = all
            .iter()
            .max_by_key(|r| parse_version(&r.version))?
            .clone();
        Some(Self {
            latest,
            missed: missed_releases(all, current),
        })
    }

    pub fn is_newer(&self) -> bool {
        !self.missed.is_empty()
    }
}

/// Release notes (GitHub Markdown) as plain lines for the window: (is a heading, text).
/// Headings lose their "#", list items get "•", bold / code marks are removed, empty lines dropped.
pub fn plain_notes(notes: &str) -> Vec<(bool, String)> {
    notes
        .lines()
        .filter_map(|line| {
            let line = line.trim_end().replace("**", "").replace('`', "");
            let trimmed = line.trim_start();
            if trimmed.is_empty() {
                None
            } else if trimmed.starts_with('#') {
                Some((true, trimmed.trim_start_matches('#').trim().to_owned()))
            } else if let Some(item) = trimmed
                .strip_prefix("* ")
                .or_else(|| trimmed.strip_prefix("- "))
            {
                let indent = "    ".repeat((line.len() - trimmed.len()) / 2);
                Some((false, format!("{indent}• {item}")))
            } else {
                Some((false, trimmed.to_owned()))
            }
        })
        .collect()
}

/// Asks GitHub for the published releases and compares them with this program (blocking – call
/// it in a background thread).
pub fn fetch_updates() -> Result<Updates, String> {
    let url = releases_url(env!("CARGO_PKG_REPOSITORY")).ok_or("no GitHub repository")?;
    let body = crate::http::get(&url, "application/vnd.github+json", |n, _| n < 16 << 20)?;
    let all = parse_releases(&String::from_utf8_lossy(&body))?;
    Updates::new(&all, &current_version()).ok_or_else(|| "no published release".to_owned())
}

/// The checksum for `name` from a `sha256sum`-style file ("<hex>  <name>", or only the hex).
pub fn parse_sha256_file(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let mut it = line.split_whitespace();
        let hash = it.next()?;
        let file = it.next().map(|f| f.trim_start_matches('*'));
        let ok = hash.len() == 64
            && hash.chars().all(|c| c.is_ascii_hexdigit())
            && file.is_none_or(|f| f.eq_ignore_ascii_case(name));
        ok.then(|| hash.to_ascii_lowercase())
    })
}

pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn u16_at(b: &[u8], i: usize) -> Option<usize> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?) as usize)
}

fn u32_at(b: &[u8], i: usize) -> Option<usize> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?) as usize)
}

/// The files of a ZIP archive (stored or deflated; directories skipped), CRC-checked.
/// Names use "/" and must stay inside the target folder (no absolute paths, no "..").
pub fn unzip(data: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let bad = || "damaged ZIP archive".to_owned();
    // End of central directory: the last "PK\x05\x06" (followed by up to 64 KB of comment).
    let from = data.len().saturating_sub(22 + 0xFFFF);
    let eocd = (from..data.len().saturating_sub(21))
        .rev()
        .find(|&i| data[i..i + 4] == [0x50, 0x4B, 0x05, 0x06])
        .ok_or_else(bad)?;
    let count = u16_at(data, eocd + 10).ok_or_else(bad)?;
    let mut pos = u32_at(data, eocd + 16).ok_or_else(bad)?;
    let mut files = Vec::new();
    for _ in 0..count {
        if data.get(pos..pos + 4) != Some(&[0x50, 0x4B, 0x01, 0x02]) {
            return Err(bad());
        }
        let method = u16_at(data, pos + 10).ok_or_else(bad)?;
        let crc = u32_at(data, pos + 16).ok_or_else(bad)? as u32;
        let packed = u32_at(data, pos + 20).ok_or_else(bad)?;
        let size = u32_at(data, pos + 24).ok_or_else(bad)?;
        let name_len = u16_at(data, pos + 28).ok_or_else(bad)?;
        let extra_len = u16_at(data, pos + 30).ok_or_else(bad)?;
        let comment_len = u16_at(data, pos + 32).ok_or_else(bad)?;
        let local = u32_at(data, pos + 42).ok_or_else(bad)?;
        let name = data.get(pos + 46..pos + 46 + name_len).ok_or_else(bad)?;
        let name = String::from_utf8_lossy(name).replace('\\', "/");
        pos += 46 + name_len + extra_len + comment_len;
        if name.ends_with('/') {
            continue;
        }
        if name.starts_with('/') || name.contains(':') || name.split('/').any(|p| p == "..") {
            return Err(format!("unsafe path in the archive: {name}"));
        }
        if data.get(local..local + 4) != Some(&[0x50, 0x4B, 0x03, 0x04]) {
            return Err(bad());
        }
        let start = local
            + 30
            + u16_at(data, local + 26).ok_or_else(bad)?
            + u16_at(data, local + 28).ok_or_else(bad)?;
        let raw = data.get(start..start + packed).ok_or_else(bad)?;
        let content = match method {
            0 => raw.to_vec(),
            8 => {
                let mut out = Vec::with_capacity(size);
                flate2::read::DeflateDecoder::new(raw)
                    .read_to_end(&mut out)
                    .map_err(|e| format!("{name}: {e}"))?;
                out
            }
            m => return Err(format!("{name}: unsupported compression method {m}")),
        };
        if content.len() != size || crc32fast::hash(&content) != crc {
            return Err(format!("{name}: checksum mismatch"));
        }
        files.push((name, content));
    }
    Ok(files)
}

/// Download / installation progress shared with the UI.
#[derive(Default)]
pub struct Progress {
    pub received: AtomicU64,
    /// 0 = unknown.
    pub total: AtomicU64,
    pub cancel: AtomicBool,
}

/// Downloads the release ZIP, checks it and installs it over the program at `exe`.
/// Returns warnings about files that could not be replaced (the program itself was).
pub fn download_and_install(
    release: &Release,
    exe: &Path,
    progress: &Progress,
) -> Result<Vec<String>, String> {
    let zip = release
        .zip
        .as_ref()
        .ok_or("the release has no Windows ZIP archive")?;
    let sha = release
        .sha256
        .as_ref()
        .ok_or("the release has no checksum file")?;
    let sha_text = crate::http::get(&sha.url, "application/octet-stream", |n, _| n < 1 << 20)?;
    let expected = parse_sha256_file(&String::from_utf8_lossy(&sha_text), &zip.name)
        .ok_or("the checksum file could not be read")?;
    let data = crate::http::get(&zip.url, "application/octet-stream", |n, total| {
        progress.received.store(n, Ordering::Relaxed);
        progress.total.store(total.unwrap_or(0), Ordering::Relaxed);
        n <= MAX_DOWNLOAD && !progress.cancel.load(Ordering::Relaxed)
    })?;
    if sha256_hex(&data) != expected {
        return Err(format!("{}: checksum mismatch", zip.name));
    }
    install(unzip(&data)?, exe)
}

/// Path the replaced program is renamed to.
pub fn old_exe_path(exe: &Path) -> PathBuf {
    let mut s = exe.as_os_str().to_owned();
    s.push(".old");
    PathBuf::from(s)
}

/// Archive files relative to the program folder: the common top folder ("RusTrace/") removed.
fn strip_top_folder(files: Vec<(String, Vec<u8>)>) -> Vec<(String, Vec<u8>)> {
    let top = files
        .first()
        .and_then(|(n, _)| n.split_once('/'))
        .map(|(t, _)| format!("{t}/"));
    match top {
        Some(top) if files.iter().all(|(n, _)| n.starts_with(&top)) => files
            .into_iter()
            .map(|(n, d)| (n[top.len()..].to_owned(), d))
            .collect(),
        _ => files,
    }
}

/// Puts the files of the archive into the program folder; `rustrace.exe` replaces `exe`.
pub fn install(files: Vec<(String, Vec<u8>)>, exe: &Path) -> Result<Vec<String>, String> {
    let files = strip_top_folder(files);
    let dir = exe.parent().ok_or("unknown program folder")?;
    let new_exe = files
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("rustrace.exe"))
        .ok_or("rustrace.exe is missing in the archive")?;

    // Write access first, so that nothing is changed when the folder is read-only (e.g. Program Files).
    let probe = dir.join(".rustrace-update-probe");
    std::fs::write(&probe, b"")
        .map_err(|e| format!("no write access to {}: {e}", dir.display()))?;
    let _ = std::fs::remove_file(&probe);

    let old = old_exe_path(exe);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(exe, &old).map_err(|e| format!("{}: {e}", exe.display()))?;
    if let Err(e) = std::fs::write(exe, &new_exe.1) {
        let _ = std::fs::rename(&old, exe);
        return Err(format!("{}: {e}", exe.display()));
    }

    let mut warnings = Vec::new();
    for (name, data) in &files {
        if name.eq_ignore_ascii_case("rustrace.exe") {
            continue;
        }
        let path = dir.join(name);
        let res = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|_| std::fs::write(&path, data));
        if let Err(e) = res {
            warnings.push(format!("{}: {e}", path.display()));
        }
    }
    Ok(warnings)
}

/// Deletes `rustrace.exe.old` left by an update (in the background: the old program may still be
/// closing for a moment).
pub fn cleanup_old() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let old = old_exe_path(&exe);
    if !old.exists() {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("rustrace-cleanup".into())
        .spawn(move || {
            for _ in 0..20 {
                if std::fs::remove_file(&old).is_ok() || !old.exists() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        });
}

/// Window position and size (logical points).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    pub pos: Option<[f32; 2]>,
    pub size: Option<[f32; 2]>,
    pub maximized: bool,
}

/// Log definition chosen manually (View → Log format).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FormatState {
    Builtin,
    Def { id: String, priority: u8 },
}

/// What the program shows, handed to the new version started after an update
/// (`rustrace.exe --restore <file>`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionState {
    pub path: Option<String>,
    /// The other logs of a merged view.
    pub extra_paths: Vec<String>,
    pub format: Option<FormatState>,
    /// Index of the selected entry.
    pub selected: Option<usize>,
    pub search_text: String,
    pub search_confirmed: bool,
    pub search_regex: bool,
    pub search_case: bool,
    pub filter_mode: bool,
    pub show_sev: [bool; 3],
    pub col_filters: Vec<(ColumnKind, String)>,
    pub time_from: String,
    pub time_to: String,
    pub window: Option<WindowState>,
}

/// Command line switch of the restarted program.
pub const RESTORE_ARG: &str = "--restore";

impl SessionState {
    /// Saves the state into a temporary file for the new program.
    pub fn save_temp(&self) -> Result<PathBuf, String> {
        let path =
            std::env::temp_dir().join(format!("rustrace-restore-{}.json", std::process::id()));
        let json = serde_json::to_string(self).map_err(|e| e.to_string())?;
        std::fs::write(&path, json).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(path)
    }

    /// Reads (and deletes) the state file given by `--restore`.
    pub fn take(path: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok();
        let _ = std::fs::remove_file(path);
        serde_json::from_str(&text?).ok()
    }
}

/// Starts the installed program with the saved state; the caller then closes this one.
pub fn restart(exe: &Path, state: &SessionState) -> Result<(), String> {
    let file = state.save_temp()?;
    std::process::Command::new(exe)
        .arg(RESTORE_ARG)
        .arg(&file)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{}: {e}", exe.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logdef::Role;

    #[test]
    fn versions() {
        assert_eq!(parse_version("v0.7.0"), Some([0, 7, 0]));
        assert_eq!(parse_version("1.2"), Some([1, 2, 0]));
        assert_eq!(parse_version("0.7.0-beta"), Some([0, 7, 0]));
        assert_eq!(parse_version("x"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
        assert!(is_newer("v0.10.0", "0.9.5"));
        assert!(is_newer("0.6.2", "0.6.1"));
        assert!(!is_newer("v0.6.1", "0.6.1"));
        assert!(!is_newer("0.5.9", "0.6.0"));
        assert!(!is_newer("garbage", "0.6.0"));
    }

    #[test]
    fn api_url() {
        assert_eq!(
            releases_url("https://github.com/MiroM73/Rustrace").as_deref(),
            Some("https://api.github.com/repos/MiroM73/Rustrace/releases?per_page=100")
        );
        assert_eq!(releases_url("https://example.com/a/b"), None);
    }

    fn release_json(tag: &str, extra: &str) -> String {
        format!(
            r#"{{"tag_name":"{tag}","html_url":"https://g/{tag}","body":"notes {tag}",
            "published_at":"2026-10-2{}T10:00:00Z"{extra}}}"#,
            tag.len() % 10
        )
    }

    #[test]
    fn releases_list_from_github_json() {
        let list = [
            release_json("v0.7.0", ""),
            release_json("v0.9.0", r#","draft":true"#),
            release_json("v0.8.1-beta", r#","prerelease":true"#),
            release_json("v0.10.0", r#","draft":false,"prerelease":false"#),
            release_json("nightly", ""),
            release_json("v0.8.0", ""),
        ]
        .join(",");
        let all = parse_releases(&format!("[{list}]")).unwrap();
        let versions: Vec<&str> = all.iter().map(|r| r.version.as_str()).collect();
        // Drafts, pre-releases and tags that are no version left out; newest first.
        assert_eq!(versions, ["0.10.0", "0.8.0", "0.7.0"]);
        assert_eq!(all[0].date, "2026-10-27");
        assert_eq!(all[0].notes, "notes v0.10.0");
        assert!(parse_releases("[]").unwrap().is_empty());
        assert!(parse_releases(r#"{"message":"API rate limit exceeded"}"#).is_err());
        // Without a date.
        let r = parse_release(r#"{"tag_name":"v1.0.0","html_url":"h"}"#).unwrap();
        assert_eq!(r.date, "");
    }

    #[test]
    fn missed_versions() {
        let tags = ["v0.5.0", "v0.6.0", "v0.6.1", "v0.7.0", "v0.8.0"];
        let list: Vec<String> = tags.iter().map(|t| release_json(t, "")).collect();
        let all = parse_releases(&format!("[{}]", list.join(","))).unwrap();
        let versions =
            |rs: &[Release]| -> Vec<String> { rs.iter().map(|r| r.version.clone()).collect() };
        assert_eq!(
            versions(&missed_releases(&all, "0.5.0")),
            ["0.8.0", "0.7.0", "0.6.1", "0.6.0"]
        );
        assert_eq!(
            versions(&missed_releases(&all, "0.6.1")),
            ["0.8.0", "0.7.0"]
        );
        // Equal to the latest or newer (a local build): nothing missed.
        assert!(missed_releases(&all, "0.8.0").is_empty());
        assert!(missed_releases(&all, "0.9.0").is_empty());

        let u = Updates::new(&all, "0.6.0").unwrap();
        assert_eq!(u.latest.version, "0.8.0");
        assert!(u.is_newer());
        assert_eq!(u.missed.len(), 3);
        let u = Updates::new(&all, "0.8.0").unwrap();
        assert_eq!(u.latest.version, "0.8.0");
        assert!(!u.is_newer());
        assert_eq!(Updates::new(&[], "0.8.0"), None);
    }

    #[test]
    fn release_from_github_json() {
        let json = r###"{"tag_name":"v0.7.0","html_url":"https://github.com/o/r/releases/tag/v0.7.0",
            "body":"## What's new\r\n* x","assets":[
            {"name":"RusTrace-v0.7.0-windows-x64.exe","browser_download_url":"https://d/exe"},
            {"name":"RusTrace-v0.7.0-windows-x64.zip.sha256","browser_download_url":"https://d/sha"},
            {"name":"RusTrace-v0.7.0-windows-x64.zip","browser_download_url":"https://d/zip"}]}"###;
        let r = parse_release(json).unwrap();
        assert_eq!(r.version, "0.7.0");
        assert_eq!(r.notes, "## What's new\n* x");
        assert_eq!(r.zip.unwrap().url, "https://d/zip");
        assert_eq!(r.sha256.unwrap().url, "https://d/sha");

        let bare = r#"{"tag_name":"v1.0.0","html_url":"h","body":null}"#;
        let r = parse_release(bare).unwrap();
        assert!(r.zip.is_none() && r.sha256.is_none() && r.notes.is_empty());
        assert!(parse_release(r#"{"tag_name":"latest","html_url":"h"}"#).is_err());
    }

    #[test]
    fn notes_as_plain_lines() {
        let notes = "## What's new\n* **Splash** – shown at `start` (#11)\n  - detail\n\nFull text";
        assert_eq!(
            plain_notes(notes),
            vec![
                (true, "What's new".to_owned()),
                (false, "• Splash – shown at start (#11)".to_owned()),
                (false, "    • detail".to_owned()),
                (false, "Full text".to_owned()),
            ]
        );
    }

    #[test]
    fn sha256_files() {
        let h = "a".repeat(64);
        assert_eq!(
            parse_sha256_file(&format!("{h}  RusTrace.zip\n"), "rustrace.zip"),
            Some(h.clone())
        );
        assert_eq!(
            parse_sha256_file(&format!("{h} *RusTrace.zip"), "RusTrace.zip"),
            Some(h.clone())
        );
        assert_eq!(parse_sha256_file(&h.to_uppercase(), "x"), Some(h.clone()));
        assert_eq!(
            parse_sha256_file(&format!("{h}  other.zip"), "RusTrace.zip"),
            None
        );
        assert_eq!(parse_sha256_file("abc  RusTrace.zip", "RusTrace.zip"), None);
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    /// A ZIP archive (central directory included) with the given entries; `deflate` compresses.
    fn make_zip(entries: &[(&str, &[u8])], deflate: bool) -> Vec<u8> {
        use std::io::Write;
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data) in entries {
            let packed = if deflate {
                let mut e =
                    flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
                e.write_all(data).unwrap();
                e.finish().unwrap()
            } else {
                data.to_vec()
            };
            let method: u16 = if deflate { 8 } else { 0 };
            let crc = crc32fast::hash(data);
            let offset = out.len() as u32;
            out.extend_from_slice(&[0x50, 0x4B, 0x03, 0x04, 20, 0, 0, 0]);
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&[0; 4]);
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(packed.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&packed);

            central.extend_from_slice(&[0x50, 0x4B, 0x01, 0x02, 20, 0, 20, 0, 0, 0]);
            central.extend_from_slice(&method.to_le_bytes());
            central.extend_from_slice(&[0; 4]);
            central.extend_from_slice(&crc.to_le_bytes());
            central.extend_from_slice(&(packed.len() as u32).to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&[0; 12]);
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let cd_offset = out.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&[0x50, 0x4B, 0x05, 0x06, 0, 0, 0, 0]);
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    #[test]
    fn unzip_stored_and_deflated() {
        let text = b"key = value\n".repeat(50);
        for deflate in [false, true] {
            let zip = make_zip(
                &[("RusTrace/", b""), ("RusTrace/lang/en.lang", &text)],
                deflate,
            );
            let files = unzip(&zip).unwrap();
            assert_eq!(
                files,
                vec![("RusTrace/lang/en.lang".to_owned(), text.clone())]
            );
        }
        let mut broken = make_zip(&[("a.txt", b"hello")], false);
        let i = broken.windows(5).position(|w| w == b"hello").unwrap();
        broken[i] = b'j';
        assert!(unzip(&broken).unwrap_err().contains("checksum"));
        assert!(unzip(&make_zip(&[("../evil.exe", b"x")], false)).is_err());
        assert!(unzip(b"not a zip").is_err());
    }

    #[test]
    fn install_replaces_program_and_files() {
        let dir =
            std::env::temp_dir().join(format!("rustrace-install-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("logdef")).unwrap();
        let exe = dir.join("rustrace.exe");
        std::fs::write(&exe, b"old exe").unwrap();
        std::fs::write(dir.join("logdef").join("mine.def"), b"own").unwrap();

        let files = vec![
            ("RusTrace/rustrace.exe".to_owned(), b"new exe".to_vec()),
            ("RusTrace/lang/en.lang".to_owned(), b"en".to_vec()),
            ("RusTrace/LICENSE".to_owned(), b"lic".to_vec()),
        ];
        let warnings = install(files, &exe).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(std::fs::read(&exe).unwrap(), b"new exe");
        assert_eq!(std::fs::read(old_exe_path(&exe)).unwrap(), b"old exe");
        assert_eq!(
            std::fs::read(dir.join("lang").join("en.lang")).unwrap(),
            b"en"
        );
        assert_eq!(std::fs::read(dir.join("LICENSE")).unwrap(), b"lic");
        assert_eq!(
            std::fs::read(dir.join("logdef").join("mine.def")).unwrap(),
            b"own"
        );

        // Without the program in the archive nothing is changed.
        let err = install(vec![("RusTrace/LICENSE".to_owned(), b"x".to_vec())], &exe).unwrap_err();
        assert!(err.contains("rustrace.exe"));
        assert_eq!(std::fs::read(dir.join("LICENSE")).unwrap(), b"lic");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn session_state_round_trip() {
        let state = SessionState {
            path: Some(r"C:\Logs\a.log".into()),
            format: Some(FormatState::Def {
                id: "cbs.log.def".into(),
                priority: 1,
            }),
            selected: Some(42),
            search_text: "error".into(),
            show_sev: [true, false, true],
            col_filters: vec![
                (ColumnKind::Role(Role::Component), "ccm".into()),
                (ColumnKind::Extra(2), "!x".into()),
            ],
            window: Some(WindowState {
                pos: Some([10.0, 20.0]),
                size: Some([800.0, 600.0]),
                maximized: false,
            }),
            ..Default::default()
        };
        let file = state.save_temp().unwrap();
        assert_eq!(SessionState::take(&file), Some(state));
        assert!(!file.exists());
    }

    /// Real download of the latest release (network): `cargo test fetch_latest_release -- --ignored`.
    #[test]
    #[ignore]
    #[cfg(windows)]
    fn fetch_latest_release() {
        let u = fetch_updates().unwrap();
        for m in &u.missed {
            println!("missed: {} {}", m.version, m.date);
        }
        let r = u.latest;
        println!("latest: {} {} {}", r.version, r.date, r.page);
        let tmp = std::env::temp_dir().join(format!("rustrace-dl-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let exe = tmp.join("rustrace.exe");
        std::fs::write(&exe, b"old").unwrap();
        let warnings = download_and_install(&r, &exe, &Progress::default()).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(std::fs::metadata(&exe).unwrap().len() > 1_000_000);
        assert!(tmp.join("lang").join("sk.lang").exists());
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
