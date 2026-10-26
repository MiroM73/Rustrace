<#
.SYNOPSIS
    Makes a new RusTrace release on GitHub: version bump, tests, commit, tag, build, publish.

.DESCRIPTION
    Runs the whole release procedure that otherwise has to be typed by hand:

      1. Checks the tools (git, cargo, gh), that gh is logged in, that the current branch is
         master, that there are no uncommitted changes, that master is not behind origin and
         that the tag v<version> does not exist yet (locally or on GitHub).
      2. Writes the new version into Cargo.toml (skipped when it is already there).
      3. Runs "cargo test". The user settings %APPDATA%\RusTrace\config.json are backed up before
         and restored afterwards (or deleted when they did not exist), because tests must not
         change them. cargo also updates the version in Cargo.lock.
      4. Asks for confirmation, then commits Cargo.toml + Cargo.lock ("Release v<version>") and
         pushes master to GitHub.
      5. Creates the annotated tag v<version> and pushes it. The push starts the GitHub Actions
         workflow .github/workflows/release.yml, which tests, builds, packages (.exe SFX + .zip,
         each with .sha256) and creates a DRAFT release.
      6. Waits for the workflow and shows the draft (skipped with -NoWatch).
      7. With -Publish, publishes the draft and marks it as the latest release. Without it the
         draft stays unpublished so its notes can be reviewed / edited on GitHub first.

    Nothing is pushed before step 4, so a failed check or test leaves GitHub untouched.

.PARAMETER Version
    The new version, e.g. "0.6.0" or "v0.6.0" (major.minor.patch).

.PARAMETER Publish
    Publish the draft release after a successful build.

.PARAMETER NoWatch
    Do not wait for the GitHub workflow; the script ends right after pushing the tag.

.PARAMETER Yes
    Do not ask for confirmation before committing and pushing.

.PARAMETER DryRun
    Only run the checks and the tests; Cargo.toml is not changed and nothing is committed,
    tagged or pushed.

.EXAMPLE
    scripts\release.ps1 0.6.0
    Release 0.6.0, wait for the build and leave the draft for review.

.EXAMPLE
    scripts\release.ps1 0.6.0 -Publish
    Release 0.6.0 and publish it right after the build succeeds.

.EXAMPLE
    scripts\release.ps1 0.6.0 -DryRun
    Check that everything is ready for 0.6.0 without changing anything.
#>
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$Version,
    [switch]$Publish,
    [switch]$NoWatch,
    [switch]$Yes,
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

function Write-Step([string]$Text) {
    Write-Host ''
    Write-Host "==> $Text" -ForegroundColor Cyan
}

# Runs an external program and stops the script when it fails (PowerShell does not do that by
# itself for native programs, it only sets $LASTEXITCODE).
function Invoke-Native([string]$File, [string[]]$Arguments) {
    & $File @Arguments
    if ($LASTEXITCODE -ne 0) { throw "'$File $($Arguments -join ' ')' failed (exit code $LASTEXITCODE)." }
}

# Same as Invoke-Native, but returns the output as a trimmed string.
function Get-NativeOutput([string]$File, [string[]]$Arguments) {
    $out = & $File @Arguments
    if ($LASTEXITCODE -ne 0) { throw "'$File $($Arguments -join ' ')' failed (exit code $LASTEXITCODE)." }
    return (($out | Out-String).Trim())
}

# --- Version -----------------------------------------------------------------------------------
$Version = $Version.Trim().TrimStart('v')
if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw "Version '$Version' is not in the form major.minor.patch, e.g. 0.6.0." }
$tag = "v$Version"

# --- 1. Checks ---------------------------------------------------------------------------------
Write-Step 'Checking tools'
# cargo lives in %USERPROFILE%\.cargo\bin; shells started before the Rust installation miss it.
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $env:PATH += ";$env:USERPROFILE\.cargo\bin"
}
foreach ($tool in 'git', 'cargo', 'gh') {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { throw "'$tool' was not found in PATH." }
}
# gh must be logged in to GitHub, otherwise it cannot watch the workflow or publish the release.
& gh auth status *> $null
if ($LASTEXITCODE -ne 0) { throw "GitHub CLI is not logged in - run 'gh auth login' first." }

Write-Step 'Checking the git repository'
$branch = Get-NativeOutput git @('rev-parse', '--abbrev-ref', 'HEAD')
if ($branch -ne 'master') { throw "The current branch is '$branch'; releases are made from master." }

# "git status --porcelain" prints one line per changed / untracked file; empty = clean tree.
$dirty = Get-NativeOutput git @('status', '--porcelain')
if ($dirty) { throw "There are uncommitted changes - commit or stash them first:`n$dirty" }

# Download the current state of GitHub (no change to local files) and count the commits that
# are on origin/master but not here. Those would make the push fail.
Invoke-Native git @('fetch', '--quiet', 'origin')
$behind = [int](Get-NativeOutput git @('rev-list', '--count', 'HEAD..origin/master'))
if ($behind -gt 0) { throw "master is $behind commit(s) behind origin/master - run 'git pull' first." }

