# RusTrace – internals of the features

Notes for Claude: how every feature works inside (names of types, functions, settings,
measurements). Linked from `CLAUDE.md`; read the section of a feature before changing it.
User-facing descriptions are in `README.md`.

## Formats
- Formats: CMTrace `<![LOG[..]LOG]!><time="" date="" component="" type="" thread="">` incl. multi-line,
  old SMS format `msg $$<comp><date time><thread=..>`, plain text. Severity from `type` (1/2/3) or
  optional keywords error/fail/warn.

## Log definitions
- **Log definitions** (`logdef\*.def`, `src/logdef.rs`): regex with named groups or W3C (`#Fields:`)
  describe other layouts (w3c `separator`: space = IIS with `-` = empty, other = CSV-style values
  with quotes, `-` kept – Exchange); built-in `iisweb.log.def`, `exchconnect.log.def`,
  `dism.log.def`, `cbs.log.def`, `msi.log.def`, external files
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
- MSI verbose logs (#40, `msi.log.def`): `header.date` (regex, group `date` / first group) +
  `header.date.format` (default `DEFAULT_DATE_FORMATS`, first fitting wins) → `DefState::date`,
  set in `DefState::parse` before the line is parsed (resets `last_dt` = new session, no
  roll-over across it). `DefState::header_datetime`: the datetime role text = time only, parsed
  as `YYYY-MM-DD <time>` with `datetime.format`; empty → `last_dt`; earlier than `last_dt` by
  more than 12 h → + days, `date` moves on. Without a header line yet the time text stays as it
  is. `role.severity` is a list (`LogDef::severity_fields`, `Mapping::severity: Vec`): the values
  of the fields present in the line are joined; none present → keywords. The MSI pattern has one
  alternative per line kind (`MSI (s|c) (pid:tid) [time]:`, `Action start/ended H:MM:SS:`, the
  header, any other text) with own message / time groups joined by the roles; every line is an
  entry, so continuation lines (*Error 25150.*) get their own severity. `errcodes::find_codes`
  also reads `error status: N`. MSI message numbers in `Error NNNN.` are **not** looked up: they
  are Windows Installer message ids (1722 = custom action failed), not Win32 codes (1722 = RPC
  server unavailable) – the existing lookup would explain them wrongly.
- `columns` order (#41): role / field names; `*` = every column not named in the list, in the
  field order (`LogDef::mapping`; kinds of the named columns collected first so `*` skips them).
  Without `*` unlisted fields are hidden (still extra columns, searchable). The built-in IIS
  definition uses `… message *` (status columns first, no `#Fields:` field lost); a later
  `#Fields:` header (merge) appends its new columns at the end.
- Status columns (#28): optional keys `http_status` / `http_substatus` / `win32_status` →
  `LogDef::status` → `Schema::status` (`StatusFields`, column **names**); the UI resolves them to
  extra column indexes by name (`StatusFields::resolve` → `StatusColumns`, `App::status_columns`), so a
  later `#Fields:` header or a merged view (`LogSet::sync` takes the status fields of the first log
  that has them; extra columns are united by name) needs no remapping. A missing column = `None`.

## Reading
- Encodings: UTF-8 (BOM or not), UTF-16 LE/BE (BOM), ANSI Windows-1250 fallback.
- Incremental reading in a **background thread** in 8 MB chunks with progress + cancel; truncation /
  rotation detection; provisional last line. 400 MB / 1.8 M lines ≈ 3.7 s in release.

## Merged logs
- **Different logs merged by time** (#21, `src/merge.rs`): `LogSet` = one or more `LogSource`s
  (App holds `source: Option<LogSet>`, `paths`, `cur_path` = first path). One log: results of
  `LogSource` passed through unchanged (no reordering). Several: every source polled, new final
  entries merged with `interleave` (k-way heap on (merge key, log index), order inside a log
  kept); merge key = timestamp, undated = key of the previous entry of the same log
  (`last_key` per log, `KEY_MIN` at the start). Results are `ReadResult` (provisional entries
  as a `LogStore` with file ids); `Appended.from_key` = batch has a key older than `newest` →
  `merge::apply_appended` / `merge_tail` (split_off after the last entry dated ≤ key, re-interleave
  per log, `file_sources`). Files of the view: `map[log][local id]` → global id, `file_sources`;
  columns: `sync` unites the schemas (extra columns by name, `extra_map`); `LogStore::copy_entry`
  re-interns + maps extra columns. A source that was loaded and returns `Reset` (truncated /
  replaced) → the whole set is read again; a source that could not be read → `errors()` (status
  bar), others shown. Provisional entries cached per log (`provisional`), merged at the end.
  UI: Open with several files / drop several / command line `a.log b.log`, File → Add log to
  view…, File → Merged logs (✖ = `remove_log`, reopens the rest), File column color mark per log
  (`cfg.color_files`, `App::log_color`), status bar "📚 N logs merged" with a tooltip per log,
  `format.mixed` when the formats differ, `SessionState::extra_paths` for the update restart.
  Selection kept by (file, line) when older rows are merged in before it. Progress counters
  accumulate over the logs (`LogSource::progress_base`). Measured: 400 MB CMTrace + 100 MB CBS +
  100 MB IIS (3.7 M rows): separately ~9.2 s, merged ~9.3 s (`bench_merge`). First merge keeps
  the per-log stores and the merged store at once (temporary peak).

## Log sets
- **Log sets** (#27, `src/logset.rs` pure + tested, window `app/dialogs/logsets.rs`): `.set` files
  (`name` / `name.<lang>`, `description` / `description.<lang>`, repeated `file`; unknown keys and
  lines without `=` ignored, no `file` = error). Translations live **in the set file** (not in
  `.lang` keys) so own sets carry their own; `LogSetDef::name(code)` tries the code, its base
  language, then the default. Loading like the log definitions (`logset::load_from`, same
  `PRIO_*`): built-in (`BUILTIN`, `include_str!` of `logsets/*.set`) replaced by a same-named file in
  `logsets\` next to the exe; own = `%APPDATA%\RusTrace\logsets` (`user_set_dir`), both offered; no
  update notice. Loaded every time the window opens (`App::open_log_sets`), so edits need no reload.
  `LogSetDef::key` = `predefined/<id>` | `own/<id>` → `cfg.last_log_set`.
- Variables: `Vars` (windir, programdata, ccmlogs, systemdrive, `env` = other `%NAME%` from the
  environment); `local_vars(ccm)` – `%CCMLOGS%` from `HKLM\SOFTWARE\Microsoft\CCM\Logging\@Global`
  `LogDirectory` (`fileassoc::hklm_string`, Windows) else `%WINDIR%\CCM\Logs`; `resolve` →
  `Err("%NAME%")` for an unknown variable, a lone `%` stays.
- Window: `App::log_sets: Option<LogSetDialog>` (taken out and put back each frame). Choosing a set
  → `start_set_check`: rows resolved on the UI thread, `logset::check_file` (metadata → `FileState`
  Found { size, modified ISO, rotated = `parser::rotated_parts` count when `cfg.merge_rotated` } /
  NotFound / AccessDenied / NotAFile / Error) for all rows in one `rustrace-logset` thread, result
  over a channel + repaint; a newer check replaces the receiver (the old result is dropped). Found
  rows are ticked; Open → `App::open_files(ticked found paths)` = the merged view of #21 (one path =
  a normal single log).
- Remote computer (#27 part 2): `logset::computer_name` (trim, leading `\\` dropped; whitespace,
  `\ / $ % * ? " < > |` rejected) → `remote_vars(pc)`: `%WINDIR%` = `\\pc\admin$` (follows the
  real Windows folder), `%CCMLOGS%` = `\\pc\admin$\CCM\Logs` (default only – no remote registry),
  `%PROGRAMDATA%` = `\\pc\c$\ProgramData`, `%SYSTEMDRIVE%` = `\\pc\c$`, no environment;
  `resolve` maps a drive path `D:\x` to `\\pc\d$\x`. Check thread: distinct `share_root`s of the
  paths → `check_share` (`read_dir` of `\\pc\share\`) → `CheckMsg::Shares`, then
  `CheckMsg::Files` (rows of a failed share get its state without touching the files).
  `LogSetDialog::poll_check`: while `shares_pending`, `REMOTE_TIMEOUT` (10 s) from `started`
  (`request_repaint_after` the rest) → rows `TimedOut`, receiver dropped (the thread may hang in
  the SMB call and ends later; its sends fail). `io_state` maps Windows codes first (only
  `cfg!(windows)`): 2/3 not found, 5/86/1219/1326/1385/1272 access denied, 67 `NoShare`,
  51/53/59/64/121/1203/1222/1231/1232/1234 `Unreachable`; banner text per state
  (`share_problem_text`). `cfg.log_set_remote`, `cfg.remote_computers` (newest first, max
  `REMOTE_COMPUTERS_MAX` 15, `Config::push_computer` on Open). Measured: unknown name ≈ 2.5 s →
  *cannot be reached*; `10.255.255.1` → timeout after 10 s; `\\localhost\c$` / `admin$` work.
- UNC paths and the reader: `LogSource::poll` opens the file again for every read
  (`File::open` + `metadata()` on the handle – the size comes from the server, not from the SMB
  client's metadata cache of `std::fs::metadata(path)`), reads from the stored offset; rotation
  detection compares the first 4 KB and the size; `rotated_parts` lists the folder over SMB. Std's
  default share mode (read | write | delete) does not block the ConfigMgr client from appending /
  renaming. Tested over `\\localhost\c$`: appends every 250 ms and a rename to `.lo_` while
  watching – every entry once, the File column shows both parts. Known limit: the raw line of the
  details pane (`parser::read_raw`) is read on the UI thread – on a remote log that went offline
  it can wait for the SMB timeout.

## Rotated logs
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

## Memory limits
- **Memory limits for long-watched logs** (`App::enforce_memory_limits`, after every poll):
  text in memory ≥ temp file threshold → `LogStore::move_text_to_temp_file`; text >
  `cfg.max_log_mb` (default 1000, `cfg.max_log_enabled`) → `LogStore::drop_front(k)` down to ¾ of
  the limit (never provisional rows), interner rebuilt; App shifts `filtered`, `is_match`,
  `selected`, `committed_len` by k, bumps `data_gen`, counts `dropped` (✂ in the status bar, reset
  by a full read). Known limit: the first read of a huge file still loads it whole before trimming.

## Raw line
- **Raw line in the details pane** (#29): `cfg.details_view` (`DetailsView::Message | Raw`),
  toggle in the details header + View menu. `LogEntry::raw` = byte range of the entry in its file
  (start of the first line .. after the line break of the last; set by `parse_line` /
  `parse_def_line`, extended for multi-line CMTrace and `unmatched = append`, line positions from
  the raw bytes by `line_ends` – memchr for UTF-8/ANSI, 2-byte units for UTF-16), packed into
  `Rec::raw` (40-bit offset, 24-bit length, 0 = unknown). `SourceFile::encoding` per file;
  `parser::read_raw` reads + decodes the range; `App::raw_cache` (entry, `data_gen`). Measured
  400 MB: load 3.98 → 4.07 s, 223 → 237 MB.

## Compact storage
- **Compact storage** (`src/store.rs`, `LogStore`): the parser still produces `LogEntry` (Strings)
  per line, but they go straight into a `LogStore` – 40-byte `Rec` (message start in the text arena,
  `i64` ms timestamp or tagged interned raw text, raw byte range, line, interned component/thread, severity), one
  text arena in entry order (message `i` ends where `i + 1` starts), interner (id 0 = ""), extra
  columns column-major as interned ids. `PollResult` carries a `LogStore`; appends are merged with
  `LogStore::append` (re-interning). The parser reserves capacity after the first chunk (estimate).
  Text arena = `String` or a **memory-mapped temp file** (`memmap2`, `FILE_ATTRIBUTE_TEMPORARY` +
  `FILE_FLAG_DELETE_ON_CLOSE`, share mode 0, grows by doubling from 64 MB, falls back to memory on
  error) for reads ≥ `cfg.temp_file_threshold_mb` (default 100, `cfg.temp_file_enabled`). Never map
  the original log (blocks the writer from truncating / rotating it on Windows).

## Renderer and refresh frames
- **Renderer** (`main.rs`, issue #6): **OpenGL (glow) by default** – with wgpu (DX12 and Vulkan
  alike) a thread of the AMD driver (`amdxc64.dll` / `amdvlk64.dll`) spun at 100 % of a core while
  the window was idle (it only stopped after ~2.5 s of continuous rendering; present mode / frame
  latency did not help). Glow: idle ~0 %, empty window 284 MB private vs 323 MB DX12. If glow fails
  before the app is created (error **or panic**, caught with `catch_unwind`; e.g. RDP / VM with GL
  1.1) → wgpu on the reused event loop. `RUSTRACE_RENDERER=wgpu|glow` forces one (no fallback).
  wgpu fallback (`prefer_dx12`): instance limited to DX12 when a non-CPU DX12 adapter exists,
  unless `WGPU_BACKEND` is set (default wgpu loads Vulkan + DX12 + GL drivers: 516 MB vs 325 MB).
  Idle CPU is measured from `Process.TotalProcessorTime` over a few seconds; per-thread start
  address (`NtQueryInformationThread` class 9) → module found the driver thread.
- Auto refresh frames: the reader thread requests a repaint only when the result is not
  `Unchanged`; `last_refresh` = start of the read (otherwise the delayed pick-up would double the
  interval); the status-bar separator belongs to the progress (a widget appearing for a moment
  costs an extra frame, egui `theme/mod.rs` repaint for unknown widget ids).
- Measured (400 MB CMTrace, GUI private bytes): v0.2.0 1231 MB → 421 MB (temp file) / 582 MB (memory);
  benchmark without GUI: 695 → 76 / 239 MB, load same, search 157 → 138 ms.

## UI basics
- Auto refresh: interval in ms / s / min, pause/resume (Ctrl+P), presets, "follow end".
- Follow end paused (#37, `app/follow.rs`): `cfg.auto_scroll` = the user's setting (checkbox,
  `--tail` / `--no-follow`, saved); `App::follow: Follow` = runtime pause (not saved, but handed
  over in `SessionState::follow_paused` after an update). `following()` = setting && !paused is
  what `stick_to_bottom`, appended rows (`check_loading`), the full reload, the memory-limit drop
  and the after-filter scrolling use. Pausing: every selection path calls
  `follow_row_selected(pos)` / `follow_entry_selected(i)` (`select_row` – keyboard, F3, F2,
  search as you type, bookmarks list; `extend_selection`; `click_row`; right click; overview strip;
  `--line`; a hidden row pauses) – a row above the last one pauses, the last row resumes.
  Resuming: `clear_selection` (Esc in `input.rs` unless a popup is open, Ctrl+click on the only
  selected row, a click below the last drawn row in `table.rs`), the last row, scrolling the table
  to the bottom with the wheel / a drag (`table.rs`, only when the content is taller than the
  view), the status bar button, the checkbox (`follow_setting_changed`, ticking scrolls to the
  end), opening / reloading a log, a selected row that disappears. `refilter_from(0)` scrolls to
  the selected row when not following, so filter changes keep it in view. Paused is shown by the
  orange checkbox text (same width – the toolbar must not move under the pointer), "(paused)" in
  the View menu and the status bar button.
- Highlight rules: text or regex, case, scope whole row / text only, also-component, order = priority,
  JSON import/export, **separate colors for light and dark theme**; severity + search colors per theme.
  Rules window (`dialogs/rules.rs`, #48): name / pattern fields have fixed widths (`NAME_W`,
  `PATTERN_W`, full text in the tooltip), default width `default_window_width` = `WINDOW_W` (fits
  the table in all four languages) limited by the main window; `ScrollStyle::solid()` so the
  horizontal bar is visible when needed (floating bars only appear on hover); the horizontal offset
  is reset to 0 when the window opens (`rules_was_open`).
- Themes: system / light / dark (View menu + quick toggle button in the toolbar).
- Zoom: Ctrl + mouse wheel over the log text (table, details message) → `cfg.font_size` (6–48);
  elsewhere → whole-UI `zoom_factor` (0.5–3.0, saved as `cfg.ui_zoom`, also Ctrl+plus/minus/0).
  Hover is tracked per frame in `hover_log`, applied in `handle_zoom` after the panels are drawn.
- Search: text/regex, next/prev, filter mode, info/warning/error toggles; search as you type from 3
  characters (`App::search_active`), the first match gets selected.

## Filtering
- **Filtering** (`src/filter.rs`, pure; UI part `app/filter.rs`): `FilterSpec` (search regex,
  filter mode, severity, time range, `CellFilter` per column, file labels) → `eval` per entry,
  `run` (range, sequential) / `run_parallel` (64 Ki-entry chunks, scoped threads = cores − 1 so
  the UI thread keeps a core, cancel flag per chunk; same result as `run`, tested).
  `App::refilter_from(start)`: a full re-filter (`start == 0`) of more than
  `SYNC_FILTER_MAX_ENTRIES` (100 000) entries runs in a background thread (#16, `FilterJob`),
  everything else on the UI thread. `entries: Arc<LogStore>` is shared with the job; every
  in-place change goes through `App::entries_mut` (cancels + joins the job and the cancelled
  `stale_filter_jobs`, sets `filter_restart` so the next `refilter_from` starts from 0); a new
  store (open / Reset) is just a new `Arc` after `cancel_filter_job`. While a job runs
  `check_loading` leaves the read result waiting (so appends never stop a search) and the old
  `filtered` / `is_match` stay displayed (not cleared on parameter changes, only for new data).
  Actions that need the result wait in `AfterFilter` (`select_first_match`,
  `scroll_to_end_after_filter`, `scroll_to_selected_after_filter`). Status bar: spinner +
  "Searching…" after `FILTER_HINT_DELAY` (200 ms). Measured (400 MB, frame time while typing,
  temporary instrumentation): v0.7.0 ~160 ms per character, now max ~7 ms; 8 workers on 8 cores
  still gave one 34 ms frame, hence cores − 1.
- `is_match` is `MatchBits` (#18): bit set that keeps its count of ones (`push`, `truncate`
  O(removed), `drain_front` for `drop_front`); `is_match.count()` is the match count – no
  recount per auto refresh; `debug_assert` against `recount()` after every filter.
- Date/time column filter (#17, `colfilter::DateFilter`): `DateLayout` = date pattern + time as
  slots (literal / chronological digit `YYYYMMDDHHMMSSmmm`). Every alternative is aligned at
  every offset of the layout; the fixed digits of each fitting offset must be a chronological
  prefix → inclusive ms range (lowest = zeros with month/day ≥ 1, invalid → empty; highest =
  nines clamped per field); union of ranges, `!` negates, `=` only at offset 0 with full length.
  Otherwise (e.g. `:30`, `23.10` without the year, non-ASCII) the displayed text is written
  into a stack buffer (`with_displayed`) and matched like before; entries without a parsed
  timestamp match their text. Tests compare it with the plain text filter for many inputs.
  `App::display_dt` = `DateLayout::format_iso`. Measured (`bench_filter`, 400 MB, 1.9 M rows,
  one day): text per row 1.20 s → ranges 40 ms; `:30` on the stack 175 ms; search sequential
  143 ms → parallel 68 ms. A match that would not be
  visible in the Message cell (later line, past `MAX_CELL_CHARS`, past the column width measured
  with `layout_no_wrap`) → `App::message_cell_text` shows `excerpt::around` (context before the
  match, "…"). Details pane: `TextEdit` with a layouter using `text_spans` (rules + search), scrolls
  to the first match once per (row, `search_gen`) (`details_scrolled`).

## Table
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
  `!` negation, case-insensitive. Numeric alternatives (#42): `number_range` turns `>n`, `>=n`,
  `<n`, `<=n`, `a..b` (either bound may be empty) into a `(Bound<f64>, Bound<f64>)`; text
  alternatives stay in one regex (`re: Option<Regex>`), `matches` = regex hit or the cell parsed by
  `number` (optional sign, digits, one `.`; no exponents / hex / thousands separators) lies in a
  range, then `!` negates. A text like `>abc` or `10.20.7.42` stays a text alternative. The
  date/time column uses `parse_text` (no numeric alternatives, its own time ranges). Cleared when
  the columns change (other log type).
- Error code lookup (Ctrl+E) + automatic explanation in the details pane; descriptions from language
  files (`code.0xXXXXXXXX`) + Windows `FormatMessageW` (also wininet/winhttp).
- HTTP / IIS status codes (#28, `src/httpstatus.rs`): `describe(status, substatus)` from the language
  files – `http.<status>`, else the class `http.<n>xx` (`exact` = false), else `http.unknown`;
  substatus `http.<status>.<sub>` (substatus 0 counts only when it has a key, e.g. `http.500.0`, so
  `404 0` shows as `404`). `from_cells` / `win32_from_cell` parse the cell values. Details pane:
  `App::selected_codes` → `RowCodes` (message codes, `http`, `win32` when ≠ 0; a message code equal to
  the Win32 status is dropped), cached in `code_cache` per entry; rows with 🔎 buttons open the lookup
  (`open_lookup_text("HTTP 404.3")`). Table: `App::status_tip` only when the status cell is hovered
  (`cell.hovered()`), so the rows cost nothing extra. Lookup window: `httpstatus::lookup_input` –
  `HTTP …` or a dotted `404.3` = HTTP only, a plain number 100–599 = the error code first and then
  "also an HTTP status", anything else = the error code as before (`errcodes::parse_code`).
  Severity of IIS rows already comes from `sc-status` via `role.severity` + `severity.error/warning`.

## Overview strip
- **Overview strip** (#24, `src/overview.rs` pure + `app/strip.rs`): `Overview` = highest mark per
  bucket of `per` displayed rows (power of two, ≤ 4096 buckets, pairs joined when it grows);
  `update(from, len, mark)` recomputes only from the bucket of `from` (appends: the end).
  `App::overview_dirty` = first changed displayed row (`overview_changed_from`: `refilter_here`
  start position, full job result / `drop_front` / bookmark or mark-setting changes → 0).
  Marks: bookmark 4 > search 3 > error 2 > warning 1 (rules not shown – a regex per row would be
  too slow). Drawn right of the table (the table `view` is narrower by `STRIP_WIDTH` + 2) from
  the header bottom; `runs(px)` per physical pixel; viewport = rows drawn by the table body;
  click / drag → `scroll_to`. `cfg.overview` (`OverviewSettings`).

## Delta column
- **Δ time column** (#23, `app/delta.rs`): `ColumnKind::Delta` inserted by `App::columns()` after
  the date/time column when `cfg.delta_column`; `cfg.delta_mode` (`DeltaMode::Visible` = previous
  row of `filtered` before the entry, also for hidden rows / `Log` = previous entry /
  `Reference` = `App::time_ref` (ms, file, line)), at most `MAX_LOOK_BACK` rows back for a dated
  one; `delta_cell` → text + long gap (`cfg.delta_threshold_s`, palette pair `gap`); never
  filtered (`passes_columns` → true, no filter field); `App::cell(idx, kind)` for copies;
  readable export `ReadableRow::delta` after the date/time; header context menu (header labels
  are not selectable, otherwise they take the click).

## Selection
- **Selection of several rows** (#25, `app/selection.rs`): `selected` = cursor row, `sel: MatchBits`
  (entry indexes; `MatchBits::set / first_one / last_one / ones`), `sel_anchor` for Shift ranges.
  Every single selection goes through `select_only` (also `select_row`); `drop_front` →
  `selection_drop_front`; merged older rows before the selection keep only the cursor row.
  Click modifiers in `click_row`, Shift + keys in `extend_selection`, Ctrl+A, Ctrl+C messages,
  Ctrl+Shift+C table (`columns()` + header), original lines via `parser::read_raw_many` in a
  thread (`copy_job`), `export_selected` (export dialog, reset when it closes). Details pane =
  `details_row()` (first selected row); summary with `timefmt::format_duration`.

## Bookmarks
- **Bookmarks** (#22, `app/bookmarks.rs`, window `dialogs/bookmarks.rs`): `App::bookmarks`
  (file id, line) → note is the truth; `bookmark_rows` = sorted entry indexes, kept like
  `filtered` (`find_bookmarks_from(changed)` after appends, `bookmarks_drop_front`,
  `bookmarks_after_reset` remaps file ids by path via `remap_files` – `open_files` stashes the old
  files in `bookmark_files`, so adding a log / Ctrl+F5 keeps them). Ctrl+F2 / F2 / Shift+F2
  (`app/input.rs`: Shift variants are consumed before the plain key – egui `consume_key` ignores
  extra Shift / Alt, #46),
  filter `FilterSpec::bookmarks` (keys), export criterion `cfg.export_bookmarks`, palette pair
  `bookmark` (line cell), `SessionState::bookmarks` for the update restart; not saved per file.

## Export
- Export highlighted rows with context (Ctrl+Shift+S): lines before/after, criteria (rules, errors,
  warnings, search), overlapping ranges merged (no duplicates), formats original lines / readable
  table, separators, **only records since date/time** (with presets relative to newest log record),
  **only the last N blocks**, **rows visible in the table = highlighted rows**
  (`export_visible_only`: hits = `filtered`, the rules/errors/warnings/search criteria are disabled;
  context always from the whole file – the owner's expectation, an earlier variant that also took
  context only from visible rows was rejected). Order: hits → time filter → merge → clip older
  context → last N.
- Export… is disabled only by a running export or a **full read** (`Loading::full`, set in
  `start_poll` from `!LogSet::has_read()` – first load, Ctrl+F5, a new source after a cancel),
  shown with a spinner and "Reading the log…" (`export.reading`). Refresh reads do not block it
  (#38): with auto refresh on `loading` is `Some` almost all the time (an `Unchanged` read requests
  no repaint, its result is taken over on the next scheduled frame, which starts the next read),
  and `start_export` copies everything it needs (blocks, lines, file paths) before the thread
  starts. `LogSet::loaded` is set for a single log too (`poll_single`).

## Table export
- **Table export** (#32, `src/tabexport/` pure + tested, `app/tabexport.rs`, window
  `dialogs/tabexport.rs`, Ctrl+Shift+E – consumed before Ctrl+E, #46). `Scope` → `Rows`
  (`All(n)` / `List` = `filtered`, `sel.ones()`, `bookmark_rows` / `Blocks` = `export_plan()` with
  `export_selected` taken out for the call); `Col` (table columns + `ADDITIONAL`: severity text,
  path, rule, note, codes, raw, block), keys `line`, `extra:<name>` … saved per log type
  (`cfg.table_export.columns[schema.name]`, `column_choice` = saved order, unknown keys dropped,
  new columns appended – table ones ticked). `Context` is moved into the `rustrace-tabexport`
  thread: `Arc<LogStore>` (no copy), file labels / paths / encodings, rules (light colors), notes,
  severity names, `DateLayout` or ISO, `DeltaFrom` (Visible → previous exported row, Log, Reference).
  While the thread holds the `Arc`, `check_loading` leaves read results waiting and
  `enforce_memory_limits` returns (like the background filter; `entries_mut` needs a unique
  `Arc`). `CellMaker::row` → `Cell` (Text / Int / Seconds / Date{ms,text}) + `RowLook` (first
  whole-row rule, else severity palette); codes cached per code, raw lines via
  `parser::RawReader` (one open file per file id). `write_table` → `TableSink`; `Control` = done
  counter + cancel flag checked every 1024 rows (`Interrupted` → the partial file is removed).
  CSV (`tabexport/csv.rs`): BOM / UTF-16 LE, CRLF, `push_field` (RFC 4180, `\r\n` → `\n`,
  formula guard only for `Cell::Text`), decimal comma with `;`, `system_list_separator` =
  `GetLocaleInfoEx(LOCALE_SLIST)`. Measured (release, 400 MB CMTrace, 1.9 M rows, 5 columns):
  258 MB CSV in ~3 s.
- XLSX (#32 part 2, `tabexport/xlsx.rs`, own writer instead of `rust_xlsxwriter` – no new
  dependency, the binary grows by a few kB only): `zip.rs` streams every part through a
  `DeflateEncoder<Vec<u8>>` (fast level, drained to the file every 1 MB) and patches CRC / sizes in
  the local header afterwards (output must be `Seek`; no ZIP64 → error above 4 GB). Worksheets use
  inline strings (no shared-string table in memory), `<cols>` widths per `Col` before
  `<sheetData>`, frozen pane + `autoFilter` + hidden `_xlnm._FilterDatabase` defined name per
  sheet. Styles are collected while streaming (`Styles::get`: (kind General / Date numFmt 164 /
  Seconds 165 / Header bold, font color, fill) → xf index) and `styles.xml`, `workbook.xml`, the
  rels and `[Content_Types].xml` are written last (ZIP order does not matter). Dates = serial
  `ms / 86 400 000 + 25 569` (store ms are local time). Split at `max_rows` (header repeated, names
  `Log`, `Log (2)`; `sheet_name` sanitizes, `unique_name`), text over `MAX_CELL_CHARS` cut with
  `…` (`truncated` → status message), XML-invalid control characters dropped. `small_sheet` /
  `new_data_sheet` / `front` for the Summary / Statistics / per-file sheets (part 3).
  Checked with Excel 2016 via COM (`RUSTRACE_XLSX_OUT=<file> cargo test xlsx_sample -- --ignored`
  writes a sample): opens without repair, Double cells for numbers and dates, AutoFilter, frozen
  row, colors. Measured: 1.9 M rows → 2 sheets, 135 MB, ~8 s.
- Summary / Statistics / sheet per file (#32 part 3, `tabexport/stats.rs`): `write_xlsx` drives
  the workbook; `write_table_stats` feeds every written entry into `Stats` (counts, first / last
  timestamp, per component (errors, warnings), `find_codes` of error / warning messages, per hour
  `ts.div_euclid(3 600 000)`), `done_before` keeps the progress over several sheets. Sheet per file:
  the file ids in row order → `Rows::of_file` (`Rows::Numbered`, block numbers kept) →
  `XlsxWriter::new_data_sheet(label)` (names sanitized / unique). The small sheets are written
  after the data (they need the counts) with `front: Some(rank)` – the workbook lists them first
  (Summary 0, Statistics 1). Texts come translated from the UI thread (`SheetTexts`,
  `App::table_export_extras`: version, time, `format_name`, file paths, scope, search + filter
  mode, `show_sev`, column filters, time range).
- Part 4: `tabexport::Job` + `run` (format by `TableFormat`, removes the file on error / cancel) is
  shared by the window thread and `--export`. `jsonl.rs` (`Col::json_key`: extra columns by name,
  dates `ms_to_iso`, empty cells left out, `serde_json` string escaping), `html.rs` (inline CSS,
  `<details>` for multi-line cells, `RowLook` as `style`, no scripts). Presets:
  `Settings::presets: Vec<(name, Settings)>` (stored without their own presets, `apply_preset`
  keeps the list; `table_export_cols` reset after applying). `open_after` → `App::open_exported`
  (`open::that_detached`; Show in folder = `explorer.exe /select,"path"` via `raw_arg`),
  `table_export_last`. Context menu Copy as CSV / Copy for Excel: `copy_selection_as_table` →
  `CsvWriter` into a `Vec` in a `rustrace-copy` thread, result through the existing `copy_job`
  channel. `--export` (`src/batch.rs`, `cli::Options::export`, `NeedsFile`): `LogSet` read once
  (defs, merge_rotated from the config), `FilterSpec` from the view options (search + filter,
  severities, `parse_range_bound`), `FilterSpec::run`, columns = saved choice or table columns,
  rules compiled from the config; `attach_console` (`AttachConsole(ATTACH_PARENT_PROCESS)`) before
  printing, no message boxes; exit 0 / 1 / 2. Measured: 400 MB log, `--errors --export x.csv` →
  95 133 rows in ~5 s (mostly reading).

## Updates
- **Updates** (#15, #31, `src/update.rs`, `src/http.rs`): Help → Check for updates; GitHub API
  `releases?per_page=100` via WinHTTP FFI (system proxy / certs, follows redirects);
  `parse_releases` drops drafts / pre-releases / non-version tags, `Updates { latest, missed }`
  (`missed_releases` = newer than the current version, newest first; install always from
  `latest`); `is_newer` on major.minor.patch. Auto check `cfg.update_when` = start (default) /
  daily / never (`UpdateCheck::due` with `cfg.update_last_check`); the old bool `update_check`
  is read as `update_check_old` (never written) and migrated in `Config::load` (false → never)
  – old versions ignore `update_when`. Silent: window only for a newer, not
  `cfg.update_skipped` latest version; errors ignored; not after `--restore`. Window: one
  collapsible section per missed version (version – date, notes), the newest open;
  status-bar `⬆ <version>` button (`update_available: Option<Updates>`). Install: ZIP asset `*-windows-x64.zip` + its `.sha256`, own ZIP
  reader (stored / deflate via flate2, CRC check, no `..`), write probe first, running exe renamed to
  `rustrace.exe.old` (`update::cleanup_old` at start), files written next to it, then
  `rustrace.exe --restore <temp json>` (`SessionState`: path, format, selection, search, filters,
  time range, window pos/size × `zoom_factor`) and the old window closes. Restore: search/format at
  start, selection + column filters after the first read (`pending_restore`).
  `RUSTRACE_UPDATE_AS_VERSION=0.5.0` pretends an older version (testing the update);
  `cargo test fetch_latest_release -- --ignored` downloads + installs the real latest release into temp.
  The release notes of the GitHub release are shown in the window – write them for users.

## Command line
- **Command-line arguments** (#26, `src/cli.rs`, pure + tested; glue in `main.rs::command_line`
  and `app/cli.rs`): `rustrace [OPTIONS] [FILE]...`, hand-written parser (no dependency) →
  `cli::Options { files, view: ViewOptions, merge, lang, help, version, restore }`. Options
  before or after the files, `--opt value` / `--opt=value`, `--` ends options, last repeated
  option wins, anything else starting with `-` (or `/?`) is an option → unknown = error; a plain
  path (file association `"%1"`) is a FILE as before. `--line`, `--regex`, `--case`, `--filter`
  need a FILE / `--search` (empty `--search` = none). Order in `main`: `Config::load` → language
  of the settings → parse → `--lang` checked against `i18n::available()` codes (`check_lang`) and
  set → help / version → `--restore` → `validate` (`--from` / `--to` with
  `export::parse_range_bound` and `format.date` of the language, the regex compiled like
  `compile_pattern`). Help / version / errors: `cli::show_message` = stdout / stderr +
  `MessageBoxW` on Windows (help with tabs in the box, padded on the console); errors exit 2,
  help / version exit 0, before the splash and `update::cleanup_old`.
- `--restore <file>` (update restart, `update::RESTORE_ARG`) is the same form as since #15, so an
  older installed version installing this one restarts it correctly; it is accepted anywhere,
  hidden from the help, and when present the other arguments are ignored.
- Several FILEs: always one merged view (`App::open_files`, as Open / drop of several files and
  the command line since #21); `--merge` only makes it explicit.
- App side: `App::new(cc, paths, view, lang, restore)`; `lang` overrides `cfg.language` only in
  `i18n::set_language` (never written to the config; the language menu still shows the saved
  choice, reload language files returns to it). `apply_cli_view` sets search (+
  `search_confirmed`, so short texts search), regex / case / filter mode, `show_sev`
  (`Severities::show_sev`), time range texts (`apply_time_range`), `cfg.auto_scroll` for
  `--tail` / `--no-follow` (like the toolbar checkbox, so it is remembered; ends a pause), then
  `cli_select = Line(n) | FirstMatch` when files were given. `check_loading` applies it after the
  first `Reset` (a full read returns the whole file at once), `open_files` clears it.
  `Line(n)`: file id of the first FILE itself (`files[id].path == paths[0]` among the files of
  log 0, else its last file – not the rotated parts), the last entry of that file with
  `line_no ≤ n` (multi-line entries); select + `scroll_to_selected_after_filter`, follow paused (#37),
  `after_filter.scroll_end` dropped; none → status info `cli.line_not_found`. `FirstMatch` =
  `select_first_match` (waits for a background re-filter). `filter_done` now runs `scroll_end`
  before `select_first`, so a pending first match wins over following the end.
- Help → About: row "Command line" (`about.cmdline`) pointing to `rustrace.exe --help`.

## Guides
- **Guides** (#39, `src/guides.rs` pure + tested): `GUIDES` = (language key of the title, file in
  `docs/guides/`) in menu order, `INDEX` = `README.md`; `url(REPOSITORY, file)` =
  `{repo}/blob/master/docs/guides/<file>`. Help → Guides submenu (`app/menu.rs`), About row
  `about.guides` (`dialogs/help.rs`), log set window: `LogSetDef::guides` (repeatable `guide` key)
  → 📖 buttons next to the set combo; `guides::target` = a plain file name → GitHub URL, anything
  with `/ \ :` (URL, path) as it is; title = the translated `title_key` or the value itself.
  `App::open_link` → `open::that_detached`, on error `error.open_link` with the link in the status
  bar. Test `table_matches_the_guide_files`: every table file exists and is in the index, every
  `docs/guides/*.md` is in the table; built-in sets' guides must be in the table. Adding a guide =
  one `GUIDES` line + `help.guide.*` keys.

## Localization and settings
- Localization: built-in en / sk / cs / de; external `<code>.lang` files load **without recompiling**
  from `lang\` next to the exe or `%APPDATA%\RusTrace\lang`; fallback to English.
- Settings saved automatically to `%APPDATA%\RusTrace\config.json` (with migration of old settings).

## Windows modules
- `src/splash.rs` – start-up splash (#11, Windows): own thread + message loop, WS_POPUP window,
  painted into a DIB (icon, anti-aliased ring of 8 dots, GDI text), per-monitor DPI aware; closed by
  `App::ui` on the second frame (`splash::close`, safe before the window exists).
- `src/icon.rs` – program icon drawn in code (`rgba(size)`, `ico(sizes)`); `build.rs` includes it
  with `#[path]` and embeds the .ico + version info with `winresource` (needs `rc.exe`).
- `src/fileassoc.rs` – Windows file association (.log / .lo_, HKCU only, raw advapi32 FFI):
  ProgID `RusTrace.LogFile`, `RegisteredApplications` → Settings page; `status()` reads
  `UserChoiceLatest\ProgId` / `UserChoiceLatest` / `UserChoice` (a user choice cannot be set by a
  program), names via `AssocQueryStringW`. Test `register_and_unregister` uses its own HKCU key.

## Tests and benchmarks
- `cargo test` (131 tests: numeric column filters (#42), table export scopes / cells / columns / cancel / CSV / XLSX / ZIP writer / statistics / sheet per file / JSONL / HTML / presets / `--export` (#32), MSI verbose log / header dates (#40), guides table / URLs / log set guides (#39), IIS column order with `*` (#41), follow end pause / resume (#37), rules window default width (#48), keyboard shortcut order (Shift+F3 / Shift+F2 not taken by F3 / F2, #46), log sets (set file parsing / translations / built-in sets, variables, loading priorities, file checks, remote computer names / variables / drive letters / share roots, network error codes, recent computers), HTTP status lookup / class fallback / lookup input parsing and ambiguity / cell values, status column keys of log definitions, command-line parsing (file association, all options, errors, `--restore`, language / time / regex checks, help text), overview buckets incremental = full / pixel runs, readable export with Δ, durations, match bits set / iterate, bookmark file id remap, update releases list / missed versions / check setting migration, merged logs (order by time, appended older rows, formats + rotated parts + re-read, single log unchanged, interleave), store copy_entry / split_off, raw lines of entries (all formats / encodings), filter (parallel = sequential, match bits), date column filter = text filter, updates (versions, release JSON, notes, sha256 file, ZIP, install, restore state), icon / .ico layout, search as you type threshold, status-bar position percentage, file association (registry in an own HKCU test key), search history, store drop_front / move to temp file, rotated parts (naming, merge on open, rotation while watching, replaced
  file), export segments by file, time range bounds, column filter syntax, parsers, chunked/ANSI/UTF-16 reading, log definitions (CBS, DISM, IIS,
  Exchange connectivity / CSV separator, choice by name/content, appending across refreshes), store (32-byte record, ISO ↔ ms, push /
  truncate / append, mapped temp file growing past 64 MB, interner), export incl. end-to-end and
  visible rows as highlighted rows, error codes, i18n completeness, config migration)
- Filter benchmark (ignored): `RUSTRACE_BENCH_FILE=<log> cargo test --release bench_filter --
  --ignored --nocapture` – date column filter as text vs ranges, search sequential vs parallel.
- Benchmark (ignored test, Windows): `RUSTRACE_BENCH_FILE=<log> cargo test --release
  bench_large_file -- --ignored --nocapture` (`RUSTRACE_BENCH_MAPPED=1` = temp file); prints load /
  search / random access times and private bytes. A realistic 400 MB CMTrace log can be generated
  with a small script (random components, threads, messages, ~1 % multi-line).
- **Test logs**: `python scripts/gen_testlogs.py [--small-only]` creates `testlogs/` (in `.gitignore`,
  ~650 MB with the big ones, ~1 min) – CMTrace (UTF-8, UTF-8 BOM, UTF-16 LE, multi-line, unfinished
  last line), SMS format, plain ANSI 1250 / without timestamps, CBS, DISM, IIS (two `#Fields` sets,
  detection by content), Exchange Transport Connectivity (by name and by content), a rotation set with set modification times, big CMTrace 400 MB / CBS
  100 MB / IIS 100 MB; list in `testlogs/README.txt`. Use these for manual / GUI checks.
  `cargo test --release testlogs_overview -- --ignored --nocapture` loads all of them and prints
  format, encoding, files, entries, errors/warnings, dated entries and time.
- `cargo fmt` + `cargo clippy --all-targets --locked -- -D warnings` – the code is rustfmt-clean and
  clippy-clean; CI (`.github/workflows/ci.yml`, #20) fails otherwise. CI on every push to master and
  every PR: Windows job fmt / clippy / test / release build (exe as artifact), Linux job clippy /
  test. Formatting-only commits go into `.git-blame-ignore-revs`.

## Packaging and releases
- `scripts\package.ps1` → `dist\RusTrace-v<version>-windows-x64.exe` (7-Zip SFX) and `.zip` (same
  content, for extracting manually), each with a `.sha256`; both extract to
  `RusTrace\` with `rustrace.exe` + `lang\` + `logdef\` + `logsets\` + `LICENSE`. Needs 7-Zip.
- Releases: push tag `v<version>` (matching `Cargo.toml`) → GitHub Actions `release.yml` tests,
  builds, packages and creates a **draft** release; then publish it. Manual workflow run = artifact only.
  `scripts\release.ps1 <version> [-Publish] [-DryRun] [-NoWatch] [-Yes]` does all of it (checks,
  version bump, tests with config backup, commit, push, tag, wait, publish); the owner runs it
  himself to save tokens.
- CRT is linked statically via `.cargo/config.toml` (no VC++ redistributable needed).
- Line endings: `.gitattributes` forces LF for all text files (`* text=auto eol=lf`); write new
  files with LF. The release `.sha256` is written with LF so `sha256sum -c` works.

## Module details
- `src/merge.rs` – `LogSet` (several logs in one view by time), `interleave`, `apply_appended`.
- `src/parser.rs` – `LogSource::poll()` chunked incremental reading, encodings, format parsing,
  `read_line_ranges()` for original-line export. Dates stored as ISO `YYYY-MM-DD HH:MM:SS.mmm`,
  formatted for display by the UI (`format.date` in the language file).
- `src/store.rs` – `LogStore` / `EntryRef` (compact entries, text arena memory / mapped temp file,
  interner, `iso_to_ms` / `ms_to_iso`).
- `src/logdef.rs` – log definitions: `.def` parsing, loading/overrides, choice by name/content,
  `DefState::parse()` line → `LogEntry`, `Schema` / `Column` / `Role`.
- `src/filter.rs` – `FilterSpec` / `CellFilter` / `MatchBits`, sequential + parallel filter run.
- `src/colfilter.rs` – column filter syntax, `DateLayout`, `DateFilter` (date column as ranges).
- `src/export.rs` – pure export logic (range merging, time filter, date input parsing, last-N, output).
- `src/logset.rs` – log sets: `.set` parsing, loading, variables (`Vars`, `resolve`), file checks.
- `src/errcodes.rs` – error code detection/parsing/lookup.
- `src/httpstatus.rs` – HTTP status + IIS substatus descriptions, lookup input (`404.3`, `HTTP 500`).
- `src/update.rs` – update check / download / install / restart state; `src/http.rs` – WinHTTP GET.
- `src/excerpt.rs` – text of the message cell: first line, or an excerpt around a search match.
- `src/timefmt.rs` – durations for display (`format_duration`; Δ column, selection span).
- `src/i18n.rs` – language files, `t!()` macro, fallback, external language folders.
- `src/config.rs` – `Config`, `HighlightRule`, per-theme palettes, serde defaults/migration.
- `lang/*.lang` – built-in translations, embedded with `include_str!`.
- `logdef/*.def` – built-in log definitions, embedded with `include_str!` and shipped in `logdef\`.
- `logsets/*.set` – built-in log sets, embedded with `include_str!` and shipped in `logsets\`.
