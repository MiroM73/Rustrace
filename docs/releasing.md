# Making a release

How the Windows release archives are built and published (maintainer information).

[← Back to README](../README.md)

- Locally: `cargo build --release`, then `scripts\package.ps1` (needs 7-Zip) →
  `dist\RusTrace-v<version>-windows-x64.exe` and `.zip`, each with a `.sha256`.
- On GitHub, in one step: `scripts\release.ps1 0.6.0` (add `-Publish` to publish right after the
  build, `-DryRun` to only run the checks and tests). It checks the repository (master, clean, not
  behind, tag not used yet), sets the version in `Cargo.toml`, runs the tests (backing up
  `%APPDATA%\RusTrace\config.json`), asks for confirmation, commits, pushes, tags, waits for the
  workflow and shows the draft. Run `Get-Help scripts\release.ps1 -Full` for the details.
- On GitHub by hand: bump `version` in `Cargo.toml`, commit, then push a tag, e.g.
  `git tag v0.1.0 && git push origin v0.1.0`. The workflow `.github/workflows/release.yml` runs the
  tests, builds, packages and creates a **draft** release with the archive attached; review it and
  publish. A manual run of the workflow (Actions → Release → Run workflow) only builds the archive as
  a downloadable artifact.