if (Get-NativeOutput git @('tag', '--list', $tag)) { throw "Tag $tag already exists locally." }
if (Get-NativeOutput git @('ls-remote', '--tags', 'origin', "refs/tags/$tag")) { throw "Tag $tag already exists on GitHub." }

# --- 2. Version in Cargo.toml ------------------------------------------------------------------
Write-Step "Setting the version $Version in Cargo.toml"
$cargoToml = Join-Path $root 'Cargo.toml'
$text = [IO.File]::ReadAllText($cargoToml)
# The first "version = ..." line at the start of a line is the one in [package].
$versionLine = [regex]::new('(?m)^version\s*=\s*"([^"]+)"')
$current = $versionLine.Match($text).Groups[1].Value
if ($current -eq $Version) {
    Write-Host "Cargo.toml already has version $Version."
} elseif ($DryRun) {
    Write-Host "Would change $current -> $Version (dry run, not written)."
} else {
    $text = $versionLine.Replace($text, "version = `"$Version`"", 1)
    # UTF-8 without BOM, line endings kept as they are (LF).
    [IO.File]::WriteAllText($cargoToml, $text, (New-Object System.Text.UTF8Encoding($false)))
    Write-Host "Changed $current -> $Version."
}

# --- 3. Tests ----------------------------------------------------------------------------------
Write-Step 'Running the tests (cargo test)'
$cfg = Join-Path $env:APPDATA 'RusTrace\config.json'
$cfgBackup = "$cfg.release-backup"
$cfgExisted = Test-Path $cfg
if ($cfgExisted) { Copy-Item $cfg $cfgBackup -Force }
try {
    Invoke-Native cargo @('test')
} finally {
    if ($cfgExisted) { Move-Item $cfgBackup $cfg -Force }
    elseif (Test-Path $cfg) { Remove-Item $cfg -Force }
}

if ($DryRun) {
    Write-Step "Dry run finished - everything is ready for $tag."
    exit 0
}

# --- 4. Commit and push ------------------------------------------------------------------------
if (-not $Yes) {
    Write-Host ''
    $answer = Read-Host "Commit, tag and push $tag to GitHub? (y/n)"
    if ($answer -notmatch '^(y|yes|a|ano)$') { Write-Host 'Stopped, nothing was pushed.'; exit 1 }
}

Write-Step 'Committing the version change'
Invoke-Native git @('add', 'Cargo.toml', 'Cargo.lock')
# "git diff --cached --quiet" exits with 1 when something is staged for the commit.
& git diff --cached --quiet
if ($LASTEXITCODE -ne 0) {
    Invoke-Native git @('commit', '--quiet', '-m', "Release $tag", '-m', "Bump the version to $Version (Cargo.toml, Cargo.lock).")
} else {
    Write-Host 'Nothing to commit (the version was already set).'
}

Write-Step 'Pushing master'
Invoke-Native git @('push', 'origin', 'master')

# --- 5. Tag ------------------------------------------------------------------------------------
Write-Step "Creating and pushing the tag $tag (starts the release workflow)"
Invoke-Native git @('tag', '-a', $tag, '-m', "RusTrace $tag")
Invoke-Native git @('push', 'origin', $tag)

if ($NoWatch) {
    Write-Step 'Done. Watch the build with: gh run watch'
    exit 0
}

# --- 6. Wait for the workflow ------------------------------------------------------------------
Write-Step 'Waiting for the GitHub workflow to start'
# GitHub needs a few seconds to register the run; for a tag push its "headBranch" is the tag.
$runId = $null
for ($i = 0; $i -lt 30 -and -not $runId; $i++) {
    Start-Sleep -Seconds 4
    $runId = Get-NativeOutput gh @('run', 'list', '--workflow', 'release.yml', '--limit', '10',
        '--json', 'databaseId,headBranch', '--jq', ".[] | select(.headBranch == `"$tag`") | .databaseId")
    $runId = ($runId -split "`n" | Select-Object -First 1)
}
if (-not $runId) { throw "The workflow run for $tag did not appear - check the Actions tab on GitHub." }

Write-Step "Watching the build (run $runId, takes about 10 minutes)"
& gh run watch $runId --exit-status --interval 30
if ($LASTEXITCODE -ne 0) {
    throw "The build failed. Details: gh run view $runId --log-failed"
}

Write-Step 'Draft release'
Invoke-Native gh @('release', 'view', $tag)

# --- 7. Publish --------------------------------------------------------------------------------
if ($Publish) {
    Write-Step "Publishing $tag"
    Invoke-Native gh @('release', 'edit', $tag, '--draft=false', '--latest')
    Write-Step "Released: https://github.com/MiroM73/Rustrace/releases/tag/$tag"
} else {
    Write-Step 'The draft is ready. Review it on GitHub, then publish with:'
    Write-Host "    gh release edit $tag --draft=false --latest"
}
