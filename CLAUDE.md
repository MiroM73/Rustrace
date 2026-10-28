# RusTrace – project memory for Claude

## Working with the owner
- The owner (Miroslav, GitHub `MiroM73`) talks to Claude in **Slovak, informal "ty" form**. Answer in Slovak.
- Everything inside the repository is in **English**: code, comments, README, commit messages.
- After every finished change: run the tests, verify the behaviour, update `README.md`, and make a
  commit with a **detailed English commit message** (what changed, why, how it was verified).
- The **first block of the README** states that everything (code, logic, architecture, UI design,
  translations, tests, scripts, docs) was created by Claude (Claude Opus 5.5, `claude-opus-5-5`,
  Anthropic) and the owner only specified requirements. Keep it at the top when editing the README.
- License: **the Unlicense** (public domain, `LICENSE`, `license = "Unlicense"` in Cargo.toml).
  `LICENSE` is also shipped in the release archive.
- This file (`CLAUDE.md`) and `docs/` are committed in the repository.
- Do not push or rewrite published history without asking.

## What the application is
A native desktop log viewer in Rust (eframe/egui 0.37) modelled on **CMTrace** from Microsoft
SCCM / ConfigMgr. Single self-contained executable; main target is Windows. User-facing details
are in `README.md`.

- **`docs/architecture.md`** – how every feature works inside. Read the section of a feature
  before changing it, and update it with the change.
- **`docs/history.md`** – what was done when (add a line per issue / release), open ideas of the
  owner, possible further memory work.

