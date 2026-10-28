# RusTrace – history and open ideas

Notes for Claude, linked from `CLAUDE.md`.

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

## History (2026-10-24)
10. Ctrl + mouse wheel zoom: log text vs. whole UI depending on the pointer position.
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
20. Test logs generator (`scripts/gen_testlogs.py`, `testlogs/` not in git) + `testlogs_overview`;
    it found that a `.lo_` opened directly was merged with itself – fixed (rotated parts opened
    directly are never merged).
21. Release v0.3.1 (bug fix: a rotated part opened directly was read twice).
22. Fix: a later w3c `#Fields:` header replaced the table columns, so rows of the first part lost
    their columns; now the first header replaces the default fields, later ones only add columns.
23. Horizontal scroll bar under the table for columns that do not fit (Shift + wheel too).
24. Log definitions: clear priority (user folder > next to exe > built-in) instead of file-name
    order, active definition with its file shown, warning when several match (owner's report).
25. Own definitions no longer replace predefined ones with the same file name (both offered),
    setting which groups are used, notice of new / changed predefined definitions after updates.
26. Release v0.4.0 (IIS columns fix, horizontal scroll bar, log definition priority / groups /
    update notice).

## History (2026-10-25)
27. GitHub workflow with issues + PRs started. #2: match count right after the search
    field.

## History (2026-10-26)
28. #1: a search match hidden in a long message → excerpt in the cell, highlighted and
    scrolled to in the details pane.
29. #13: Exchange Server Transport Connectivity log definition; w3c definitions got `separator`
    (comma-separated `#Fields:` and values, CSV quoting, `-` kept as a value).
30. Release v0.5.0 (#2 search match count, #1 search match in long messages, #13 Exchange
    Transport Connectivity log definition + w3c `separator`).
31. `scripts/release.ps1` – the owner makes releases himself (checks, version, tests, tag, publish).
32. #10 version in the window title, #7 Help → About, #5 search history (🕘 next to the search
    field, `cfg.search_history`, max 30), #9 default app for .log / .lo_ (`src/fileassoc.rs`).
33. Release v0.6.0 (#10, #7, #5, #9).
34. #6 high CPU: the AMD driver spun a core with wgpu → OpenGL renderer by default with wgpu
    fallback; fewer frames per auto refresh.
35. Release v0.6.1 (#6 high CPU fix).
36. #3: position of the selected row in the status bar (`📍 Row 25 / 100 (25 %)`, counted among
    the displayed rows `filtered`, tooltip = line in the file; `position_percent` rounds down).
37. #4: search as you type from `INSTANT_SEARCH_MIN_CHARS` (3) characters, the first match (from the
    top of `filtered`) is selected; shorter texts only after Enter / F3 / history / search component
    (`search_confirmed`, cleared by typing); before, every keystroke searched even 1 character.
38. #11: start-up splash (icon, name + version, "Starting…", rotating dots) until the main window
    shows; new program icon drawn in code, used for the window and embedded in the exe.
39. #15: update check (Help menu + automatic once a day, can be turned off), download + install of
    the release ZIP over the running program and restart with the same log and view.
40. Release v0.7.0 (#11 splash + icon, #15 updates). Verified end to end on the published release:
    0.7.0 started with RUSTRACE_UPDATE_AS_VERSION=0.6.0 updated itself and came back with the log,
    search and selected row; `rustrace.exe.old` was deleted by the new process.

## History (2026-10-27)
41. #19: `src/app.rs` (~4 450 lines) split into `src/app/` modules – a pure move of code, no
    behaviour change. Checked: the multiset of non-blank source lines is the same apart from module
    headers / visibility, 64 tests pass, clippy shows the same warnings as before, Xvfb screenshots
    of the main window from the old and new build are pixel-identical (except the clock in the
    status bar). Not run through `cargo fmt` on purpose (the code was not rustfmt-clean before and
    formatting would hide the move in the diff).
42. #20: CI workflow `ci.yml` (Windows: fmt, clippy -D warnings, tests, release build artifact;
    Linux: clippy, tests), the whole code formatted with `cargo fmt` in its own commit (listed in
    `.git-blame-ignore-revs`), the one clippy warning (test in `config.rs`) fixed, CI badge.
43. #16 search / filters of big logs in a background thread (parallel, cancelable,
    previous result stays visible), #17 date/time column filter compared as time ranges,
    #18 match count kept incrementally (`MatchBits`). Release v0.8.0.
44. #29 details pane Message | Raw line (byte range of every entry, read from the file on demand).
45. #21 several different logs merged into one view by time (`LogSet`), tabs still not wanted.
    Release v0.9.0 (#21, #29), published 2026-10-27.
46. #31 update check at every start (setting start / daily / never) and the notes of all missed
    releases in the update window. Release v0.9.1.
47. #22 bookmarks (Ctrl+F2, F2 / Shift+F2, notes, list window, filter, export criterion).
48. #25 several selected rows (Shift / Ctrl, Ctrl+A, copy messages / table / original lines,
    export selected rows).
