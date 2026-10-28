# Log definitions

How to describe any line-based log format in a small `.def` file – no recompiling.

[← Back to README](../README.md)

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