Feature index (file → section of `docs/architecture.md`):
- Formats, encodings, chunked background reading – `src/parser.rs` → Formats, Reading
- Log definitions (`.def`, W3C, groups, update notice) – `src/logdef.rs`, `app/logdefs.rs` → Log definitions
- Several logs merged by time – `src/merge.rs` → Merged logs
- Log sets (File → Open log set, `.set` files, variables, remote computer) – `src/logset.rs`, `dialogs/logsets.rs` → Log sets
- Rotated parts, rotation while watching – `src/parser.rs` → Rotated logs
- Compact store, temp file, memory limit – `src/store.rs`, `app/loading.rs` → Compact storage, Memory limits
- Details pane message / raw line – `app/details.rs` → Raw line
- Renderer (glow, wgpu fallback), repaint rules – `src/main.rs` → Renderer and refresh frames
- Search, filters, background re-filter, date column filter – `src/filter.rs`, `src/colfilter.rs`, `app/filter.rs` → Filtering
- Table, horizontal scrolling, column filters – `app/table.rs` → Table
- Overview strip – `src/overview.rs`, `app/strip.rs` → Overview strip
- Δ time column – `app/delta.rs` → Delta column
- Several selected rows, copying – `app/selection.rs` → Selection
- Bookmarks – `app/bookmarks.rs`, `dialogs/bookmarks.rs` → Bookmarks
- Follow end paused while a row is selected (#37) – `app/follow.rs` → UI basics
- Error codes, HTTP / IIS status codes (#28) – `src/errcodes.rs`, `src/httpstatus.rs`, `app/details.rs`,
  `dialogs/lookup.rs` → Table (error code lookup, HTTP / IIS status codes), Log definitions
- Export with context – `src/export.rs`, `dialogs/export.rs` → Export
- Updates – `src/update.rs`, `src/http.rs`, `dialogs/update.rs` → Updates
- Languages, settings – `src/i18n.rs`, `src/config.rs` → Localization and settings
- Command-line arguments – `src/cli.rs`, `app/cli.rs`, `src/main.rs` → Command line

## Code layout
- `src/main.rs` – window start-up, command line (parsed by `src/cli.rs`), renderer choice,
  start-up splash.
- `src/splash.rs`, `src/icon.rs`, `build.rs`, `src/fileassoc.rs` – Windows-specific parts
  (details: `docs/architecture.md` → Windows modules).
- `src/app/` – the whole UI (`App`), split by area (#19): `mod.rs` holds the `App` struct, its
  helper types/consts, `App::new` and the frame loop; every other file is an `impl App` block for
  one area. Conventions: each submodule starts with `use crate::app::*;` (gets the parent's
  imports, private types and consts – child modules may use private items of their ancestors);
  methods used outside their own file are `pub(in crate::app) fn`; shared free helpers are
  `pub(in crate::app)` and glob-imported in `mod.rs` (`use table::*;` …). A new window →
  `dialogs/<name>.rs` + `mod <name>;` in `dialogs/mod.rs`; keep files under ~800 lines.
- `src/parser.rs` – `LogSource::poll()` chunked incremental reading, encodings, format parsing,
  raw lines. Dates stored as ISO `YYYY-MM-DD HH:MM:SS.mmm`, formatted for display by the UI
  (`format.date` in the language file).
- `src/store.rs` – `LogStore` / `EntryRef` (compact entries, text arena, interner, ISO ↔ ms).
- `src/merge.rs` – `LogSet`, `interleave`, `apply_appended`.
- `src/logdef.rs` – log definitions, `Schema` / `Column` / `ColumnKind` / `Role`.
- `src/logset.rs` – log sets: `.set` parsing / loading, path variables (local / remote), share and file checks.
- `src/filter.rs` – `FilterSpec` / `CellFilter` / `MatchBits`; `src/colfilter.rs` – column filter syntax.
- `src/export.rs` – pure export logic; `src/overview.rs` – overview strip buckets;
  `src/timefmt.rs` – durations; `src/excerpt.rs` – message cell text; `src/errcodes.rs` – error codes;
  `src/httpstatus.rs` – HTTP / IIS status codes.
- `src/update.rs` / `src/http.rs` – updates; `src/i18n.rs` – `t!()`, language files;
  `src/config.rs` – `Config`, rules, per-theme palettes, serde defaults/migration.
- `lang/*.lang` – built-in translations; `logdef/*.def` – built-in log definitions; `logsets/*.set`
  – built-in log sets (all embedded with `include_str!` and shipped next to the exe).

## Conventions
- **Every user-visible string goes through `t!("key")`** (placeholders `t!("key", name = value)`).
  A new key must be added to **all four** `lang/*.lang` files with the same placeholders –
  the test `i18n::tests::builtin_languages_are_complete` enforces this.
- Keep heavy work (file reading, export writing) off the UI thread (see `start_poll`, `start_export`).
- New settings: add to `Config` with a default (`#[serde(default)]` keeps old config files loading).
- Prefer pure, unit-tested functions in modules (`export.rs`, `parser.rs`), thin UI code in `src/app/`.
- egui 0.37 API notes: `eframe::App::ui(&mut self, ui, frame)`, `egui::Panel::top/bottom(..).show(ui, ..)`,
  `egui::MenuBar::new().ui(..)`, `ui.close()`, `egui::Id::unique(..)` (not `Id::new`).

## Commands
- `cargo build --release` → `target\release\rustrace.exe`; `cargo test` (107 tests; what they
  cover: `docs/architecture.md` → Tests and benchmarks).
- `cargo fmt` + `cargo clippy --all-targets --locked -- -D warnings` – the code is rustfmt-clean and
  clippy-clean; CI (`.github/workflows/ci.yml`) fails otherwise. Formatting-only commits go into
  `.git-blame-ignore-revs`.
- Do not leave test runs touching `%APPDATA%\RusTrace\config.json`: back it up and restore it
  (or delete it if it did not exist before).
- `cargo` is in `%USERPROFILE%\.cargo\bin` – add it to PATH in shells started before 2026-10-24.
- **Test logs**: `python scripts/gen_testlogs.py [--small-only]` creates `testlogs/` (not in git,
  list in `testlogs/README.txt`, incl. 400 MB CMTrace / 100 MB CBS / IIS). Use these for manual /
  GUI checks; benchmarks: `docs/architecture.md` → Tests and benchmarks.
- Releases: `scripts\release.ps1 <version> [-Publish] [-DryRun] [-NoWatch] [-Yes]`; the owner runs
  it himself to save tokens. Details: `docs/architecture.md` → Packaging and releases.
- Line endings: `.gitattributes` forces LF for all text files (`* text=auto eol=lf`); write new
  files with LF.
