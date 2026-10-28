# Languages

Built-in languages and how to add or fix a translation without recompiling.

[← Back to README](../README.md)

Built in: **English, Slovenčina, Čeština, Deutsch**. Choose under View → Language. "Automatic" follows
the operating system language (falls back to English).

## Adding or fixing a translation – no recompiling

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
