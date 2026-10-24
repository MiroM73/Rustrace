# RusTrace – a CMTrace-style log viewer

> **This program was created entirely by AI.** Everything in this repository – the source code,
> program logic, architecture, user interface design, translations, tests, build and release
> scripts and the documentation – was created by **Claude Opus 5.5** (model ID `claude-opus-5-5`),
> a large language model by [Anthropic](https://www.anthropic.com). The project owner only
> specified the requirements and gave feedback; he did not write the code.
> The code was compiled and tested (unit tests and checks in the running application) during
> generation. Review it like any other third-party code before using it in production.

A desktop application in Rust (egui/eframe) for watching log files like CMTrace from MS SCCM / ConfigMgr –
with custom color rules, regex search, adjustable automatic refresh, error code lookup, export of
highlighted rows with their context and a translatable user interface.

## Download

Ready-made Windows builds are on the [Releases](https://github.com/MiroM73/Rustrace/releases) page:
`RusTrace-<version>-windows-x64.exe` is a self-extracting archive. Run it, choose a destination
folder, and it creates:

```
RusTrace\
    rustrace.exe      the program (no installation, no Visual C++ runtime needed)
    lang\*.lang       language files – edit or add translations here
    LICENSE           the Unlicense (public domain)
```

The archive is not code-signed, so Windows SmartScreen may ask for confirmation
("More info" → "Run anyway"). The `.sha256` file next to it contains the checksum.

## Building

1. Install Rust: https://rustup.rs (on Windows with Visual Studio Build Tools – "Desktop development with C++").
2. In the project folder:

```
cargo build --release
```

Result: `target\release\rustrace.exe` – a single self-contained executable.

Open a specific file: `rustrace.exe C:\Windows\CCM\Logs\AppEnforce.log`
(it can also be set as the default program for `.log` files). Files can be dragged onto the window too.

Tests: `cargo test`

The C runtime is linked statically (`.cargo/config.toml`), so the exe runs without the
Visual C++ Redistributable.

### Making a release

- Locally: `cargo build --release`, then `scripts\package.ps1` (needs 7-Zip) →
  `dist\RusTrace-v<version>-windows-x64.exe` + `.sha256`.
- On GitHub: bump `version` in `Cargo.toml`, commit, then push a tag, e.g.
  `git tag v0.1.0 && git push origin v0.1.0`. The workflow `.github/workflows/release.yml` runs the
  tests, builds, packages and creates a **draft** release with the archive attached; review it and
  publish. A manual run of the workflow (Actions → Release → Run workflow) only builds the archive as
  a downloadable artifact.

## Features

**Log formats**
- CMTrace / ConfigMgr format `<![LOG[...]LOG]!><time=... type=...>` including multi-line messages
- the older SMS format `message $$<component><date time><thread=...>`
- plain text (severity from the words error / fail / warn – can be turned off)
- encodings UTF-8, UTF-16 LE/BE (by BOM), ANSI (Windows-1250)

**Automatic refresh**
- interval in **ms, s or min** (value + unit; the value is converted when the unit changes), quick presets via ⏱
- **Pause / Resume** (Ctrl+P), manual refresh F5, full reload Ctrl+F5
- reading runs **in a background thread** in 8 MB chunks – the window never freezes, even with logs of
  hundreds of MB; the first load shows progress in % with a **Cancel** button, later refreshes in the status bar
- only newly appended data is read; test: 400 MB / 2.2 million lines ≈ 3 s (release build),
  memory roughly 2× the file size
- detects truncation/rotation (e.g. when ConfigMgr rolls a log over to `.lo_`) and reloads the file
- "Follow end" – sticks to the last line; turns off when you scroll/navigate manually, End turns it on again

**Color highlighting** (Highlighting → Highlight rules, Ctrl+H)
- any number of rules: plain text or **regex**, optional case sensitivity
- scope **Whole row** (text + background color of the row) or **Text only** (colors just the match in the message)
- optionally match the Component column as well
- order = priority (up/down arrows), on/off, live preview, invalid regex warning
- import/export of rules as JSON (e.g. to share with colleagues)
- error / warning / search match colors are configurable (Highlighting → Colors…)
- right-click a row: copy, search component, create a rule for the component, explain error code

**Themes**
- View → Theme: **System** (follows Windows, also when it changes while running), **Light**, **Dark**
- quick toggle button in the toolbar (🖥 → ☀ → 🌙)
- every highlight rule and the error / warning / search colors have **separate colors for the light and
  the dark theme**; the editors have a "Colors for theme: Light | Dark" switch with a preview on the
  matching background and a "Copy colors from … theme" button
- rules from older versions or imported files get their light colors for the dark theme as a start

**Zoom**
- **Ctrl + mouse wheel over the log text** (the table or the message in the details pane) changes
  the font size of the log only (6 – 48 pt, also View → Font size)
- **Ctrl + mouse wheel anywhere else** (menu, toolbar, status bar, …) zooms the whole user
  interface (50 – 300 %); Ctrl+plus / Ctrl+minus / Ctrl+0 zoom the whole window from the keyboard
- both the log font size and the window zoom are remembered

**Error codes** (Tools → Error code lookup, Ctrl+E)
- the details pane automatically explains codes found in the selected message
  (`0x80070005`, `-2147024891`, `exit code 1603` …)
- the lookup window accepts hex, negative decimal or Win32/MSI codes and shows all forms (hex, decimal,
  Win32), the facility and a description
- descriptions: common ConfigMgr, Windows Update, MSI, WMI, WinINet and Win32 codes from the language
  files + on Windows the system message (`FormatMessage`, in the OS language, also from `wininet.dll`)

**Export highlighted rows with context** (File → Export highlighted rows with context…, Ctrl+Shift+S)
- exports every highlighted row together with a chosen number of **lines before and after** it
  (0 – 100 000 each)
- what counts as highlighted is selectable: rows matching enabled highlight rules, errors, warnings,
  search matches
- **overlapping or adjacent ranges are merged**, so no row is exported twice; ranges are clamped at
  the start and end of the file
- formats:
  - **Original lines** – the exact lines from the log file (multi-line entries complete), so the result
    opens again in CMTrace / RusTrace
  - **Readable table** – tab-separated: highlight marker `»`, line, date/time, severity, component,
    thread, message (in the UI language, good for e-mails, tickets or Excel)
- optional separator line between blocks with the original line numbers (`-------- 9 – 15 --------`)
- **only records from a date/time** (if the log contains timestamps):
  - entered in the date format of the UI language or as ISO (`23.10.2026 13:05`, `2026-10-23 13:05:30`);
    the time part, seconds and milliseconds are optional
  - quick presets: time of the selected row, last hour, last 24 h, last 7 days – relative to the
    newest record in the log (log time, not the PC clock)
  - highlighted rows older than the time are ignored and older context lines are cut off, so the
    export contains nothing from before the chosen time; invalid input is reported and blocks the export
  - logs without timestamps show a notice instead
- **only the last N blocks** – keeps the newest N blocks after merging (e.g. "what happened during
  the last 3 failures"); can be combined with the time filter
- **rows visible in the table** as the highlighted rows – every row currently shown in the table
  (search filter in Filter mode and the Info / Warnings / Errors toggles) counts as highlighted and
  the other criteria are not used (the checkbox shows how many rows are visible, e.g.
  "1124 of 53762"). Example: show only Errors, 5 lines before/after, only the last 2 blocks →
  the two newest errors (or groups of close errors) with 5 surrounding lines of any kind
- the lines before and after are always taken from the whole file, independent of the current filter
- live preview of the number of highlighted rows, blocks and rows to export
- order of processing: highlighted rows → time filter → context ranges merged → older context
  clipped → last N blocks
- writing runs in a background thread; settings are remembered

**Search**
- text or regex, Aa = match case, number of matches
- Enter/F3 = next, Shift+Enter/Shift+F3 = previous
- **Filter** (Ctrl+L) – show only matching rows
- Info / Warnings / Errors toggles

Settings and rules are saved automatically to `%APPDATA%\RusTrace\config.json`
(Linux: `~/.config/RusTrace/config.json`).

## Languages

Built in: **English, Slovenčina, Čeština, Deutsch**. Choose under View → Language. "Automatic" follows
the operating system language (falls back to English).

### Adding or fixing a translation – no recompiling

Language files are plain UTF-8 text files named `<code>.lang` (`fr.lang`, `pl.lang`, `pt-BR.lang` …):

```
# comment
language.name = Français
format.date = {d}/{m}/{y}
menu.file = Fichier
status.entries = Entrées : {count}
```

1. View → Language → **New translation…**, enter a code (e.g. `fr`) → a copy of the English file is created
   and opened. (Or copy `lang/en.lang` manually.)
2. Translate the text after `=`. Keep the keys and placeholders like `{count}` unchanged.
3. View → Language → **Reload language files** and pick the new language.

Files are loaded from:
- `lang\` next to `rustrace.exe` (good for distributing with the program),
- `%APPDATA%\RusTrace\lang\` (per user; Linux `~/.config/RusTrace/lang/`).

Rules:
- missing keys fall back to English, so a partial translation works too,
- a file named like a built-in language (`sk.lang`, `de.lang` …) overrides only the keys it contains –
  handy for fixing a single text,
- `\n` in a value is a line break,
- error code descriptions use keys `code.0xXXXXXXXX` (8 hex digits, upper case; Win32/MSI codes as
  `0x8007xxxx`), so you can **add your own error codes** in a language file as well,
- `key.ctrl` / `key.shift` localize modifier names in shortcuts (e.g. German "Strg").

The built-in languages live in `lang/*.lang` in the source tree and are embedded at compile time;
`cargo test` checks that every built-in language has all English keys with the same placeholders.

## Keyboard shortcuts

| Shortcut | Action |
|---|---|
| Ctrl+O | Open file |
| F5 / Ctrl+F5 | Refresh / reload the whole file |
| Ctrl+P | Pause / resume automatic refresh |
| Ctrl+F | Search |
| Enter, F3 / Shift+F3 | Next / previous match |
| Ctrl+L | Filter on/off |
| Ctrl+H | Highlight rules |
| Ctrl+E | Error code lookup |
| Ctrl+Shift+S | Export highlighted rows with context |
| ↑ ↓ PgUp PgDn Home End | Move in the table |
| Ctrl+C | Copy message of the selected row |
| Ctrl + mouse wheel | Over the log: log font size; elsewhere: zoom the whole window |
| Ctrl+plus / Ctrl+minus / Ctrl+0 | Zoom the whole window in / out / reset |

## Source layout

- `src/parser.rs` – incremental chunked file reading with progress, encoding detection, format parsing (+ tests)
- `src/errcodes.rs` – error code detection and lookup (+ tests)
- `src/export.rs` – export around highlighted rows: range merging, date/time filter and input
  parsing, last-N blocks, output formats (+ tests)
- `src/i18n.rs` – localization: language files, fallback, `t!()` macro (+ tests)
- `src/config.rs` – settings and rules (serde/JSON)
- `src/app.rs` – user interface
- `src/main.rs` – window start-up
- `lang/*.lang` – built-in translations
- `scripts/package.ps1` – packs the release build into the self-extracting archive
- `.github/workflows/release.yml` – builds and publishes releases on GitHub

- `LICENSE` – the Unlicense (public domain)
- `CLAUDE.md` – project notes for Claude (conventions, history, how to continue the work)

## License

RusTrace is free and unencumbered software released into the **public domain** under
[the Unlicense](LICENSE): anyone may copy, modify, publish, use, compile, sell or distribute it,
in source or binary form, for any purpose, without any conditions. It is provided "as is",
without warranty of any kind.

The third-party Rust libraries compiled into the program (egui/eframe, regex, serde, …) keep their
own licenses (mostly MIT / Apache-2.0).
