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

## Ideas not done yet (offered by the owner)
(none at the moment)


## Possible further memory work (not done)
- The "original lines" export reads the whole log file into memory (`read_line_ranges`), a
  temporary peak of ~2× the file size while exporting; could stream the file instead.
- `filtered: Vec<usize>` could be `Vec<u32>` (8 → 4 bytes per visible row).