49. #23 Δ time column (previous shown / previous of the log / reference row, long gaps colored).
50. #24 overview strip next to the scroll bar (errors, warnings, search, bookmarks; click to scroll).
51. Release v0.9.5 (#22 bookmarks, #25 several selected rows, #23 Δ time column, #24 overview
    strip).
52. #26 command-line arguments (`--line`, `--search` / `--regex` / `--case` / `--filter`,
    `--errors` / `--warnings`, `--from` / `--to`, `--tail` / `--no-follow`, `--merge`, `--lang`,
    `--help`, `--version`; own small parser in `src/cli.rs`, message box on Windows, `--restore`
    unchanged).
53. #28 HTTP / IIS status codes explained (details pane, tooltip of the status cells, Ctrl+E accepts
    `404.3` / `HTTP 500`; a plain 100–599 shows both meanings), driven by the new optional `.def`
    keys `http_status` / `http_substatus` / `win32_status` set in `iisweb.log.def`; descriptions
    `http.*` in all four languages (`src/httpstatus.rs`).
54. #27 (part 1) log sets: File → Open log set…, `.set` files (built-in application deployment /
    software updates / client installation / policy / Intune; own in `%APPDATA%\RusTrace\logsets`),
    names / descriptions translated in the set file, `%CCMLOGS%` from the client's registry, file
    check in a background thread, Open = merged view. Paths of the issue checked: ccmsetup logs
    in `%WINDIR%\ccmsetup\Logs`, IME logs in `%PROGRAMDATA%\Microsoft\IntuneManagementExtension\Logs`
    (+ `AppWorkload.log`, `AppActionProcessor.log`), `WindowsUpdate.log` left out (ETL since Win 10).
55. #27 (part 2) log sets on a remote computer: name / IP → admin shares (`%WINDIR%` =
    `\\pc\admin$`, `%PROGRAMDATA%` = `\\pc\c$\ProgramData`, drive letters → `\\pc\x$`), shares
    checked first with a 10 s timeout, translated messages (unreachable / no answer / access denied /
    share missing), recently used computers in the settings, UNC watching + rotation tested over
    `\\localhost\c$`. Not tested on a real remote ConfigMgr client.
56. Release v1.0.0 (#26 command line, #28 HTTP / IIS status codes, #27 log sets) – first version
    the owner considers complete for full everyday use.
57. Guides for real situations (`docs/guides/`, linked from the README): the first one, *An
    application deployment failed* (log set, errors only, error code explanation, Δ time,
    bookmarks, filter by application, MSI log, export, command line, table of typical codes),
    with annotated screenshots of v1.0.0 made on Xvfb from invented logs
    (`scripts/gen_guide_logs.py`, fixed seed). Found while taking them: a click on a row does not
    turn "Follow end" off (the view jumps to the end after a filter change), and the Export button
    is disabled while an automatic refresh read is pending (`busy` = `self.loading.is_some()`; on
    Xvfb with 6 merged logs it stayed disabled until the refresh was paused) – not fixed yet.
58. Second guide *Software updates do not install* (remote log set over `admin$`, CMTrace + CBS.log
    merged, column filters `ScanAgent|WUAHandler` and `!CSI`, time range around the deadline,
    highlight rule for the update ID / KB / CBS package, raw line, export of the visible rows,
    codes table for scan / install errors). `scripts/gen_guide_logs.py` now runs one generator per
    guide from `scripts/guide_logs/` (the first guide's logs are unchanged, checked by checksum).

## History (2026-10-28)
59. Guides 3 and 4: *A new ConfigMgr client does not register* (client installation log
    set with a rotated part, rows without a date, harmless MSI notes, regex milestones, time
    reference, bookmarks list, error code lookup) and *The management point / distribution point is
    not healthy* (IIS log: own log definition with the status columns first, 500 after 30 s,
    503.2 tooltip, column filter `!=2`, 401.1 + 1326, 404.3, regex search of slow requests);
    generators `scripts/guide_logs/client_install.py`, `iis_management_point.py`. Issues found
    while writing them: #40 MSI verbose log definition (rows at the top of a merged view, false
    errors), #41 IIS column order, #42 numeric column filters, #43 IIS Win32 status texts (the
    1326 / 50 texts in the IIS screenshots come from a temporary external `en.lang` override –
    without it the Linux build shows "Unknown code"; on Windows the system message is shown),
    #44 search highlight in all cells. Note: the screenshot build splits file names on `\` too
    (Windows-like names on Linux); log definitions match by `Path::file_name`, so on Linux the
    own IIS definition needed `detect` to be chosen – not an issue on Windows.
