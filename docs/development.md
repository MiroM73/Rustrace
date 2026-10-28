# Development

Tests, checks, test logs, benchmarks and build details for contributors.

[← Back to README](../README.md)

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
