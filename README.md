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

Ready-made Windows builds are on the [Releases](https://github.com/MiroM73/Rustrace/releases) page,
in two forms with the same content:

- `RusTrace-<version>-windows-x64.exe` – self-extracting archive: run it and choose a destination folder,
- `RusTrace-<version>-windows-x64.zip` – plain ZIP to extract yourself (e.g. Explorer → Extract all).

Both create:

```
RusTrace\
    rustrace.exe      the program (no installation, no Visual C++ runtime needed)
    lang\*.lang       language files – edit or add translations here
    logdef\*.def      log definitions (IIS, Exchange, DISM, CBS) – edit or add your own formats here
    LICENSE           the Unlicense (public domain)
```

The programs are not code-signed, so Windows SmartScreen may ask for confirmation
("More info" → "Run anyway"). The `.sha256` files contain the checksums.

Right after the start a small **start-up window** (the RusTrace icon, name and version, "Starting…"
and a rotating ring of dots) shows that the program is starting, until the main window appears –
creating the graphics context can take a moment, especially on the first start after boot. It
follows the theme from the settings (or the Windows theme).

The window title shows the version in use, e.g. `RusTrace 0.6.0 – ccmexec.log`. **Help → About
RusTrace** shows the version, author, license, links to the project page, new versions and the
issue tracker, the build (platform) and the settings file; "Copy version information" puts e.g.
`RusTrace 0.6.0 (windows x86_64, release)` on the clipboard for a bug report.

**Updates** (Help → Check for updates…): RusTrace asks GitHub for the latest release and shows
whether it is newer than the installed version, together with its release notes. **Download and
install** downloads the release ZIP, verifies it against the published SHA-256 checksum, replaces
`rustrace.exe` and the files from the archive (`lang\`, `logdef\`, `LICENSE`; other files in those
folders are kept, the old program is renamed to `rustrace.exe.old` and deleted by the next start) and
**restarts the program with the same state**: the open log, log format, search, filter, severity
toggles, column filters, time range, selected row and the window position and size. If the program
folder is not writable (e.g. under Program Files), nothing is changed and the window points to the
release page for a manual download. Other buttons: open the release page, skip this version, later.
With "Check for a new version automatically at start-up (once a day)" (on by default, can be turned
off in the same window) a newer version opens this window at start-up and a green `⬆ <version>`
button appears in the status bar. The check uses the Windows HTTP stack (WinHTTP), so the system
proxy and certificates apply; only `api.github.com` / GitHub downloads are contacted, nothing is sent
except the request itself (user agent `RusTrace/<version>`).

**Default app for `.log` / `.lo_` files** (Windows, Help → Set as default app for .log / .lo_
files…): registers RusTrace for your user account (no administrator rights, nothing outside
`HKEY_CURRENT_USER`) and shows which program opens each extension now. Windows does not let a
program replace an app **you chose yourself** (e.g. Notepad for `.log`); the window then offers
"Open the Windows default apps settings", which opens the RusTrace page of Settings → Apps →
Default apps, where one click switches the extension to RusTrace (or right-click a `.log` file →
Open with → Choose another app → RusTrace → Always). "Remove the registration" deletes everything
RusTrace wrote. The registration stores the path of `rustrace.exe` – after moving the program,
set it as the default app again (the window warns when another copy is registered).

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

Test logs: `python scripts/gen_testlogs.py` (add `--small-only` to skip the big ones) creates the
folder `testlogs\` (not in git) with small and big logs of every supported kind – CMTrace in several
encodings, multi-line and unfinished entries, the old SMS format, plain text, CBS, DISM, IIS, a set of
rotated parts – described in `testlogs\README.txt`.
`cargo test --release testlogs_overview -- --ignored --nocapture` reads all of them and prints the
detected format, encoding, files and entry counts.

Benchmark (loading, search, random access and memory on a big log, Windows):
`RUSTRACE_BENCH_FILE=<log> cargo test --release bench_large_file -- --ignored --nocapture`
(`RUSTRACE_BENCH_MAPPED=1` = text in a temporary file)

The C runtime is linked statically (`.cargo/config.toml`), so the exe runs without the
Visual C++ Redistributable. `build.rs` embeds the program icon (drawn in code by `src/icon.rs`) and
the version information into the exe; this uses the resource compiler `rc.exe` of the Windows SDK
(installed with the Visual Studio C++ build tools).

### Making a release

- Locally: `cargo build --release`, then `scripts\package.ps1` (needs 7-Zip) →
  `dist\RusTrace-v<version>-windows-x64.exe` and `.zip`, each with a `.sha256`.
- On GitHub, in one step: `scripts\release.ps1 0.6.0` (add `-Publish` to publish right after the
  build, `-DryRun` to only run the checks and tests). It checks the repository (master, clean, not
  behind, tag not used yet), sets the version in `Cargo.toml`, runs the tests (backing up
  `%APPDATA%\RusTrace\config.json`), asks for confirmation, commits, pushes, tags, waits for the
  workflow and shows the draft. Run `Get-Help scripts\release.ps1 -Full` for the details.
- On GitHub by hand: bump `version` in `Cargo.toml`, commit, then push a tag, e.g.
  `git tag v0.1.0 && git push origin v0.1.0`. The workflow `.github/workflows/release.yml` runs the
  tests, builds, packages and creates a **draft** release with the archive attached; review it and
  publish. A manual run of the workflow (Actions → Release → Run workflow) only builds the archive as
  a downloadable artifact.

## Features

**Log formats**
- CMTrace / ConfigMgr format `<![LOG[...]LOG]!><time=... type=...>` including multi-line messages
- the older SMS format `message $$<component><date time><thread=...>`
- plain text (severity from the words error / fail / warn – can be turned off)
- **any other layout described by a log definition** – built in: **IIS web logs** (W3C),
  **Exchange Transport Connectivity logs**, **dism.log**, **CBS.log**; your own in `logdef\*.def` (see [Log definitions](#log-definitions))
- encodings UTF-8, UTF-16 LE/BE (by BOM), ANSI (Windows-1250)

**Automatic refresh**
- interval in **ms, s or min** (value + unit; the value is converted when the unit changes), quick presets via ⏱
- **Pause / Resume** (Ctrl+P), manual refresh F5, full reload Ctrl+F5
- reading runs **in a background thread** in 8 MB chunks – the window never freezes, even with logs of
  hundreds of MB; the first load shows progress in % with a **Cancel** button, later refreshes in the status bar
- only newly appended data is read; test: 400 MB / 1.8 million lines ≈ 3.7 s (release build)
- **rotated logs are merged** (View → "Merge rotated parts of the log", default on): when
  `service.log` is opened, its older parts `service.lo_` (ConfigMgr renames the full log) and
  `service-YYYYMMDD-HHMMSS.log` (newer ConfigMgr versions keeping more history files) are shown
  before it as one log, oldest first (by modification time); then a **File** column shows where
  every row comes from (line numbers are per file) and the status bar shows the number of files
  (📚, the list in its tooltip)
- **rotation while watching**: when the active log is renamed to `.lo_` (or a dated file) and a new
  one is started, the rest of the old file is read from its new name and the new file is appended –
  nothing read so far disappears. A part overwritten by a later rotation is marked "(overwritten)";
  the export of original lines then uses the message texts kept in memory for it
- other truncation / replacement of the file (also with different content of the same size) is
  detected and the file is read again
- "Follow end" – sticks to the last line; turns off when you scroll/navigate manually, End turns it on again

**Memory**
- entries are stored compactly: 32 bytes per entry plus the message text; components, threads
  and other repeating values are stored only once, date/time as a number
- **log text in a temporary file** (View → "Log text in a temporary file from … MB", default on,
  from 100 MB): the message text of big logs is kept in a memory-mapped temporary file in `%TEMP%`
  that only RusTrace can open and that Windows deletes when the program ends. While memory is free
  Windows keeps it cached (as fast as memory); when memory is short it drops those pages instead of
  swapping the program. The status bar shows 💾 when it is used. Applies when a file is read from
  the start (open, Ctrl+F5). The log file itself is never mapped, so the program writing it can
  still truncate or rotate it.
- **protection for logs watched for a long time** (a rotating log keeps growing in memory,
  because nothing read so far is dropped):
  - when the message text of a watched log grows past the temporary file threshold, it moves to
    the temporary file automatically (not only when the file is opened)
  - **View → "Keep at most … MB"** (default on, 1000 MB of message text): above the limit the
    oldest records are removed from memory and from the view, in one go down to three quarters of
    the limit; the status bar shows ✂ with the number of removed records, Ctrl+F5 reads the file
    again; the file on disk is not changed. Removed records are no longer exported.
- the window is drawn with **OpenGL**: an idle window takes practically no CPU (with Direct3D 12 /
  Vulkan through wgpu, the AMD graphics driver kept one CPU core fully busy, issue #6) and the empty
  window takes ~284 MB private bytes instead of ~325 MB. When OpenGL cannot start (e.g. a remote
  desktop or a virtual machine with only the software OpenGL 1.1), the program uses wgpu instead,
  with Direct3D 12 when available. The environment variable `RUSTRACE_RENDERER=wgpu` (or `glow`)
  forces one of them; `WGPU_BACKEND=dx12|vulkan|gl` chooses the wgpu backend
- measured with a 400 MB CMTrace log (1.8 million lines) with the earlier Direct3D 12 drawing,
  private bytes of the whole program:

  | | private bytes | working set |
  |---|---|---|
  | version 0.2.0 | 1231 MB | 938 MB |
  | log text in a temporary file (default) | 421 MB | 419 MB |
  | everything in memory | 582 MB | 419 MB |

  of which ~325 MB is the window and graphics stack itself; loading takes the same time and
  searching is slightly faster than before

**Color highlighting** (Highlighting → Highlight rules, Ctrl+H)
- any number of rules: plain text or **regex**, optional case sensitivity
- scope **Whole row** (text + background color of the row) or **Text only** (colors just the match in the message)
- optionally match the Component column as well (and the extra columns of a log definition)
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
- optional separator line between blocks with the original line numbers (`-------- 9 – 15 --------`);
  for a log merged from rotated parts with the file name (`-------- service.lo_: 9 – 15 --------`),
  original lines are read from the right file and a block over a file boundary is split
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
- text or regex, Aa = match case; the number of matches (or an invalid-regex warning) and the ✖ clear
  button are shown right after the search field
- **search as you type**: from 3 characters on the search runs while typing and the first matching
  row (from the top of the table) is selected and scrolled to; a shorter text is searched with
  Enter / F3 (the field then shows "Enter = search (automatically from 3 characters)")
- **search history** (🕘 next to the search field): the last 30 search texts, newest first, kept
  between sessions; a text is remembered when you search with Enter / F3 / the arrows or leave the
  field; picking one searches it again; "Clear search history" empties the list
- searches the message, the component and the extra columns of a log definition
- when a match in a long message would not be visible in the Message cell (past the right edge of
  the column, or on a later line of a multi-line message), the cell shows the part of the message
  around the match, starting with "…"
- the details pane highlights the matches in the full message (and the text highlight rules) and
  scrolls to the first match
- Enter/F3 = next, Shift+Enter/Shift+F3 = previous
- **Filter** (Ctrl+L) – show only matching rows
- Info / Warnings / Errors toggles
- **Column filters** (toolbar button) – a filter field under every column header; the fields
  always follow the columns of the open log type (CMTrace, DISM, CBS, IIS or your own log
  definition). All active column filters must match, together with the search filter and the
  severity toggles. Syntax (case-insensitive):
  - `text` – the value contains the text, `=text` – exactly this value
  - `a|b` – any of them (`Error|Warning`, `=404|=500`)
  - `!` at the start – none of them (`!Info`, `!=0`)
  - the date/time column is filtered as displayed (e.g. `23.10.2026 10:2`)
  - an active field is highlighted, the button shows the number of active filters, ✖ in the line
    column clears them all; they are cleared when a log with other columns is opened
- **Time range** (toolbar button 🕑) – show only records from – to a date/time:
  - entered in the date format of the UI language or as ISO; the time is optional; either bound
    may stay empty (only "from" or only "to")
  - "to" includes the whole entered day / hour / minute / second (`23.10.2026` = the whole day,
    `23.10.2026 10:30` = up to 10:30:59.999)
  - presets: from / to the selected row, last hour, last 24 h (relative to the newest record)
  - rows without a date/time are hidden while a range is active; invalid input is marked ⚠
  - works together with the search filter, the severity toggles and the column filters; the
    button shows "(on)" while a range is active, also when the row is hidden
- the "rows visible in the table" export uses the column filters and the time range too

**Position in the log** – the status bar shows where the selected row is among the rows shown in
the table: `📍 Row 25 / 100 (25 %)`; with a filter active the numbers count only the visible rows,
the tooltip shows the line number in the file.

Settings and rules are saved automatically to `%APPDATA%\RusTrace\config.json`
(Linux: `~/.config/RusTrace/config.json`).

## Log definitions

Logs that are not in the CMTrace format can be described by a **log definition** – a small text
file `<name>.def` that says how one line is split into columns (date/time, severity, component,
message, …). The table then shows exactly these columns; severity coloring, search, highlight rules,
filters, the time filter and the export work as with CMTrace logs.

Built in: `iisweb.log.def` (IIS web logs `u_ex*.log`, W3C format), `exchconnect.log.def` (Exchange
Server Transport Connectivity logs `CONNECTLOG*.LOG`, comma-separated W3C-like format),
`dism.log.def` (`dism.log`) and `cbs.log.def` (`CBS.log`, `CbsPersist_*.log`).

**Choosing the format** – View → Log format:
- **Automatic** (default for every opened file): a definition whose `files` patterns match the file
  name, otherwise one whose `detect` regex matches one of the first lines of the file (CMTrace / SMS
  content always keeps the built-in parser), otherwise the built-in CMTrace / plain text parser.
- **CMTrace / plain text (built-in)** or any definition can be chosen manually; the file is read again.
- The status bar shows the format in use (📐).

**Files** are loaded – without recompiling – and come in two groups:

- **predefined**: the built-in definitions; the copies in `logdef\` next to `rustrace.exe` (shipped
  with the program) replace the built-in ones with the same file name,
- **your own**: every `.def` file in `%APPDATA%\RusTrace\logdef\` (View → Log format → Open log
  definitions folder; Linux `~/.config/RusTrace/logdef/`). Your own definitions **never replace** a
  predefined one – a copy of `iisweb.log.def` in your folder and the predefined `iisweb.log.def` are
  both available, and you can switch between them in View → Log format.

View → Log format → **Use log definitions**: *predefined and my own* (default), *only predefined*
or *only my own*. After editing a file use **Reload log definitions**; errors (invalid regex, unknown
key) are listed in that menu.

When the names of several definitions match the opened file, the one with the highest priority is
used: **your folder** → `logdef\` next to the program → built-in; within one folder by file name.
The status bar shows the definition in use with its file name (the tooltip shows its path) and ⚠
when several match; View → Log format lists them with the one in use marked ✔ and shows where every
definition comes from (your folder / next to the program / built-in). To switch a definition off,
delete it or change its extension (e.g. `my-iis.def.off`), or choose another one in the menu.

**New and changed predefined definitions**: RusTrace remembers the predefined definitions you have
seen. When a new version brings new, changed or removed ones, a window lists them at start-up – for a
changed definition every changed key with its value before and now, and if you have your own copy
with the same file name, for every key whether your copy already has the new value, still has the old
one (take over the new one) or has a value of its own (check it). "Got it" stores the current state,
"Later" shows the notice again at the next start (also View → Log format → New and changed predefined
definitions…). The first start only remembers the current state.

**Format** – `key = value` per line, `#` starts a comment line. Example (`cbs.log.def`):

```
name = Windows CBS log
files = cbs.log; cbs*.log; cbspersist_*.log
detect = ^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}, \w+\s+(CBS|CSI)\s
pattern = ^(?P<datetime>\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}),\s+(?P<severity>\w+)\s+(?P<component>\S+)(?:\s+(?P<message>.*))?$
datetime.format = %Y-%m-%d %H:%M:%S
unmatched = append
severity.error = ^(error|fatal|critical)$
severity.warning = ^warn(ing)?$
columns = datetime severity component message
title.severity = Level
```

| Key | Meaning |
|---|---|
| `name` | name shown in the UI |
| `files` | file name patterns (`*`, `?`, case-insensitive) separated by `;` – default: the file name without `.def` |
| `detect` | regex; used for files whose name matches no definition – applies when one of the first 30 lines matches |
| `format` | `regex` (default) or `w3c` – W3C extended log format (IIS): column names from the `#Fields:` header (when a later header has other fields, the table shows the columns of all headers), space-separated values, `-` = empty |
| `pattern` | regex for one line (format `regex`); every **named group** `(?P<name>…)` is a column |
| `fields` | `w3c`: column names used until a `#Fields:` header is found |
| `separator` | `w3c`: what separates the `#Fields:` names and the values – `space` (default), `tab` or one character, e.g. `,` for Exchange logs. With a separator other than space the values are read CSV-style: a value in quotes may contain the separator (`"a, b"`, `""` inside = `"`), and `-` stays a value (in Exchange logs `direction` `-` means disconnect) |
| `skip` | regex; matching lines are ignored |
| `unmatched` | lines not matching the pattern: `append` to the previous entry (multi-line entries, default for `regex`), `entry` = own row (default for `w3c`), `skip` |
| `role.datetime`, `role.message`, `role.component`, `role.thread` | which column(s) fill the standard columns (several are joined with a space, e.g. `role.message = cs-method cs-uri-stem cs-uri-query`); default: the column with the same name (`datetime` also `date` + `time`); without a message column the whole line is the message |
| `role.severity` | column whose value gives the severity (default `severity`); it stays visible as a column |
| `severity.error`, `severity.warning` | regexes (case-insensitive) for that value – e.g. `^5\d\d$` for HTTP 5xx; default: error/err/e/fatal/critical/3 and warning/warn/w/2; without a severity column the error/fail/warn keywords are used |
| `datetime.format` | [chrono format](https://docs.rs/chrono/latest/chrono/format/strftime/index.html)(s) separated by `\|`; ISO-like dates are recognized without it |
| `timezone` | `local` (default) or `utc` – converted to local time (IIS logs are in UTC) |
| `columns` | order of the table columns (role or column names); default: the order in the pattern / header |
| `title.<column>` | column header, e.g. `title.sc-status = Status`, `title.message = Request` |

Columns not used by a role (e.g. IIS `sc-status`, `c-ip`, DISM `function`) are **extra columns**:
they are shown in the table and the details pane, searched by the search box and by highlight rules
with "Also component", and appended to the readable export.

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
| Shift + mouse wheel | Scroll the table horizontally (when the columns do not fit; also the scroll bar below the table) |
| Ctrl+C | Copy message of the selected row |
| Ctrl + mouse wheel | Over the log: log font size; elsewhere: zoom the whole window |
| Ctrl+plus / Ctrl+minus / Ctrl+0 | Zoom the whole window in / out / reset |

## Source layout

- `src/parser.rs` – incremental chunked file reading with progress, encoding detection, format parsing (+ tests)
- `src/colfilter.rs` – column filter syntax (+ tests)
- `src/store.rs` – compact entry storage: 32-byte records, text arena in memory or in a
  memory-mapped temporary file, string interner, date/time as milliseconds (+ tests)
- `src/logdef.rs` – log definitions: `.def` file format, loading, choosing by file name / content,
  parsing lines into columns (+ tests)
- `src/errcodes.rs` – error code detection and lookup (+ tests)
- `src/fileassoc.rs` – Windows file association for `.log` / `.lo_` (registry, status, + tests)
- `src/export.rs` – export around highlighted rows: range merging, date/time filter and input
  parsing, last-N blocks, output formats (+ tests)
- `src/i18n.rs` – localization: language files, fallback, `t!()` macro (+ tests)
- `src/config.rs` – settings and rules (serde/JSON)
- `src/app.rs` – user interface
- `src/main.rs` – window start-up, renderer choice (OpenGL, wgpu / Direct3D 12 as the fallback)
- `src/update.rs` – new versions: GitHub release, version comparison, ZIP extraction, SHA-256 check,
  installation over the running program, state for the restart (+ tests)
- `src/http.rs` – HTTPS download via WinHTTP (system proxy and certificates)
- `src/splash.rs` – start-up window with a rotating ring of dots (Windows, Win32 / GDI)
- `src/icon.rs` – the program icon drawn in code (window, start-up window, `.ico` for `build.rs`)
- `build.rs` – embeds the icon and the version information into `rustrace.exe` (Windows)
- `lang/*.lang` – built-in translations
- `logdef/*.def` – built-in log definitions (IIS, Exchange, DISM, CBS)
- `scripts/package.ps1` – packs the release build into the self-extracting archive and the ZIP
- `scripts/release.ps1` – the whole GitHub release (checks, version, tests, commit, tag, build, publish)
- `scripts/gen_testlogs.py` – generates the test logs in `testlogs/` (not in git)
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
