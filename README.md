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

## At a glance

- CMTrace / ConfigMgr and SMS logs, plain text and any other layout described by a small
  [log definition](docs/log-definitions.md) – built in: IIS, Exchange, DISM, CBS, Windows Installer
- several logs in one view merged by time, rotated parts (`.lo_`) included; [log sets](docs/log-sets.md)
  open the logs of one scenario at once, also from a remote computer
- adjustable automatic refresh in a background thread – logs of hundreds of MB stay responsive
- search as you type (text or regex), filter mode, severity toggles, column filters, time range
- color highlight rules, light / dark theme, overview strip, Δ time column, bookmarks
- error code and HTTP / IIS status explanation in the details pane, raw line view
- [export](docs/export.md) of highlighted rows with context, table export to CSV / Excel / JSON Lines / HTML
- [command line](docs/command-line.md) options, also for exports without a window
- [translatable user interface](docs/languages.md) (English, Slovenčina, Čeština, Deutsch) – no recompiling

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

Updates (Help → Check for updates…), the default app for `.log` / `.lo_` files and the About
window: see [Features](docs/features.md#updates).

## Building

1. Install Rust: https://rustup.rs (on Windows with Visual Studio Build Tools – "Desktop development with C++").
2. In the project folder:

```
cargo build --release
```

Result: `target\release\rustrace.exe` – a single self-contained executable.

Open a specific file: `rustrace.exe C:\Windows\CCM\Logs\AppEnforce.log`
(it can also be set as the default program for `.log` files). Files can be dragged onto the window too.
Options for opening at a line, searching and filtering: see [Command line](docs/command-line.md).

Tests: `cargo test`. Checks, test logs, benchmarks and build details: [Development](docs/development.md).

## Documentation

| Document | Content |
|---|---|
| [Guides](docs/guides/README.md) | step-by-step help for real ConfigMgr situations |
| [Features](docs/features.md) | everything the program does, area by area |
| [Export](docs/export.md) | export of highlighted rows with context, table export (CSV / Excel / JSON Lines / HTML) |
| [Log definitions](docs/log-definitions.md) | `.def` files for other log layouts |
| [Log sets](docs/log-sets.md) | `.set` files, remote computers, path variables |
| [Languages](docs/languages.md) | built-in languages, adding or fixing a translation |
| [Command line](docs/command-line.md) | options and examples |
| [Keyboard shortcuts](docs/keyboard-shortcuts.md) | all shortcuts of the main window |
| [Development](docs/development.md) | tests, CI checks, test logs, benchmarks, build details |
| [Making a release](docs/releasing.md) | packaging and publishing (maintainers) |
| [Source layout](docs/source-layout.md) | what every source file contains |

## License

RusTrace is free and unencumbered software released into the **public domain** under
[the Unlicense](LICENSE): anyone may copy, modify, publish, use, compile, sell or distribute it,
in source or binary form, for any purpose, without any conditions. It is provided "as is",
without warranty of any kind.

The third-party Rust libraries compiled into the program (egui/eframe, regex, serde, …) keep their
own licenses (mostly MIT / Apache-2.0).
