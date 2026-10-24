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
- **Log definitions** (`logdef\*.def`, `src/logdef.rs`): regex with named groups or W3C (`#Fields:`)
  describe other layouts; built-in `iisweb.log.def`, `dism.log.def`, `cbs.log.def`, external files
  from `logdef\` next to the exe or `%APPDATA%\RusTrace\logdef` (same name overrides built-in).
  Roles fill the standard columns (message/component/datetime/thread), `role.severity` + regexes give
  the severity, other groups are extra columns (`LogEntry::extra`, `Schema::extra_names`). The table
  columns come from `Schema` (`LogSource::schema()`); CMTrace = `Schema::default()`. Format choice:
  Groups: predefined = built-in, replaced by a same-named file in `logdef\` next to the exe
  (`PRIO_BUILTIN` 0 / `PRIO_PROGRAM` 1); own = user folder (`PRIO_USER` 2) – own files never
  replace predefined ones, both are offered (`LogDef::same_as` = id + priority identifies a
  definition, e.g. for the manual choice after reloading). `cfg.logdef_sources` = all /
  predefined / own (`SourceFilter`). `DefSet::defs` sorted by priority desc, then id;
  `logdef::matching_by_name`; the UI shows the active definition (`LogSource::active_def`, name –
  file in the status bar, ⚠ + list in the menu when several match).
  Update notice: `cfg.known_logdefs` (file → text of the predefined definitions seen; filled
  silently on the first start) vs `DefSet::predefined` → `logdef::definition_updates` (new /
  changed key by key with the own copy's value / removed) → window at start-up; "Got it" stores
  the current texts.
  View → Log format (Auto = file name → content `detect` → built-in; CMTrace content never uses
  `detect`), reset to Auto for every newly opened file. `unmatched = append` keeps the last entry
  provisional until the next one starts (`ParserState::pending_entry`).
- Encodings: UTF-8 (BOM or not), UTF-16 LE/BE (BOM), ANSI Windows-1250 fallback.
- Incremental reading in a **background thread** in 8 MB chunks with progress + cancel; truncation /
  rotation detection; provisional last line. 400 MB / 1.8 M lines ≈ 3.7 s in release.
- **Rotated logs merged** (`cfg.merge_rotated`, default on; `parser::rotated_parts`): opening
  `service.log` also reads `service.lo_` and `service-YYYYMMDD-HHMMSS.log` first (sorted by mtime,
  then name; only for the active file name, not when a part itself is opened). `LogSource::files`
  (`SourceFile { name, path: Option }`) = file id of every entry (`Rec::file: u16`, fits the
  32-byte record); line numbers are per file. Rotation/replacement detection: the first 4 KB of the
  active file (`head`) must still match and the file must not shrink; on a mismatch with merging the
  older part starting with `head` (and ≥ offset) is the renamed active file → its unread rest is
  read, the file id is retagged to the new name, files with that path before are marked overwritten
  (`path = None`), the new active file is read from 0 – an `Appended` result, nothing is lost.
  Otherwise (or without merging) a full reset. UI: `App::columns()` adds `ColumnKind::File` first
  when `files.len() > 1`; status bar 📚 with the list. Export: `export::segments` splits blocks by
  file; original lines are read per file, overwritten parts fall back to the message texts.
- **Memory limits for long-watched logs** (`App::enforce_memory_limits`, after every poll):
  text in memory ≥ temp file threshold → `LogStore::move_text_to_temp_file`; text >
  `cfg.max_log_mb` (default 1000, `cfg.max_log_enabled`) → `LogStore::drop_front(k)` down to ¾ of
  the limit (never provisional rows), interner rebuilt; App shifts `filtered`, `is_match`,
  `selected`, `committed_len` by k, bumps `data_gen`, counts `dropped` (✂ in the status bar, reset
  by a full read). Known limit: the first read of a huge file still loads it whole before trimming.
- **Compact storage** (`src/store.rs`, `LogStore`): the parser still produces `LogEntry` (Strings)
  per line, but they go straight into a `LogStore` – 32-byte `Rec` (message start in the text arena,
  `i64` ms timestamp or tagged interned raw text, line, interned component/thread, severity), one
  text arena in entry order (message `i` ends where `i + 1` starts), interner (id 0 = ""), extra
  columns column-major as interned ids. `PollResult` carries a `LogStore`; appends are merged with
  `LogStore::append` (re-interning). The parser reserves capacity after the first chunk (estimate).
  Text arena = `String` or a **memory-mapped temp file** (`memmap2`, `FILE_ATTRIBUTE_TEMPORARY` +
  `FILE_FLAG_DELETE_ON_CLOSE`, share mode 0, grows by doubling from 64 MB, falls back to memory on
  error) for reads ≥ `cfg.temp_file_threshold_mb` (default 100, `cfg.temp_file_enabled`). Never map
  the original log (blocks the writer from truncating / rotating it on Windows).
- **DX12 on Windows** (`main.rs::prefer_dx12`): wgpu instance limited to DX12 when a non-CPU DX12
  adapter exists (probe before start), unless `WGPU_BACKEND` is set. Default wgpu loads Vulkan +
  DX12 + GL drivers: empty window 516 MB private vs 325 MB with DX12 only (AMD RX 9060 XT).
- Measured (400 MB CMTrace, GUI private bytes): v0.2.0 1231 MB → 421 MB (temp file) / 582 MB (memory);
  benchmark without GUI: 695 → 76 / 239 MB, load same, search 157 → 138 ms.
- Auto refresh: interval in ms / s / min, pause/resume (Ctrl+P), presets, "follow end".
- Highlight rules: text or regex, case, scope whole row / text only, also-component, order = priority,
  JSON import/export, **separate colors for light and dark theme**; severity + search colors per theme.
- Themes: system / light / dark (View menu + quick toggle button in the toolbar).
- Zoom: Ctrl + mouse wheel over the log text (table, details message) → `cfg.font_size` (6–48);
  elsewhere → whole-UI `zoom_factor` (0.5–3.0, saved as `cfg.ui_zoom`, also Ctrl+plus/minus/0).
  Hover is tracked per frame in `hover_log`, applied in `handle_zoom` after the panels are drawn.
- Search: text/regex, next/prev, filter mode, info/warning/error toggles.
- Horizontal scrolling of the table (`App::table`): egui_extras tables only scroll vertically, so
  the table is laid out in a child Ui shifted left by `table_scroll_x` and clipped to the visible
  rect – its right edge with the vertical scroll bar stays in view; a separate horizontal
  `ScrollArea` (solid style) below it owns the offset, Shift + wheel changes it too. The natural
  width `table_width` is measured every frame from the header cells (last column min
  `LAST_COLUMN_MIN` = 150, it fills the rest when everything fits).
- Column filters (`src/colfilter.rs`, toolbar toggle `cfg.show_column_filters`): filter row in the
  table header built from `Schema::columns` (adapts to the log type); `App::col_filter_text`
  (by `ColumnKind`) → `col_filters` compiled; checked in `refilter_from` via
  `passes_column_filters` (date/time as displayed). Syntax: contains / `=exact` / `a|b` / leading
  `!` negation, case-insensitive. Cleared when the columns change (other log type).
- Error code lookup (Ctrl+E) + automatic explanation in the details pane; descriptions from language
  files (`code.0xXXXXXXXX`) + Windows `FormatMessageW` (also wininet/winhttp).
- Export highlighted rows with context (Ctrl+Shift+S): lines before/after, criteria (rules, errors,
  warnings, search), overlapping ranges merged (no duplicates), formats original lines / readable
  table, separators, **only records since date/time** (with presets relative to newest log record),
  **only the last N blocks**, **rows visible in the table = highlighted rows**
  (`export_visible_only`: hits = `filtered`, the rules/errors/warnings/search criteria are disabled;
  context always from the whole file – the owner's expectation, an earlier variant that also took
  context only from visible rows was rejected). Order: hits → time filter → merge → clip older
  context → last N.
- Localization: built-in en / sk / cs / de; external `<code>.lang` files load **without recompiling**
  from `lang\` next to the exe or `%APPDATA%\RusTrace\lang`; fallback to English.
- Settings saved automatically to `%APPDATA%\RusTrace\config.json` (with migration of old settings).

## Code layout
- `src/main.rs` – window start-up, optional log path argument.
- `src/app.rs` – the whole UI (`App`), background loading, export dialog, windows.
- `src/parser.rs` – `LogSource::poll()` chunked incremental reading, encodings, format parsing,
  `read_line_ranges()` for original-line export. Dates stored as ISO `YYYY-MM-DD HH:MM:SS.mmm`,
  formatted for display by the UI (`format.date` in the language file).
- `src/store.rs` – `LogStore` / `EntryRef` (compact entries, text arena memory / mapped temp file,
  interner, `iso_to_ms` / `ms_to_iso`).
- `src/logdef.rs` – log definitions: `.def` parsing, loading/overrides, choice by name/content,
  `DefState::parse()` line → `LogEntry`, `Schema` / `Column` / `Role`.
- `src/export.rs` – pure export logic (range merging, time filter, date input parsing, last-N, output).
- `src/errcodes.rs` – error code detection/parsing/lookup.
- `src/i18n.rs` – language files, `t!()` macro, fallback, external language folders.
- `src/config.rs` – `Config`, `HighlightRule`, per-theme palettes, serde defaults/migration.
- `lang/*.lang` – built-in translations, embedded with `include_str!`.
- `logdef/*.def` – built-in log definitions, embedded with `include_str!` and shipped in `logdef\`.

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
- `cargo test` (38 tests: store drop_front / move to temp file, rotated parts (naming, merge on open, rotation while watching, replaced
  file), export segments by file, time range bounds, column filter syntax, parsers, chunked/ANSI/UTF-16 reading, log definitions (CBS, DISM, IIS,
  choice by name/content, appending across refreshes), store (32-byte record, ISO ↔ ms, push /
  truncate / append, mapped temp file growing past 64 MB, interner), export incl. end-to-end and
  visible rows as highlighted rows, error codes, i18n completeness, config migration)
- **Test logs**: `python scripts/gen_testlogs.py [--small-only]` creates `testlogs/` (in `.gitignore`,
  ~650 MB with the big ones, ~1 min) – CMTrace (UTF-8, UTF-8 BOM, UTF-16 LE, multi-line, unfinished
  last line), SMS format, plain ANSI 1250 / without timestamps, CBS, DISM, IIS (two `#Fields` sets,
  detection by content), a rotation set with set modification times, big CMTrace 400 MB / CBS
  100 MB / IIS 100 MB; list in `testlogs/README.txt`. Use these for manual / GUI checks.
  `cargo test --release testlogs_overview -- --ignored --nocapture` loads all of them and prints
  format, encoding, files, entries, errors/warnings, dated entries and time.
- Benchmark (ignored test, Windows): `RUSTRACE_BENCH_FILE=<log> cargo test --release
  bench_large_file -- --ignored --nocapture` (`RUSTRACE_BENCH_MAPPED=1` = temp file); prints load /
  search / random access times and private bytes. A realistic 400 MB CMTrace log can be generated
  with a small script (random components, threads, messages, ~1 % multi-line).
- `cargo clippy`
- `scripts\package.ps1` → `dist\RusTrace-v<version>-windows-x64.exe` (7-Zip SFX) and `.zip` (same
  content, for extracting manually), each with a `.sha256`; both extract to
  `RusTrace\` with `rustrace.exe` + `lang\` + `logdef\` + `LICENSE`. Needs 7-Zip.
- Releases: push tag `v<version>` (matching `Cargo.toml`) → GitHub Actions `release.yml` tests,
  builds, packages and creates a **draft** release; then publish it. Manual workflow run = artifact only.
- Rust (rustup, stable MSVC) is installed on the owner's PC since 2026-10-24 (VS 2019 C++ tools);
  `cargo` is in `%USERPROFILE%\.cargo\bin` – add it to PATH in shells started before that.
- Do not leave test runs touching `%APPDATA%\RusTrace\config.json`: back it up and restore it
  (or delete it if it did not exist before).
- CRT is linked statically via `.cargo/config.toml` (no VC++ redistributable needed).
- Line endings: `.gitattributes` forces LF for all text files (`* text=auto eol=lf`); write new
  files with LF. The release `.sha256` is written with LF so `sha256sum -c` works.

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
12. Export option "rows visible in the table" as the highlighted rows (context from the whole file).
13. Log definitions (owner's idea): `logdef\*.def` files, built-in IIS (W3C) / DISM / CBS,
    View → Log format, dynamic table columns, extra columns in search/rules/details/readable export.
14. Memory saving (owner's idea): measured first, then compact `LogStore`, text in a memory-mapped
    temp file for big logs (View menu setting, 💾 in the status bar), DX12 preferred on Windows.
15. Column filters: filter row under the headers, adapts to the columns of the open log type.
16. Time range filter (from – to) in the main view: `cfg.show_time_range`, `App::time_range` (ms),
    `export::parse_range_bound` (inclusive end fills up the missing day/hour/minute/second).
17. Merging of rotated logs (owner: only concatenation for now, also the dated history files,
    File column only for logs of several files) incl. rotation while watching.
18. Memory protection for long-watched logs: automatic move to the temp file + size limit that
    removes the oldest records (owner agreed to dropping old records from memory and view).
19. Release v0.3.0 (log definitions, memory saving, column filters, time range, rotated logs,
    memory limit); the release now also contains a plain ZIP next to the self-extracting .exe.
23. Horizontal scroll bar under the table for columns that do not fit (Shift + wheel too).
24. Log definitions: clear priority (user folder > next to exe > built-in) instead of file-name
    order, active definition with its file shown, warning when several match (owner's report).
25. Own definitions no longer replace predefined ones with the same file name (both offered),
    setting which groups are used, notice of new / changed predefined definitions after updates.
26. Release v0.4.0 (IIS columns fix, horizontal scroll bar, log definition priority / groups /
    update notice).
20. Test logs generator (`scripts/gen_testlogs.py`, `testlogs/` not in git) + `testlogs_overview`;
    it found that a `.lo_` opened directly was merged with itself – fixed (rotated parts opened
    directly are never merged).
21. Release v0.3.1 (bug fix: a rotated part opened directly was read twice).
22. Fix: a later w3c `#Fields:` header replaced the table columns, so rows of the first part lost
    their columns; now the first header replaces the default fields, later ones only add columns.

## Ideas not done yet (offered to the owner)
- Merge **different** logs into one view sorted by time (like CMTrace File → Open → "Merge
  selected files", e.g. AppEnforce.log + AppDiscovery.log). The owner wants it noted as an idea;
  tabs with several logs are not wanted. The rotated-parts merge (concatenation) already exists.

## Ideas not done yet (offered by the owner)
- (none open – the memory saving and the log definitions are done, see History 13 and 14)

## Possible further memory work (not done)
- The "original lines" export reads the whole log file into memory (`read_line_ranges`), a
  temporary peak of ~2× the file size while exporting; could stream the file instead.
- `filtered: Vec<usize>` could be `Vec<u32>` (8 → 4 bytes per visible row).
