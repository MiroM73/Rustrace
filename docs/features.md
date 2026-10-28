# Features

Everything RusTrace can do, area by area.

[← Back to README](../README.md)

**Contents:** [Log formats](#log-formats) · [Several logs in one view, sorted by time](#several-logs-in-one-view-sorted-by-time) · [Log sets](#log-sets) · [Details pane: message or raw line](#details-pane-message-or-raw-line) · [Automatic refresh](#automatic-refresh) · [Memory](#memory) · [Color highlighting](#color-highlighting) · [Themes](#themes) · [Zoom](#zoom) · [Error codes](#error-codes) · [Search](#search) · [Position in the log](#position-in-the-log) · [Overview strip](#overview-strip) · [Δ time column](#δ-time-column) · [Selecting several rows](#selecting-several-rows) · [Bookmarks](#bookmarks) · [Start-up window and About](#start-up-window-and-about) · [Updates](#updates) · [Default app for .log / .lo_ files](#default-app-for-log--lo_-files) · [Settings](#settings) · export of rows and of the table: [Export](export.md)

## Log formats

- CMTrace / ConfigMgr format `<![LOG[...]LOG]!><time=... type=...>` including multi-line messages
- the older SMS format `message $$<component><date time><thread=...>`
- plain text (severity from the words error / fail / warn – can be turned off)
- **any other layout described by a log definition** – built in: **IIS web logs** (W3C),
  **Exchange Transport Connectivity logs**, **dism.log**, **CBS.log**; your own in `logdef\*.def` (see [Log definitions](log-definitions.md))
- encodings UTF-8, UTF-16 LE/BE (by BOM), ANSI (Windows-1250)

## Several logs in one view, sorted by time

*like CMTrace "Merge selected files"*

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

## Log sets

The logs of one troubleshooting scenario in one step – details in [Log sets](log-sets.md).

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

## Details pane: message or raw line

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

## Automatic refresh

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

## Memory

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

## Color highlighting

*Highlighting → Highlight rules, Ctrl+H*

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

## Themes

- View → Theme: **System** (follows Windows, also when it changes while running), **Light**, **Dark**
- quick toggle button in the toolbar (🖥 → ☀ → 🌙)
- every highlight rule and the error / warning / search colors have **separate colors for the light and
  the dark theme**; the editors have a "Colors for theme: Light | Dark" switch with a preview on the
  matching background and a "Copy colors from … theme" button
- rules from older versions or imported files get their light colors for the dark theme as a start

## Zoom

- **Ctrl + mouse wheel over the log text** (the table or the message in the details pane) changes
  the font size of the log only (6 – 48 pt, also View → Font size)
- **Ctrl + mouse wheel anywhere else** (menu, toolbar, status bar, …) zooms the whole user
  interface (50 – 300 %); Ctrl+plus / Ctrl+minus / Ctrl+0 zoom the whole window from the keyboard
- both the log font size and the window zoom are remembered

## Error codes

*Tools → Error code lookup, Ctrl+E*

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
  the Win32 / SSPI codes typical for IIS logs (`1`, `50`, `64`, `121`, `995`, `1229`, `1236`,
  `1314`, `1326`, `1330`, `1331`, `1385`, `1909`, `2148074252`, `2148074254`) have own texts worded
  for IIS (e.g. `64` = the client closed the connection, usually harmless for BITS downloads), on
  every OS; on Windows the system message is shown below them
  hovering a status / substatus / Win32 status cell in the table shows the same explanation as a
  tooltip. Codes without an own description show their class (e.g. "4xx – client error")
- the lookup window also accepts HTTP statuses: `404.3`, `HTTP 500`, `http 403.14` show only the
  HTTP meaning; a **plain number from 100 to 599** (e.g. `404`) is shown both as an error code
  (Win32 404) and below it as an HTTP status (enter `HTTP 404` for the HTTP meaning only); other
  numbers (`1603`, `0x80070005` …) work as before

## Search

- text or regex, Aa = match case; the number of matches (or an invalid-regex warning) and the ✖ clear
  button are shown right after the search field
- **search as you type**: from 3 characters on the search runs while typing and the first matching
  row (from the top of the table) is selected and scrolled to; a shorter text is searched with
  Enter / F3 (the field then shows "Enter = search (automatically from 3 characters)"). Enter
  pressed right after typing only confirms the search (adds it to the history) and keeps the
  first match selected; every further Enter / F3 moves to the next match
- **search history** (🕘 next to the search field): the last 30 search texts, newest first, kept
  between sessions; a text is remembered when you search with Enter / F3 / the arrows or leave the
  field; picking one searches it again; "Clear search history" empties the list
- searches the message, the component and the extra columns of a log definition; in a matching
  row the found text is highlighted in **every** cell, not only in the message (e.g. a client IP in
  the *Client IP* column of an IIS log), and in the column values of the details pane
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
  - numbers: `>10000`, `>=500`, `<100`, `<=3` and inclusive ranges `500..599` (`500..`, `..10`
    leave a bound open) – e.g. `>=10000` in *Time (ms)* of an IIS log = requests that took 10 s
    or more; integers and decimals (`1.5`), combinable with the rest (`>=500|=401`,
    `!500..599`); a cell that is not a number (empty, `-`) never matches a number condition.
    The tooltip of a filter field lists the syntax
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

## Position in the log

The status bar shows where the selected row is among the rows shown in
the table: `📍 Row 25 / 100 (25 %)`; with a filter active the numbers count only the visible rows,
the tooltip shows the line number in the file.


## Overview strip

A narrow strip right of the table's vertical scroll bar shows where the
errors, warnings, search matches and bookmarks are in the whole log (the rows shown in the
table, top to bottom), like the markers in VS Code or Notepad++

- colors of the current theme (error / warning / search / bookmark colors); when several fall
  into one pixel: bookmark > search match > error > warning
- the part of the log in view is drawn as a translucent rectangle; a **click or drag** scrolls
  there, the tooltip shows the row, its line in the file and its time
- View → "Overview strip next to the scroll bar" turns it off and chooses the marks shown
- the marks are computed once after a search / filter change (after a refresh only for the new
  rows) and kept in at most 4096 buckets, so drawing costs a few hundred rectangles per frame

## Δ time column

To find where an installation or update "hangs", the gaps between entries
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

## Selecting several rows

Like the lists in Windows Explorer

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

## Bookmarks

Mark interesting rows ("download started", "install failed") and jump back to them

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


## Start-up window and About

Right after the start a small **start-up window** (the RusTrace icon, name and version, "Starting…"
and a rotating ring of dots) shows that the program is starting, until the main window appears –
creating the graphics context can take a moment, especially on the first start after boot. It
follows the theme from the settings (or the Windows theme).

The window title shows the version in use, e.g. `RusTrace 0.6.0 – ccmexec.log`. **Help → About
RusTrace** shows the version, author, license, links to the project page, new versions, the
issue tracker and the guides, the build (platform) and the settings file; "Copy version information" puts e.g.
`RusTrace 0.6.0 (windows x86_64, release)` on the clipboard for a bug report.

## Updates

*Help → Check for updates…* – RusTrace asks GitHub for the published releases and shows
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


## Default app for `.log` / `.lo_` files

*Windows, Help → Set as default app for .log / .lo_ files…* – registers RusTrace for your user account (no administrator rights, nothing outside
`HKEY_CURRENT_USER`) and shows which program opens each extension now. Windows does not let a
program replace an app **you chose yourself** (e.g. Notepad for `.log`); the window then offers
"Open the Windows default apps settings", which opens the RusTrace page of Settings → Apps →
Default apps, where one click switches the extension to RusTrace (or right-click a `.log` file →
Open with → Choose another app → RusTrace → Always). "Remove the registration" deletes everything
RusTrace wrote. The registration stores the path of `rustrace.exe` – after moving the program,
set it as the default app again (the window warns when another copy is registered).


## Settings

Settings and rules are saved automatically to `%APPDATA%\RusTrace\config.json`
(Linux: `~/.config/RusTrace/config.json`).
