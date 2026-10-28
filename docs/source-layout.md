# Source layout

What every source file and folder of the repository contains.

[← Back to README](../README.md)

- `src/parser.rs` – incremental chunked file reading with progress, encoding detection, format parsing (+ tests)
- `src/colfilter.rs` – column filter syntax, the date/time column filter compared as time ranges
  (+ tests)
- `src/filter.rs` – which entries are displayed / match the search: the filter run (sequential
  and parallel in chunks), match bits with a running count (+ tests)
- `src/merge.rs` – several logs in one view: reading them as a set, merging by time (also
  appended older rows), file ids and united columns (+ tests)
- `src/store.rs` – compact entry storage: 40-byte records, text arena in memory or in a
  memory-mapped temporary file, string interner, date/time as milliseconds (+ tests)
- `src/logdef.rs` – log definitions: `.def` file format, loading, choosing by file name / content,
  parsing lines into columns (+ tests)
- `src/logset.rs` – log sets: `.set` file format, loading, variables, file checks (+ tests)
- `src/errcodes.rs` – error code detection and lookup (+ tests)
- `src/httpstatus.rs` – HTTP status / IIS substatus descriptions, lookup input parsing (+ tests)
- `src/guides.rs` – the guides of Help → Guides: table of guide files and titles, GitHub URLs (+ tests)
- `src/fileassoc.rs` – Windows file association for `.log` / `.lo_` (registry, status, + tests)
- `src/tabexport/` – table export: rows of a scope, column model, cell values, CSV writer,
  minimal XLSX writer and the streaming ZIP writer it uses (+ tests)
- `src/export.rs` – export around highlighted rows: range merging, date/time filter and input
  parsing, last-N blocks, output formats (+ tests)
- `src/i18n.rs` – localization: language files, fallback, `t!()` macro (+ tests)
- `src/cli.rs` – command-line arguments: parsing, checks, help text, message box (+ tests)
- `src/config.rs` – settings and rules (serde/JSON)
- `src/overview.rs` – marks of the overview strip in buckets, updated from the first changed
  row on (+ tests)
- `src/timefmt.rs` – durations for display: selection span, Δ column (+ tests)
- `src/app/` – user interface (`App`), one `impl App` block per area:
  - `mod.rs` – the `App` state, start-up, theme / zoom / language, the frame loop (`eframe::App`)
  - `loading.rs` – opening files, background reading, auto refresh, rotation, memory limits
  - `filter.rs` – search, filter mode, severity toggles, time range, column filters, the
    background re-filter (+ tests)
  - `cli.rs` – view options from the command line, the row selected after the first read
  - `logdefs.rs` – log definitions, update notice, the log format menu
  - `input.rs` – keyboard shortcuts, drag & drop
  - `menu.rs`, `toolbar.rs`, `statusbar.rs` (+ tests) – menu bar, toolbar and time range row, status bar
  - `table.rs` – the log table, row colors, highlighted text, context menu
  - `details.rs` – details pane (message / raw line), error code and status code explanation
  - `strip.rs` – the overview strip next to the scroll bar
  - `delta.rs` – the Δ time column (modes, reference row, long gaps)
  - `selection.rs` – selection of several rows, copying them (messages, table, original lines)
  - `bookmarks.rs` – bookmarks: toggling, F2 navigation, kept on their lines through refreshes (+ tests)
  - `widgets.rs` – small shared UI helpers (color button, theme switch, …)
  - `dialogs/` – highlight rules, colors, error code lookup, export, bookmarks, help / about,
    default app (Windows), new versions, open log set
- `src/main.rs` – window start-up, command line (`src/cli.rs`), renderer choice (OpenGL, wgpu / Direct3D 12 as the fallback)
- `src/update.rs` – new versions: GitHub release, version comparison, ZIP extraction, SHA-256 check,
  installation over the running program, state for the restart (+ tests)
- `src/http.rs` – HTTPS download via WinHTTP (system proxy and certificates)
- `src/splash.rs` – start-up window with a rotating ring of dots (Windows, Win32 / GDI)
- `src/icon.rs` – the program icon drawn in code (window, start-up window, `.ico` for `build.rs`)
- `build.rs` – embeds the icon and the version information into `rustrace.exe` (Windows)
- `lang/*.lang` – built-in translations
- `logdef/*.def` – built-in log definitions (IIS, Exchange, DISM, CBS, Windows Installer)
- `logsets/*.set` – built-in log sets (ConfigMgr / Intune scenarios)
- `scripts/package.ps1` – packs the release build into the self-extracting archive and the ZIP
- `scripts/release.ps1` – the whole GitHub release (checks, version, tests, commit, tag, build, publish)
- `scripts/gen_testlogs.py` – generates the test logs in `testlogs/` (not in git)
- `scripts/gen_guide_logs.py` – generates the sample logs of the guides in `docs/guides` (one script per guide in `scripts/guide_logs/`)
- `.github/workflows/ci.yml` – formatting, clippy, tests and a build for every push and pull request
- `.github/workflows/release.yml` – builds and publishes releases on GitHub

- `LICENSE` – the Unlicense (public domain)
- `CLAUDE.md` – project notes for Claude (conventions, commands, feature index)
- `docs/architecture.md`, `docs/history.md` – internals of every feature, history and open ideas
  (notes for Claude, linked from `CLAUDE.md`)