60. Guide 5 *The ConfigMgr client installation fails* (ccmsetup.log: 1618 then 1603 with the failed
    action and the error messages in the details pane, resizing the details pane; client.msi.log:
    misleading keyword errors, MSI highlight rules shipped as `docs/guides/rules/msi-verbose-log.json`,
    the first `Return value 3`, selection → Copy original lines / Explain code, WMI fix);
    generator `scripts/guide_logs/client_msi_1603.py`. Issues found: #46 Shift+F3 / Shift+F2 go
    forward (egui `consume_key` ignores extra Shift – NONE pattern checked first), #47 Enter after
    search-as-you-type skips the first match, #48 rules window columns cut off, #49 empty columns
    of plain text logs, #50 severity from highlight rules, #51 rules import duplicates / partial
    export.
61. #46 Shift+F3 / Shift+F2 went forward: egui `consume_key` matches modifiers with
    `matches_logically` (extra Shift / Alt ignored) and the `NONE` pattern was checked first, so it
    also took the Shift variant. New helper `consume_key_and_shifted` in `app/input.rs` consumes
    `Shift+key` first (F3 and F2), module note on the order rule, unit test (fails with the old
    order). Other `consume_key` calls reviewed: Ctrl+Shift+S has no Ctrl+S counterpart and is
    consumed first anyway; Ctrl is matched exactly, so F5 / Ctrl+F5 and F2 / Ctrl+F2 are fine;
    Ctrl+Shift+C does not produce egui's `Copy` event (egui-winit requires Ctrl only).
62. #48 Highlight rules window columns cut off: the table (name 150 px, pattern 300 px fields) was
    wider than the 1150 px default window, egui's floating scroll bars stayed hidden, so the
    ↑ ↓ 🗑 column (or On / Name) was cut off without any hint. Name 140 px / pattern 250 px with
    the full text in the tooltip, default width 1240 px limited to the main window (min. 600 px),
    solid scroll bar style, scroll offset reset on opening. Verified on Xvfb: 3 default + 3 MSI
    rules fully visible in a 1400 × 900 window (English and German), 1000 × 800 shows the
    horizontal bar and starts at the left; unit test for the width.
63. #37 Follow end paused while a row is selected: `cfg.auto_scroll` stays the user's setting, a
    runtime `Follow` pause (`app/follow.rs`) is set by every selection path above the last row and
    cleared by Esc (new: clears the selection), Ctrl+click on the only selected row, a click below
    the last row, the last row (End, click, ↓ / PgDn), scrolling to the bottom, the status bar
    button, the checkbox, reloading. Filter changes keep the selected row in view. Paused shown by
    the orange checkbox (first version changed the label to "(paused)", which moved the toolbar
    buttons under the pointer – a click meant for Info hit the checkbox), "(paused)" in the View
    menu and *⏸ Follow end paused* in the status bar. `SessionState::follow_paused` keeps it
    over an update restart. Verified on Xvfb with an appended client.msi.log: click on row 107 →
    paused, 40 appended rows do not scroll, Info off / on keeps row 107 centered, Esc → selection
    cleared and the view follows new rows, End on a row → follows, mouse wheel to the bottom →
    follows. Guides (client-install, client-msi-1603, failed-app-install) no longer tell users to
    untick Follow end.
64. #38 Export… disabled while an automatic refresh read was pending (with auto refresh on almost
    always – the guides told users to press Ctrl+P first). The button now waits only for a full
    read (`Loading::full` from `LogSet::has_read()`, which is now set for a single log too) with a
    spinner and "Reading the log…"; refresh reads do not block it because the export copies the
    rows, lines and file paths before its thread starts. Ctrl+P workaround removed from the
    failed-app-install and software-updates guides; unit test for `has_read` in
    `single_log_passes_through`.
65. Release v1.0.8 (#38 export during auto refresh, #37 Follow end paused by a selection, #46
    Shift+F3 / Shift+F2, #48 rules window width, guides 1–5). `release.ps1` must run under
    PowerShell 7 (`pwsh`): Windows PowerShell 5.1 strips the quotes of the `gh run list --jq`
    filter, so the "waiting for the workflow" step failed after the tag was pushed.
66. #41 Built-in IIS definition shows the status columns first: new `*` item in the `columns` key
    of log definitions (= all the other columns in field order, so no `#Fields:` field
    disappears); `iisweb.log.def` gets `columns = datetime sc-status sc-substatus sc-win32-status
    time-taken c-ip cs-username message *`. Users with an own copy get the "changed predefined
    definitions" notice. Tests for the default and a large `#Fields:` set, and for `columns`
    without `*` (unlisted columns hidden); the IIS guide's step 2 is now optional.

## Ideas not done yet (offered by the owner)
- `--set "<name>"` command-line option for log sets (#27): would need the set loading, the file
  check (and for a remote computer its timeout) before the window starts – not done for now.


## Possible further memory work (not done)
- The "original lines" export reads the whole log file into memory (`read_line_ranges`), a
  temporary peak of ~2× the file size while exporting; could stream the file instead.
- `filtered: Vec<usize>` could be `Vec<u32>` (8 → 4 bytes per visible row).
