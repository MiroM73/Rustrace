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

[![CI](https://github.com/MiroM73/Rustrace/actions/workflows/ci.yml/badge.svg?branch=master)](https://github.com/MiroM73/Rustrace/actions/workflows/ci.yml)

**Guides** – step-by-step help for real situations with annotated screenshots and command-line
examples: [docs/guides](docs/guides/README.md), e.g.
[An application deployment failed](docs/guides/failed-app-install.md),
[Software updates do not install](docs/guides/software-updates.md),
[A new ConfigMgr client does not register](docs/guides/client-install.md),
[The management point / distribution point is not healthy](docs/guides/iis-management-point.md),
[The ConfigMgr client installation fails](docs/guides/client-msi-1603.md).
In the program: **Help → Guides** (one entry per guide, *All guides…* = the index; opened on GitHub
in the browser, the guides are in English), the About window, and a 📖 button next to the log sets
that have a guide (File → Open log set…).

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
    logdef\*.def      log definitions (IIS, Exchange, DISM, CBS, MSI) – edit or add your own formats here
    logsets\*.set     log sets (groups of ConfigMgr / Intune logs opened together)
    LICENSE           the Unlicense (public domain)
```

The programs are not code-signed, so Windows SmartScreen may ask for confirmation
("More info" → "Run anyway"). The `.sha256` files contain the checksums.

Right after the start a small **start-up window** (the RusTrace icon, name and version, "Starting…"
and a rotating ring of dots) shows that the program is starting, until the main window appears –
creating the graphics context can take a moment, especially on the first start after boot. It
follows the theme from the settings (or the Windows theme).

The window title shows the version in use, e.g. `RusTrace 0.6.0 – ccmexec.log`. **Help → About
RusTrace** shows the version, author, license, links to the project page, new versions, the
issue tracker and the guides, the build (platform) and the settings file; "Copy version information" puts e.g.
`RusTrace 0.6.0 (windows x86_64, release)` on the clipboard for a bug report.

**Updates** (Help → Check for updates…): RusTrace asks GitHub for the published releases and shows
whether there is a newer one than the installed version, together with its release notes. When
several versions were released since yours, the window lists **all of them** with their dates and
release notes, newest first (the newest one open, older ones folded), so nothing that changed in
between is missed; drafts and pre-releases are ignored. **Download and
install** downloads the release ZIP, verifies it against the published SHA-256 checksum, replaces
`rustrace.exe` and the files from the archive (`lang\`, `logdef\`, `logsets\`, `LICENSE`; other files in those
folders are kept, the old program is renamed to `rustrace.exe.old` and deleted by the next start) and
**restarts the program with the same state**: the open log, log format, search, filter, severity
toggles, column filters, time range, selected row and the window position and size. If the program
folder is not writable (e.g. under Program Files), nothing is changed and the window points to the
release page for a manual download. Other buttons: open the release page, skip this version, later.
"Check for a new version: **at every start** (default) / **once a day** / **never**" at the bottom of
the window sets the automatic check: it runs in the background without slowing down the start, a
newer version opens this window and a green `⬆ <version>` button appears in the status bar (it
stays there after "Later" and opens the window again). A skipped version does not open the window
again, a newer one does. Errors of the automatic check (e.g. no internet) are not shown, and there
is no check right after the program was restarted by an update. The check uses the Windows HTTP stack (WinHTTP), so the system
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
Options for opening at a line, searching and filtering: see [Command line](#command-line).

Tests: `cargo test`

Checks before a commit / pull request (the same ones the CI runs): `cargo fmt --check`,
`cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked`.
The workflow `.github/workflows/ci.yml` runs them on every push to `master` and on every pull
request – on Windows (plus a release build, downloadable as the `rustrace-windows-<commit>`
artifact of the run) and on Linux (clippy and tests, so the build keeps working there).
Formatting-only commits are listed in `.git-blame-ignore-revs`
(`git config blame.ignoreRevsFile .git-blame-ignore-revs` makes a local `git blame` skip them).

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

**Several logs in one view, sorted by time** (like CMTrace "Merge selected files")
- select **several files** in File → Open (Ctrl / Shift + click), **drop several files** onto
  the window, or start `rustrace.exe AppDiscovery.log AppEnforce.log` – they are shown as one
  table interleaved by date/time, e.g. `AppDiscovery.log` + `AppEnforce.log` +
  `AppIntentEval.log` to follow one deployment across components
- **File → Add log to view…** adds more logs to the open view; **File → Merged logs** lists them
  and removes one (✖) from the view
- order: by date/time; equal times keep the order of the logs, then the order of the lines; a
  row without a date/time stays right after the row before it in the same log; the order of the
  rows inside one log never changes
- the **File** column shows where every row comes from, with a color mark per log (View →
  "Color mark per log in the File column"); the status bar shows "📚 N logs merged" – its
  tooltip lists every log with its format, its files, their row counts and encodings
- every log is watched and refreshed on its own; new rows normally go at the end, a row older
  than the newest one in the view is merged into the right place (only the end of the view after
  its time is rearranged)
- logs of **different formats** can be merged (e.g. CMTrace + plain text + IIS): the columns are
  united – the standard columns once, the extra columns of log definitions by name
- rotated parts of every merged log are still included; when a log is truncated or replaced, the
  whole view is read again; a log that cannot be read is reported in the status bar and the others
  are shown
- search, highlight rules, filters, time range and export work across all logs; the export of
  original lines writes blocks per file with the file name in the separator
- loading three big logs merged (400 MB CMTrace + 100 MB CBS + 100 MB IIS, 3.7 million rows)
  takes about as long as loading them one after another (~9.3 s)

**Log sets** – the logs of one troubleshooting scenario in one step (see [Log sets](#log-sets))
- **File → Open log set…**: choose a set (e.g. *Application deployment* = `AppDiscovery.log`,
  `AppEnforce.log`, `AppIntentEval.log`, `CAS.log`, `ContentTransferManager.log`,
  `DataTransferService.log`), see which of its logs exist (✔ / ✖ with size and last write time,
  rotated older parts), untick the ones you do not need and **Open** them as one view merged by time
- **on this computer or on a remote one** (name or IP address → its administrative shares
  `\\<computer>\admin$` / `\\<computer>\c$`, with your Windows account; recently used computers
  are remembered); an offline computer or missing rights give a clear message within seconds,
  the window never freezes
- built in: application deployment, software updates, client installation, policy, Intune /
  co-management; your own sets are small text files

**Details pane: message or raw line**
- the details pane under the table shows the selected entry: its line number, severity, the other
  columns and the full message
- **Message | Raw line** switch in the details pane header (also View → "Details pane shows:"),
  remembered between sessions: *Message* = the parsed message, *Raw line* = the line(s) of the
  entry exactly as they are in the log file (CMTrace markup with all attributes, every W3C field,
  multi-line entries complete), decoded with the file's encoding and nothing else
- switching is instant and keeps the selection and the table position, also while the log is
  followed; search matches and text highlight rules are highlighted in both views and the pane
  scrolls to the first match
- the raw line is read from the file only for the selected entry (a few bytes from its remembered
  position, no re-parsing). When the file part is no longer there (overwritten by a later rotation,
  truncated, deleted), the pane says "Raw line not available"

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
- "Follow end" – sticks to the last line. Selecting a row that is not the last one (click, keyboard,
  F3 / F2, bookmarks list, overview strip, `--line`) only **pauses** it: the table stays at the
  row, new rows arrive below it and filter changes keep the row in view; the checkbox turns orange
  and the status bar shows **⏸ Follow end paused**. It resumes when the selection is cleared
  (**Esc**, Ctrl+click on the only selected row, a click below the last row), when you reach the
  last row (End, a click on it, scrolling to the very bottom) or with a click on the status bar
  item. Unticking the checkbox (or `--no-follow`) turns it off until you tick it again

**Memory**
- entries are stored compactly: 40 bytes per entry plus the message text; components, threads
  and other repeating values are stored only once, date/time as a number; the 40 bytes include
  the position of the entry in its file (for the raw line in the details pane, +8 bytes per entry:
  400 MB / 1.9 million lines: 223 → 237 MB, loading ~2 % slower)
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
- the window opens wide enough for the whole table (up to the size of the main window); long names
  and patterns are shown in full in the tooltip of their field; a narrower window shows a
  horizontal scroll bar
- import/export of rules as JSON (e.g. to share with colleagues)
- error / warning / search match, bookmark and long-gap (Δ column) colors are configurable
  (Highlighting → Colors…)
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
- codes found in the message: `0x…` HRESULTs, negative decimals, `exit / error / return code N`
  and the final `error status: N` of a Windows Installer log
- descriptions: common ConfigMgr, Windows Update, MSI, WMI, WinINet and Win32 codes from the language
  files + on Windows the system message (`FormatMessage`, in the OS language, also from `wininet.dll`)
- **HTTP status codes** (IIS logs and any log whose definition names its status columns): the details
  pane of the selected row explains the HTTP status with the IIS substatus (e.g. `404.3` Not Found:
  MIME type restriction) and the Win32 status (e.g. `50`, `64`, `1236` – via the Win32 lookup above);
  hovering a status / substatus / Win32 status cell in the table shows the same explanation as a
  tooltip. Codes without an own description show their class (e.g. "4xx – client error")
- the lookup window also accepts HTTP statuses: `404.3`, `HTTP 500`, `http 403.14` show only the
  HTTP meaning; a **plain number from 100 to 599** (e.g. `404`) is shown both as an error code
  (Win32 404) and below it as an HTTP status (enter `HTTP 404` for the HTTP meaning only); other
  numbers (`1603`, `0x80070005` …) work as before

**Export highlighted rows with context** (File → Export highlighted rows with context…, Ctrl+Shift+S)
- exports every highlighted row together with a chosen number of **lines before and after** it
  (0 – 100 000 each)
- what counts as highlighted is selectable: rows matching enabled highlight rules, errors, warnings,
  search matches, bookmarked rows
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
- the automatic refresh does not have to be paused: the export takes the rows the preview shows;
  only while the log is read from the beginning (first load, full reload) the Export… button waits
  with "Reading the log…"

**Export table – CSV / Excel** (File → Export table (CSV / Excel)…, Ctrl+Shift+E; row context menu →
Export selected rows to CSV / Excel…) – the **parsed** log as a real table for Excel / Power BI,
tickets or colleagues without RusTrace: every column is a spreadsheet column
- **rows** (with a live count): the whole parsed log (all merged logs / rotated parts), the rows
  shown in the table (search filter, severity toggles, column filters, time range), the selected
  rows, the bookmarked rows, or the highlighted rows with context – the plan of *Export highlighted
  rows* (its criteria, lines before / after, since, last N blocks; ⚙ Settings opens it) with a
  **Block** column numbering the blocks
- **columns**: by default the columns of the table in their order (line, file, date/time, Δ,
  component, thread, message, the columns of the log definition); tick / untick and reorder them
  with ↑ ↓ – the choice is remembered **per log type**. Additional columns: **Severity** as text
  (UI language or English), **Source file** (full path), **Highlight rule** (the first matching
  rule – handy for filtering), **Bookmark note**, **Error code explanation** (`0x80070005 – Access
  is denied.` like the details pane), **Raw line(s)** (the original text, read from the file),
  **Block**; Δ is in seconds (since the previous exported row, or as set for the Δ column)
- **CSV options**: separator comma / semicolon / tab, default the **Windows list separator** (`;`
  for SK / CS / DE, so Excel opens the file correctly on a double click; numbers with a fraction then
  use a decimal comma); encoding **UTF-8 with BOM** (default, Excel recognizes it), UTF-8 without BOM,
  UTF-16 LE; header row on / off; dates as ISO `2026-10-27 19:49:05.123` (default, sorts correctly)
  or as in the table; RFC 4180 quoting (values with the separator, a quote or a line break in quotes,
  quotes doubled) – **multi-line messages stay in one cell**
- **Excel (.xlsx)** – written by RusTrace itself (no Excel needed, no extra library): **real typed
  cells** – the date/time is an Excel date with the format `yyyy-mm-dd hh:mm:ss.000` (sortable,
  filterable by date), line numbers, block numbers and Δ (seconds) are numbers, everything else is
  text (log text is never evaluated as a formula); bold **frozen header row with AutoFilter**,
  sensible column widths (the message column wide); optional **Export with colors** – the row text
  and background of the highlight rules and of errors / warnings as in the table (light theme).
  Excel limits: more than 1 048 576 rows continue on the next sheet (`Log`, `Log (2)`, …, every one
  with its header); a cell longer than 32 767 characters is cut with `…` and the status bar says how
  many were cut. The whole 400 MB test log: two sheets, 135 MB, about 8 s
- Excel extra sheets (each can be turned off): **Summary** – RusTrace version, export time, log
  type, the files, entries in the log, which rows were exported, search text, severities shown,
  column filters, time range, rows / errors / warnings in the export, first and last date/time,
  number of data sheets, cut cells; **Statistics** – errors and warnings per component, the 20 most
  frequent error codes (in error / warning rows) with their explanation, entries / errors / warnings
  per hour (a quick start for a pivot table or a chart); for merged logs optionally **one sheet per
  source file** (otherwise one sheet with the File column)
- **Safe for Excel** (default on): log text starting with `=`, `+`, `-`, `@` (or a tab / CR) gets a
  leading `'`, so a spreadsheet never runs it as a formula (CSV / formula injection)
- runs in a background thread straight from the parsed entries (no copy of the log): the whole 400 MB
  test log (1.9 million rows) is written in about 3 s; progress and **Cancel** in the window and in
  the status bar, a cancelled export leaves no file. New rows read meanwhile are added after it.

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
- **big logs stay responsive**: on logs with more than 100 000 records the search and all filters
  (filter mode, severity toggles, time range, column filters) run in the background on all CPU
  cores but one; the table keeps showing the previous result, the status bar shows
  "Searching…" when it takes longer than a moment, and a new keystroke stops the old search
  (400 MB / 1.9 M records: no frame over ~7 ms while typing, before ~160 ms per character).
  After an auto refresh only the new rows are filtered and the match count is updated, never
  recounted over the whole log
- Info / Warnings / Errors toggles
- **Column filters** (toolbar button) – a filter field under every column header; the fields
  always follow the columns of the open log type (CMTrace, DISM, CBS, IIS or your own log
  definition). All active column filters must match, together with the search filter and the
  severity toggles. Syntax (case-insensitive):
  - `text` – the value contains the text, `=text` – exactly this value
  - `a|b` – any of them (`Error|Warning`, `=404|=500`)
  - `!` at the start – none of them (`!Info`, `!=0`)
  - the date/time column is filtered as displayed (e.g. `23.10.2026 10:2`); a filter that fixes
    the date from the year on (`23.10.2026`, `23.10.2026 10:3`, `10.2026`, `2026`) is compared
    with the stored time directly – about 30× faster on big logs than comparing texts
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

**Overview strip** – a narrow strip right of the table's vertical scroll bar shows where the
errors, warnings, search matches and bookmarks are in the whole log (the rows shown in the
table, top to bottom), like the markers in VS Code or Notepad++
- colors of the current theme (error / warning / search / bookmark colors); when several fall
  into one pixel: bookmark > search match > error > warning
- the part of the log in view is drawn as a translucent rectangle; a **click or drag** scrolls
  there, the tooltip shows the row, its line in the file and its time
- View → "Overview strip next to the scroll bar" turns it off and chooses the marks shown
- the marks are computed once after a search / filter change (after a refresh only for the new
  rows) and kept in at most 4096 buckets, so drawing costs a few hundred rectangles per frame

**Δ time column** – to find where an installation or update "hangs", the gaps between entries
matter more than the absolute times
- turned on in the View menu or with a right click on any column header ("Δ time column"); it
  is shown after the date/time column and hidden by default
- modes (View menu): **since the previous row shown** (default – respects the filters, so with
  only errors shown it is the time between errors), **since the previous row of the log**
  (ignores the filters), **since the reference row** (right click on a row → "Set as time
  reference"; rows before it are negative)
- the format adapts to the size: `+0.120 s`, `+12.5 s`, `+3 min 05 s`, `+1 h 02 min`; rows without
  a date/time have an empty cell
- **long gaps are colored**: from 30 s by default (View → "Color gaps of at least … s"), colors
  "Long gap (Δ column)" per theme in Highlighting → Colors
- the details pane shows the time to the previous row shown and to the reference row; the
  readable-table export and "Copy as table" include the Δ column while it is shown
- computed from the stored timestamps only for the rows drawn – no memory per row, works for
  merged logs and rotated parts too

**Selecting several rows** – like the lists in Windows Explorer
- **Shift + click** / **Shift + ↑ ↓ PgUp PgDn Home End** select a range from the last clicked row,
  **Ctrl + click** adds / removes one row, **Ctrl + Shift + click** adds a range, **Ctrl+A**
  selects every row shown in the table
- the selection belongs to the log rows, not to the table positions: it stays when the filters
  change (hidden selected rows come back selected) and after refreshes
- **Ctrl+C** copies the messages of all selected rows, **Ctrl+Shift+C** (context menu "Copy as
  table") copies them tab-separated with the columns of the table and a header line, the context
  menu "Copy original lines" copies their lines exactly as they are in the log file (read in the
  background); with one selected row everything works as before
- context menu **"Export selected rows…"** opens the export with the selected rows as the
  highlighted rows, so context lines can be added
- the status bar and the details pane show "N rows selected, span 3 min 05 s" (time between the
  first and the last selected row); the details pane shows the first selected row

**Bookmarks** – mark interesting rows ("download started", "install failed") and jump back to them
- **Ctrl+F2** (or right-click → Add bookmark, or 🔖 in the details pane) bookmarks / un-bookmarks
  the selected row; **F2 / Shift+F2** jump to the next / previous bookmarked row shown in the
  table (wrapping around)
- a bookmarked row has its line number cell in the bookmark color (🔖, configurable per theme in
  Highlighting → Colors); an optional **note** is edited in the details pane or the bookmarks list
  and shown as the tooltip of the line number
- **View → Bookmarks…** (or the 🔖 N button in the status bar) lists every bookmark with its line,
  time, note and message – double-click jumps to the row, ✖ removes one, "Remove all"
- **🔖 Bookmarked** in the toolbar (also View → Show only bookmarked rows) shows only the
  bookmarked rows, together with the other filters
- export: **Bookmarked rows** as a "highlighted rows" criterion, so bookmarks + context lines can be
  exported
- bookmarks are kept by file and line number, so they stay on the same log line after refreshes,
  filtering, merges of older rows of other logs, the removal of old records by the memory limit
  (bookmarks of removed rows go with them), a full reload (Ctrl+F5) and adding / removing logs of a
  merged view; they are kept over an update restart, but not saved between sessions

Settings and rules are saved automatically to `%APPDATA%\RusTrace\config.json`
(Linux: `~/.config/RusTrace/config.json`).

## Log definitions

Logs that are not in the CMTrace format can be described by a **log definition** – a small text
file `<name>.def` that says how one line is split into columns (date/time, severity, component,
message, …). The table then shows exactly these columns; severity coloring, search, highlight rules,
filters, the time filter and the export work as with CMTrace logs.

Built in: `iisweb.log.def` (IIS web logs `u_ex*.log`, W3C format; HTTP 5xx = error, 4xx = warning;
`sc-status` / `sc-substatus` / `sc-win32-status` explained and shown first – date/time, status,
substatus, Win32 status, time taken, client IP, user, request, then every other field of the
`#Fields:` header), `exchconnect.log.def` (Exchange
Server Transport Connectivity logs `CONNECTLOG*.LOG`, comma-separated W3C-like format),
`dism.log.def` (`dism.log`), `cbs.log.def` (`CBS.log`, `CbsPersist_*.log`) and `msi.log.def`
(Windows Installer verbose logs `msiexec /l*v` – `*.msi.log`, `MSI*.log` and any file starting with
`=== Verbose logging started:` or `MSI (s) (`: the date comes from the *Verbose logging started*
line, so the rows are placed at their real time in a merged view; columns MSI context `s` / `c`,
process:thread, message; errors are only the real failures – `Return value 3`, `Error 1722.`,
`returned actual error code 1603`, `@@ERR:`, *Installation failed*, `error status: 1603` – not the
harmless `Note: 1: 2205 2:  3: Error` lines or `error status: 0`; `Return value 2` = warning).

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
| `role.severity` | column(s) whose value gives the severity (default `severity`; several are joined with a space, e.g. `role.severity = message text`); it stays visible as a column |
| `severity.error`, `severity.warning` | regexes (case-insensitive) for that value – e.g. `^5\d\d$` for HTTP 5xx; default: error/err/e/fatal/critical/3 and warning/warn/w/2; without a severity column the error/fail/warn keywords are used |
| `datetime.format` | [chrono format](https://docs.rs/chrono/latest/chrono/format/strftime/index.html)(s) separated by `\|`; ISO-like dates are recognized without it |
| `timezone` | `local` (default) or `utc` – converted to local time (IIS logs are in UTC) |
| `header.date` | for logs whose lines have only a time: regex of the line that carries the date (named group `date`, else the first group), e.g. `header.date = ^=== Verbose logging started: (?P<date>\S+)`. The date/time column then holds the time of each line and the date of the last such line is put in front of it (`datetime.format` gets `YYYY-MM-DD <time>`, e.g. `%Y-%m-%d %H:%M:%S:%3f`); rows without a time get the previous date/time; a time more than 12 hours before the previous one is on the next day (midnight) |
| `header.date.format` | chrono format(s) of that date separated by `\|`; default `%Y-%m-%d \| %d.%m.%Y \| %m/%d/%Y \| %d/%m/%Y \| %Y/%m/%d` (the first that fits wins – a slash date that fits both orders is read month/day) |
| `columns` | order of the table columns (role or column names); `*` = all the other columns in the order of the pattern / header (e.g. `columns = datetime sc-status message *`); without `*` the columns not listed are hidden; default: the order in the pattern / header |
| `title.<column>` | column header, e.g. `title.sc-status = Status`, `title.message = Request` |
| `http_status`, `http_substatus`, `win32_status` | optional: the columns holding an HTTP status, an IIS substatus and a Win32 status code (decimal) – explained in the details pane and in the cell tooltip, e.g. `http_status = sc-status`; a named column missing in the log is ignored; only extra columns (not used by a role) are explained |

Columns not used by a role (e.g. IIS `sc-status`, `c-ip`, DISM `function`) are **extra columns**:
they are shown in the table and the details pane, searched by the search box and by highlight rules
with "Also component", and appended to the readable export.

## Log sets

A **log set** is a named group of logs for one troubleshooting scenario. **File → Open log set…**
opens a window: choose the set (the last one is remembered) and the computer, and the window checks
in the background which of its logs exist – ✔ *Found* with the size, the last write time
and the number of rotated older parts that are opened with it (`AppEnforce.lo_`,
`AppEnforce-20261027-101500.log`), ✖ *Not found*, 🔒 *Access denied*. Found logs are ticked; untick
the ones you do not need and press **Open** – the logs are opened as **one view merged by time**
(the same as opening several files, see [Several logs in one view](#features)). **Check again**
repeats the check (e.g. after starting the scenario on the client).

Built-in sets (`%CCMLOGS%` = the log folder of the ConfigMgr client, see below); *Application
deployment*, *Software updates* and *Client installation* have a 📖 button that opens their guide:

| Set | Logs |
|---|---|
| Application deployment | `%CCMLOGS%\` `AppDiscovery.log`, `AppEnforce.log`, `AppIntentEval.log`, `CAS.log`, `ContentTransferManager.log`, `DataTransferService.log` |
| Software updates | `%CCMLOGS%\` `WUAHandler.log`, `ScanAgent.log`, `UpdatesDeployment.log`, `UpdatesHandler.log`, `UpdatesStore.log`; `%WINDIR%\Logs\CBS\CBS.log` |
| Client installation | `%WINDIR%\ccmsetup\Logs\` `ccmsetup.log`, `client.msi.log`; `%CCMLOGS%\` `ClientIDManagerStartup.log`, `LocationServices.log` |
| Policy | `%CCMLOGS%\` `PolicyAgent.log`, `PolicyEvaluator.log`, `ClientLocation.log` |
| Intune / co-management | `%PROGRAMDATA%\Microsoft\IntuneManagementExtension\Logs\` `IntuneManagementExtension.log`, `AppWorkload.log`, `AppActionProcessor.log`, `AgentExecutor.log`; `%CCMLOGS%\CoManagementHandler.log` |

**Remote computer**: choose *Remote computer*, type its name or IP address (`pc01`,
`pc01.contoso.com`, `10.0.0.5`; a leading `\\` is fine) and press **Check** or Enter; 🕘 lists the
recently used computers (the last 15, remembered when a set is opened). The logs are read over the
**administrative shares** with **your current Windows account** – it needs administrator rights on
that computer (RusTrace never asks for or stores credentials; to use another account, connect
first, e.g. `net use \\pc01\admin$ /user:DOMAIN\admin`). The variables point to the remote
computer:

| Variable | This computer | Remote computer `pc01` |
|---|---|---|
| `%WINDIR%` | `C:\Windows` | `\\pc01\admin$` (its Windows folder, wherever it is installed) |
| `%CCMLOGS%` | client `LogDirectory` from the registry, else `%WINDIR%\CCM\Logs` | `\\pc01\admin$\CCM\Logs` (the default; its registry is not read) |
| `%PROGRAMDATA%` | `C:\ProgramData` | `\\pc01\c$\ProgramData` |
| `%SYSTEMDRIVE%` | `C:` | `\\pc01\c$` |
| a path `D:\Logs\x.log` | `D:\Logs\x.log` | `\\pc01\d$\Logs\x.log` |
| other `%NAME%` | environment variable | ✖ *Unknown variable* |

Every share is checked first and must answer within **10 seconds**; the window stays usable all
the time. Messages: *pc01 cannot be reached* (offline, wrong name, firewall blocks SMB), *No answer
from pc01 within 10 s*, *Access to* `\\pc01\admin$` *denied* (no administrator rights),
*The share* `\\pc01\c$` *does not exist* (administrative shares turned off); the rows show *Not reachable*,
*No answer*, *Access denied*, *Share missing*. The opened remote logs are refreshed automatically
like local ones, including rotation (`AppEnforce.log` → `AppEnforce.lo_`) – every refresh opens the
file again and reads only the new part, so the network carries only what was added. When the
computer goes offline while the logs are watched, the status bar shows the read error and the
view keeps what was read.

`%WINDIR%\WindowsUpdate.log` is not in the software updates set: since Windows 10 it only says to
run `Get-WindowsUpdateLog`.

**Variables** in the paths (case-insensitive): `%CCMLOGS%` = the `LogDirectory` of the ConfigMgr
client (`HKLM\SOFTWARE\Microsoft\CCM\Logging\@Global`), or `%WINDIR%\CCM\Logs` when it is not set;
`%WINDIR%`, `%PROGRAMDATA%`, `%SYSTEMDRIVE%`; any other environment variable works too. A path with
an unknown variable is shown as ✖ *Unknown variable*.

**Files** – like the log definitions, loaded without recompiling every time the window opens:
- **predefined**: the built-in sets; the copies in `logsets\` next to `rustrace.exe` (shipped with
  the program) replace the built-in ones with the same file name,
- **your own**: every `.set` file in `%APPDATA%\RusTrace\logsets\` (button **Own sets folder** in the
  window; Linux `~/.config/RusTrace/logsets/`), shown with "(own)"; they never replace a predefined
  set. Files that cannot be used (no `file` line, unreadable) are listed in red in the window.

**Format** – `key = value` per line, `#` starts a comment line, unknown keys and other lines are
ignored. Names and descriptions are translated **in the set file itself** (`name.<language>`,
`description.<language>` – the language code of the user interface, e.g. `sk`, `de`, `pt-BR`; then
the base language, then the plain `name` / `description`), so an own set carries its own
translations:

```
name = Application deployment
name.sk = Nasadenie aplikácií
description = Discovery, requirement evaluation and installation of applications.
description.sk = Zisťovanie, vyhodnotenie požiadaviek a inštalácia aplikácií.
file = %CCMLOGS%\AppDiscovery.log
file = %CCMLOGS%\AppEnforce.log
file = C:\MyApp\Logs\install.log
```

| Key | Meaning |
|---|---|
| `name`, `name.<language>` | name in the list (default: the file name without `.set`) |
| `description`, `description.<language>` | one or two sentences under the name |
| `file` | one log of the set (one line per log, in the order shown) |
| `guide` | optional, repeatable: a guide for the scenario, shown as a 📖 button next to the set – a file name of [docs/guides](docs/guides/README.md) (e.g. `guide = failed-app-install.md`, opened on GitHub with its translated title), or a URL / path of your own guide (`guide = https://intranet/wiki/AppDeploy`, `guide = \\server\docs\deploy.pdf`) |

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
- HTTP status descriptions use keys `http.<status>` (`http.404`), `http.<status>.<substatus>` for IIS
  substatus codes (`http.404.3`) and `http.1xx` … `http.5xx` for codes without an own description,
- `key.ctrl` / `key.shift` localize modifier names in shortcuts (e.g. German "Strg").

The built-in languages live in `lang/*.lang` in the source tree and are embedded at compile time;
`cargo test` checks that every built-in language has all English keys with the same placeholders.

## Command line

```
rustrace [OPTIONS] [FILE]...
```

| Option | Meaning |
|---|---|
| `--line <N>` | Select line N of the (first) FILE and scroll to it. A line inside a multi-line entry selects that entry. |
| `--search <TEXT>` | Set the search text and select the first match (also a 1–2 character text is searched). |
| `--regex` | Treat `--search` as a regular expression. |
| `--case` | Case-sensitive search. |
| `--filter` | Filter mode: show only the rows matching `--search`. |
| `--errors` / `--warnings` | Show only errors / errors and warnings. |
| `--from <TIME>` / `--to <TIME>` | Time range (`--to` inclusive, a date alone = the whole day). The same formats as the time range in the toolbar: ISO (`2026-10-27 13:05:30`, `2026-10-27T13:05`, `2026-10-27`) always, or the date order of the UI language (e.g. `27.10.2026 13:05` in Slovak). |
| `--tail` / `--no-follow` | Follow / do not follow the end of the log (default: the "Follow end" setting; the options set it like the toolbar checkbox). |
| `--merge` | Open several FILEs as one view merged by time. Several FILEs are merged also without it (the same as opening or dropping several files), the option just makes it explicit. |
| `--lang <CODE>` | UI language for this run only (`en`, `sk`, `cs`, `de` or an external language file); not saved in the settings. |
| `--help`, `-h`, `/?` | Show the options (in a window; also printed when started from a console). |
| `--version`, `-V` | Show the version. |
| `--` | Everything after it is a FILE (for file names starting with `-`). |

Options can come before or after the files, values as `--line 5` or `--line=5`; when an option is
repeated, the last one counts. `--line`, `--regex`, `--case` and `--filter` need a FILE /
`--search`. An invalid option, line number, time, language or regular expression shows a message
(a window, and on stderr) and the program ends with exit code 2. `--line` and the first search
match are selected once the log has been read; selecting a row pauses following the end, like a
click on a row. Double-clicking a `.log` file (file association) works as before – it is just a
FILE. Help → About points to `rustrace.exe --help` as well.

Examples:

```
rustrace C:\Windows\CCM\Logs\AppEnforce.log --search 0x87D00324 --filter
rustrace AppEnforce.log --line 1500
rustrace AppEnforce.log --errors --from "2026-10-27 08:00" --to "2026-10-27 09:30"
rustrace --merge AppDiscovery.log AppEnforce.log --search "fail.*0x8" --regex
rustrace --lang de AppEnforce.log
```

## Keyboard shortcuts

| Shortcut | Action |
|---|---|
| Ctrl+O | Open file (several selected files: one view sorted by time) |
| F5 / Ctrl+F5 | Refresh / reload the whole file |
| Ctrl+P | Pause / resume automatic refresh |
| Ctrl+F | Search |
| Enter, F3 / Shift+F3 | Next / previous match |
| Ctrl+L | Filter on/off |
| Ctrl+H | Highlight rules |
| Ctrl+E | Error code lookup |
| Ctrl+Shift+S | Export highlighted rows with context |
| Ctrl+Shift+E | Export table (CSV / Excel) |
| Ctrl+F2 | Add / remove a bookmark on the selected row |
| F2 / Shift+F2 | Next / previous bookmark |
| ↑ ↓ PgUp PgDn Home End | Move in the table (End / the last row resumes a paused "Follow end") |
| Esc | Clear the selection (resumes a paused "Follow end") |
| Shift + mouse wheel | Scroll the table horizontally (when the columns do not fit; also the scroll bar below the table) |
| Shift + ↑ ↓ PgUp PgDn Home End, Shift + click | Extend the selection |
| Ctrl + click / Ctrl+A | Add / remove a row / select all rows shown |
| Ctrl+C | Copy the messages of the selected rows |
| Ctrl+Shift+C | Copy the selected rows as a table (tab-separated) |
| Ctrl + mouse wheel | Over the log: log font size; elsewhere: zoom the whole window |
| Ctrl+plus / Ctrl+minus / Ctrl+0 | Zoom the whole window in / out / reset |

## Source layout

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

## License

RusTrace is free and unencumbered software released into the **public domain** under
[the Unlicense](LICENSE): anyone may copy, modify, publish, use, compile, sell or distribute it,
in source or binary form, for any purpose, without any conditions. It is provided "as is",
without warranty of any kind.

The third-party Rust libraries compiled into the program (egui/eframe, regex, serde, …) keep their
own licenses (mostly MIT / Apache-2.0).
