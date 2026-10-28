# Command line

Options for opening a log at a line, searching, filtering and exporting without a window.

[← Back to README](../README.md)

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
| `--export <FILE>` | Write the table export (CSV / Excel / JSON Lines / HTML by the extension of FILE) of the rows the other options select and exit without a window – see [Export table](export.md#export-table--csv--excel). Messages go to the console; exit code 0 = written, 1 = export error, 2 = invalid command line. |
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
rustrace AppEnforce.log AppDiscovery.log --errors --from "2026-10-27 08:00" --export errors.xlsx
```
