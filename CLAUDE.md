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
- This file (`CLAUDE.md`) is committed in the repository.
- Do not push or rewrite published history without asking.

## What the application is
A native desktop log viewer in Rust (eframe/egui 0.37) modelled on **CMTrace** from Microsoft
SCCM / ConfigMgr. Single self-contained executable; main target is Windows.

Features (all implemented, see README for user-facing details):
- Formats: CMTrace `<![LOG[..]LOG]!><time="" date="" component="" type="" thread="">` incl. multi-line,
  old SMS format `msg $$<comp><date time><thread=..>`, plain text. Severity from `type` (1/2/3) or
  optional keywords error/fail/warn.
- Encodings: UTF-8 (BOM or not), UTF-16 LE/BE (BOM), ANSI Windows-1250 fallback.
- Incremental reading in a **background thread** in 8 MB chunks with progress + cancel; truncation /
  rotation detection; provisional last line. 400 MB / 2.2 M lines ≈ 3 s in release.
- Auto refresh: interval in ms / s / min, pause/resume (Ctrl+P), presets, "follow end".
- Highlight rules: text or regex, case, scope whole row / text only, also-component, order = priority,
  JSON import/export, **separate colors for light and dark theme**; severity + search colors per theme.
- Themes: system / light / dark (View menu + quick toggle button in the toolbar).
- Zoom: Ctrl + mouse wheel over the log text (table, details message) → `cfg.font_size` (6–48);
  elsewhere → whole-UI `zoom_factor` (0.5–3.0, saved as `cfg.ui_zoom`, also Ctrl+plus/minus/0).
  Hover is tracked per frame in `hover_log`, applied in `handle_zoom` after the panels are drawn.
- Search: text/regex, next/prev, filter mode, info/warning/error toggles.
- Error code lookup (Ctrl+E) + automatic explanation in the details pane; descriptions from language
  files (`code.0xXXXXXXXX`) + Windows `FormatMessageW` (also wininet/winhttp).
- Export highlighted rows with context (Ctrl+Shift+S): lines before/after, criteria (rules, errors,
  warnings, search), overlapping ranges merged (no duplicates), formats original lines / readable
  table, separators, **only records since date/time** (with presets relative to newest log record),
  **only the last N blocks**. Order: hits → time filter → merge → clip older context → last N.
- Localization: built-in en / sk / cs / de; external `<code>.lang` files load **without recompiling**
  from `lang\` next to the exe or `%APPDATA%\RusTrace\lang`; fallback to English.
- Settings saved automatically to `%APPDATA%\RusTrace\config.json` (with migration of old settings).

## Code layout
- `src/main.rs` – window start-up, optional log path argument.
- `src/app.rs` – the whole UI (`App`), background loading, export dialog, windows.
- `src/parser.rs` – `LogSource::poll()` chunked incremental reading, encodings, format parsing,
  `read_line_ranges()` for original-line export. Dates stored as ISO `YYYY-MM-DD HH:MM:SS.mmm`,
  formatted for display by the UI (`format.date` in the language file).
- `src/export.rs` – pure export logic (range merging, time filter, date input parsing, last-N, output).
- `src/errcodes.rs` – error code detection/parsing/lookup.
- `src/i18n.rs` – language files, `t!()` macro, fallback, external language folders.
- `src/config.rs` – `Config`, `HighlightRule`, per-theme palettes, serde defaults/migration.
- `lang/*.lang` – built-in translations, embedded with `include_str!`.

## Conventions
- **Every user-visible string goes through `t!("key")`** (placeholders `t!("key", name = value)`).
  A new key must be added to **all four** `lang/*.lang` files with the same placeholders –
  the test `i18n::tests::builtin_languages_are_complete` enforces this.
- Keep heavy work (file reading, export writing) off the UI thread (see `start_poll`, `start_export`).
- New settings: add to `Config` with a default (`#[serde(default)]` keeps old config files loading).
- Prefer pure, unit-tested functions in modules (`export.rs`, `parser.rs`), thin UI code in `app.rs`.
- egui 0.37 API notes: `eframe::App::ui(&mut self, ui, frame)`, `egui::Panel::top/bottom(..).show(ui, ..)`,
  `egui::MenuBar::new().ui(..)`, `ui.close()`, `egui::Id::unique(..)` (not `Id::new`).

## Commands
- `cargo build --release` → `target\release\rustrace.exe`
- `cargo test` (18 tests: parsers, chunked/ANSI/UTF-16 reading, export incl. end-to-end, error codes,
  i18n completeness, config migration)
- `cargo clippy`
- `scripts\package.ps1` → `dist\RusTrace-v<version>-windows-x64.exe` (7-Zip SFX, extracts to
  `RusTrace\` with `rustrace.exe` + `lang\` + `LICENSE`) and `.sha256`. Needs 7-Zip.
- Releases: push tag `v<version>` (matching `Cargo.toml`) → GitHub Actions `release.yml` tests,
  builds, packages and creates a **draft** release; then publish it. Manual workflow run = artifact only.
- Rust (rustup, stable MSVC) is installed on the owner's PC since 2026-10-24 (VS 2019 C++ tools);
  `cargo` is in `%USERPROFILE%\.cargo\bin` – add it to PATH in shells started before that.
- Do not leave test runs touching `%APPDATA%\RusTrace\config.json`: back it up and restore it
  (or delete it if it did not exist before).
- CRT is linked statically via `.cargo/config.toml` (no VC++ redistributable needed).

## History (2026-10-23)
1. Initial viewer: parser, highlight rules, regex search, auto refresh (ms/s/min, pause).
2. Background chunked loading with progress; error code lookup.
3. All code translated to English; localization system (en/sk/cs/de, external .lang files).
4. Theme quick toggle + separate light/dark colors for rules and severity colors.
5. Export of highlighted rows with context (merging, two formats).
6. Export: since date/time filter + last N blocks.
7. Owner rewrote the history so that all commits are authored by him (Claude stays as
   `Co-Authored-By`); old hashes are gone.
8. Release packaging (7-Zip SFX), GitHub Actions release workflow, static CRT; first release v0.1.0
   (https://github.com/MiroM73/Rustrace/releases/tag/v0.1.0, published 2026-10-23).
9. Unlicense added, AI-authorship notice moved to the top of the README, CLAUDE.md committed.
10. (2026-10-24) Ctrl + mouse wheel zoom: log text vs. whole UI depending on the pointer position.
11. Release v0.2.0 (zoom; LICENSE now shipped in the archive).

## Ideas not done yet (offered to the owner)
- Several logs at once in tabs / merged by time (like CMTrace "Merge").
- Filter by component or time range in the main view.
- Export option "only currently visible (filtered) rows".
