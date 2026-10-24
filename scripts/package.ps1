<#
.SYNOPSIS
    Packages RusTrace as a self-extracting archive for Windows.

.DESCRIPTION
    Takes the release build of rustrace.exe and the built-in language files and produces
    dist\RusTrace-<version>-windows-x64.exe - a 7-Zip self-extracting archive (7z.sfx).
    Running it asks for a destination folder and extracts:

        RusTrace\
            rustrace.exe
            lang\*.lang
            LICENSE

    Requires 7-Zip (7z.exe and the 7z.sfx module), found in PATH or in "Program Files\7-Zip".
    Build first with: cargo build --release

.PARAMETER Version
    Version used in the file name, e.g. "v0.1.0". Default: "v" + version from Cargo.toml.
#>
param(
    [string]$Version = ""
)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent

if (-not $Version) {
    $match = Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"' |
        Select-Object -First 1
    $Version = 'v' + $match.Matches[0].Groups[1].Value
}

$exe = Join-Path $root 'target\release\rustrace.exe'
if (-not (Test-Path $exe)) { throw "$exe not found - run 'cargo build --release' first." }

# Locate 7-Zip and its GUI self-extractor module. The installation folder is preferred, because a
# 7z.exe in PATH may be a shim (e.g. Chocolatey) without 7z.sfx next to it.
$candidates = @(Join-Path $env:ProgramFiles '7-Zip\7z.exe')
$candidates += Get-Command 7z.exe -All -ErrorAction SilentlyContinue | ForEach-Object Source
$sevenZip = $candidates | Where-Object { $_ -and (Test-Path (Join-Path (Split-Path $_) '7z.sfx')) } |
    Select-Object -First 1
if (-not $sevenZip) { throw '7-Zip with the 7z.sfx module not found (install 7-Zip).' }
$sfxModule = Join-Path (Split-Path $sevenZip) '7z.sfx'

# Stage the folder layout exactly as it should appear after extraction.
$dist = Join-Path $root 'dist'
$stage = Join-Path $dist 'stage'
$appDir = Join-Path $stage 'RusTrace'
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Force (Join-Path $appDir 'lang') | Out-Null
Copy-Item $exe $appDir
Copy-Item (Join-Path $root 'lang\*.lang') (Join-Path $appDir 'lang')
Copy-Item (Join-Path $root 'LICENSE') $appDir

# Archive the RusTrace folder itself, so everything extracts into <destination>\RusTrace.
$archive = Join-Path $dist 'RusTrace.7z'
if (Test-Path $archive) { Remove-Item $archive -Force }
Push-Location $stage
try {
    & $sevenZip a -t7z -mx=9 $archive 'RusTrace' | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "7-Zip failed with exit code $LASTEXITCODE" }
} finally {
    Pop-Location
}

# A 7-Zip SFX is the SFX module followed directly by the 7z archive.
$output = Join-Path $dist "RusTrace-$Version-windows-x64.exe"
$out = [System.IO.File]::Create($output)
try {
    foreach ($part in @($sfxModule, $archive)) {
        $in = [System.IO.File]::OpenRead($part)
        try { $in.CopyTo($out) } finally { $in.Dispose() }
    }
} finally {
    $out.Dispose()
}
Remove-Item $archive -Force
Remove-Item $stage -Recurse -Force

$hash = (Get-FileHash $output -Algorithm SHA256).Hash.ToLower()
# LF line ending, so "sha256sum -c" on Linux / macOS reads the file too.
[System.IO.File]::WriteAllText("$output.sha256", "$hash  $(Split-Path $output -Leaf)`n",
    [System.Text.Encoding]::ASCII)

Write-Host "Created $output"
Write-Host "SHA256  $hash"
