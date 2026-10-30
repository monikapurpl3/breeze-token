# run.ps1 -- run breeze-token once on Windows, and leave nothing behind.
#
#   irm https://aspic.salataputarica.hr.eu.org/breeze-token/run.ps1 | iex
#
# With arguments:
#   & ([scriptblock]::Create((irm https://aspic.salataputarica.hr.eu.org/breeze-token/run.ps1))) --write config.json
#
# What it does, and nothing more:
#   * downloads breeze-token.exe into a new folder in your temporary directory;
#   * checks it against SHA256SUMS, and runs nothing that does not match;
#   * runs it once, in this window, with the arguments given;
#   * deletes the folder again, whatever happened.
#
# Nothing is installed: no PATH change, no profile, no registry.
#
# Plain ASCII: Windows PowerShell 5.1 reads a file without a BOM as ANSI.
#
# Copyright 2026 Monika. AGPL-3.0-or-later; see LICENSE.

# A child scope: under `irm | iex` this runs in the caller's own session,
# which must not be left holding its variables, or be closed by an `exit`.
& {
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$Base = if ($env:BREEZE_TOKEN_BASE) { $env:BREEZE_TOKEN_BASE.TrimEnd('/') } else { 'https://aspic.salataputarica.hr.eu.org/breeze-token' }

# Windows on ARM runs the x64 build, emulated.
$Me = switch ($env:PROCESSOR_ARCHITECTURE) {
    'AMD64' { 'windows-x86_64'; break }
    'ARM64' { 'windows-x86_64'; break }
    default { $null }
}
if (-not $Me) {
    Write-Host "breeze-token: there is no build for Windows on $($env:PROCESSOR_ARCHITECTURE)"
    $global:LASTEXITCODE = 1
    return
}
# Windows PowerShell 5.1 may not offer TLS 1.2 by default.
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
} catch {}

$Dir = Join-Path ([IO.Path]::GetTempPath()) ('breeze-token-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $Dir | Out-Null
try {
    $Sums = Join-Path $Dir 'SHA256SUMS'
    Invoke-WebRequest -UseBasicParsing -Uri "$Base/SHA256SUMS" -OutFile $Sums
    # "HASH  breeze-token-VERSION-LABEL.exe": this machine's line. One
    # version is served at a time.
    $Pattern = '^([0-9a-f]{64})  (breeze-token-([0-9][0-9.]*)-' + [regex]::Escape($Me) + '\.exe)$'
    $Best = $null
    foreach ($Line in (Get-Content -LiteralPath $Sums)) {
        $M = [regex]::Match($Line.Trim(), $Pattern)
        if ($M.Success) { $Best = $M }
    }
    if ($null -eq $Best) { throw "$Base has no breeze-token for $Me" }
    $Exe = Join-Path $Dir 'breeze-token.exe'
    Invoke-WebRequest -UseBasicParsing -Uri "$Base/$($Best.Groups[2].Value)" -OutFile $Exe
    $Got = (Get-FileHash -Algorithm SHA256 -LiteralPath $Exe).Hash.ToLowerInvariant()
    if ($Got -ne $Best.Groups[1].Value) { throw "$($Best.Groups[2].Value) does not match its SHA-256 in SHA256SUMS; nothing was run" }
    Write-Host "breeze-token $($Best.Groups[3].Value) for $Me, checked against its SHA-256. It runs once and is deleted afterwards."
    # Its messages on stderr are its own, not errors: Windows PowerShell 5.1
    # would otherwise throw on the first one whenever stderr is redirected.
    $ErrorActionPreference = 'Continue'
    & $Exe @args
    $global:LASTEXITCODE = $LASTEXITCODE
} catch {
    Write-Host "breeze-token: $($_.Exception.Message)"
    $global:LASTEXITCODE = 1
} finally {
    Remove-Item -LiteralPath $Dir -Recurse -Force -ErrorAction SilentlyContinue
}
} @args
