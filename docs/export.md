# Export

Export of highlighted rows with their context, and the table export to CSV / Excel / JSON Lines / HTML.

[← Back to README](../README.md)

**Contents:** [Export highlighted rows with context](#export-highlighted-rows-with-context) · [Export table – CSV / Excel](#export-table--csv--excel)

## Export highlighted rows with context

*File → Export highlighted rows with context…, Ctrl+Shift+S*

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

## Export table – CSV / Excel

*File → Export table (CSV / Excel)…, Ctrl+Shift+E; row context menu → Export selected rows to CSV / Excel…*

The **parsed** log as a real table for Excel / Power BI,
tickets or colleagues without RusTrace: every column is a spreadsheet column.

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
- **JSON Lines** (`.jsonl`) – one JSON object per row for scripts, Log Analytics or Splunk: keys
  `line`, `file`, `datetime` (always ISO), `message`, `component`, `thread`, the extra column names
  (`sc-status` …), `severity`, `rule`, `note`, `codes`, `raw`, `block`; empty cells are left out;
  Δ and numbers are JSON numbers
- **HTML report** – one self-contained page (no scripts, no external files) with the summary
  lines, a sticky header and the table, optionally in the colors of the table; multi-line
  messages are collapsible – for e-mails or tickets where Excel is not wanted
- **Presets**: type a name and **Save** to keep the current rows, format, options and column choice
  (e.g. "Errors for a ticket", "Full CSV for Power BI"); choose one from the list next time, 🗑
  deletes the one with the typed name
- **Open the file after the export** (checkbox); after an export the window shows its path with
  **Open**, **Show in folder** (Explorer with the file selected) and **Copy path**
- **Copy as CSV / Copy for Excel** in the row context menu: the selected rows with the chosen
  columns to the clipboard (comma-separated, or tab-separated to paste into Excel with Ctrl+V),
  without a file
- **command line**: `rustrace FILE... --export out.xlsx` (or `.csv`, `.jsonl`, `.html`) writes the
  export of the rows the other options select – `--errors` / `--warnings`, `--search` with
  `--filter`, `--from` / `--to` – and exits without a window, e.g.
  `rustrace AppEnforce.log --errors --from "2026-10-27 08:00" --export errors.xlsx`; the columns and
  options are the saved ones of Export table (the table columns of the log type when none were
  saved). Messages go to the console it was started from (no message box – fine for scheduled
  tasks); exit code 0 = written, 1 = export error, 2 = invalid command line
- **Safe for Excel** (default on): log text starting with `=`, `+`, `-`, `@` (or a tab / CR) gets a
  leading `'`, so a spreadsheet never runs it as a formula (CSV / formula injection)
- runs in a background thread straight from the parsed entries (no copy of the log): the whole 400 MB
  test log (1.9 million rows) is written in about 3 s; progress and **Cancel** in the window and in
  the status bar, a cancelled export leaves no file. New rows read meanwhile are added after it.
