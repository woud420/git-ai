# git-ai uninstaller for Windows
#
# Primary path: delegates to `git-ai uninstall "$@"` (the installed binary).
# Standalone fallback: when the binary is already gone, removes the known
# artifacts directly and reports what remains.
#
# Usage:
#   irm https://usegitai.com/uninstall.ps1 | iex
#   # or locally:
#   .\uninstall.ps1 [-Yes] [-Purge]
#
# Options:
#   -Yes    Skip the confirmation prompt
#   -Purge  Also delete ~/.git-ai (config + local attribution databases)

param(
    [switch]$Yes,
    [switch]$Purge
)

$ErrorActionPreference = 'Stop'

function Write-Success { param([string]$Msg) Write-Host $Msg -ForegroundColor Green }
function Write-Warn    { param([string]$Msg) Write-Host "Warning: $Msg" -ForegroundColor Yellow }

# ── locate the git-ai binary ─────────────────────────────────────────────────
$gitAiBin = $null
$candidates = @(
    (Join-Path $HOME '.git-ai\bin\git-ai.exe'),
    (Join-Path $HOME '.local\bin\git-ai.exe'),
    (Join-Path $HOME '.local\bin\git-ai')
)
# Also check PATH
$onPath = Get-Command git-ai -ErrorAction SilentlyContinue
if ($onPath) { $candidates += $onPath.Source }

foreach ($c in $candidates) {
    if ($c -and (Test-Path -LiteralPath $c)) {
        $gitAiBin = $c
        break
    }
}

# ── primary path: delegate to the installed binary ────────────────────────────
if ($gitAiBin) {
    $extraArgs = @()
    if ($Yes)   { $extraArgs += '--yes' }
    if ($Purge) { $extraArgs += '--purge' }
    & $gitAiBin uninstall @extraArgs
    exit $LASTEXITCODE
}

# ── standalone fallback ───────────────────────────────────────────────────────
Write-Warn 'git-ai binary not found — running standalone fallback removal.'
Write-Host ''

if (-not $Yes) {
    if ($Purge) {
        Write-Host 'This will remove git-ai artifacts AND delete ~/.git-ai.'
    } else {
        Write-Host 'This will remove git-ai artifacts (binary dir, shell rc edits, PATH entry).'
    }
    $answer = Read-Host 'Continue? [y/N]'
    if ($answer -notmatch '^[yY]') {
        Write-Host 'Aborted.'
        exit 0
    }
}

$remaining = @()

# 1. Remove binary directory (best-effort; exe may be locked on Windows)
$binDir = Join-Path $HOME '.git-ai\bin'
if (Test-Path -LiteralPath $binDir) {
    try {
        Remove-Item -Recurse -Force -LiteralPath $binDir
        Write-Success "Removed $binDir"
    } catch {
        Write-Warn "Could not remove ${binDir}: $($_.Exception.Message)"
        $remaining += $binDir
    }
}

# 2. Remove user PATH registry entry for git-ai bin directory
$gitAiPathEntry = Join-Path $HOME '.git-ai\bin'
try {
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ($userPath) {
        $normalized = ([IO.Path]::GetFullPath($gitAiPathEntry.Trim())).TrimEnd('\').ToLowerInvariant()
        $entries = $userPath -split ';' | Where-Object { $_ -and $_.Trim() -ne '' }
        $filtered = $entries | Where-Object {
            try { ([IO.Path]::GetFullPath($_.Trim())).TrimEnd('\').ToLowerInvariant() -ne $normalized }
            catch { $true }
        }
        $newPath = $filtered -join ';'
        if ($newPath -ne $userPath) {
            [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
            Write-Success 'Removed git-ai from user PATH.'
        }
    }
} catch {
    Write-Warn "Could not update user PATH: $($_.Exception.Message)"
}

# 3. Remove fenced block from Git Bash rc files.
# Fence markers: # >>> git-ai >>> ... # <<< git-ai <<<
# Also strips legacy bare "# Added by git-ai installer …" lines.
$fenceOpen  = '# >>> git-ai >>>'
$fenceClose = '# <<< git-ai <<<'
$legacyMarker = '# Added by git-ai installer'
$rcFiles = @(
    (Join-Path $HOME '.bashrc'),
    (Join-Path $HOME '.bash_profile')
)
foreach ($rcFile in $rcFiles) {
    if (-not (Test-Path -LiteralPath $rcFile)) { continue }
    try {
        $content = Get-Content -LiteralPath $rcFile -Raw -Encoding UTF8
        $cleaned = $content
        # Remove fenced block
        $pattern = "(?ms)^$([regex]::Escape($fenceOpen))\r?\n.*?^$([regex]::Escape($fenceClose))\r?\n?"
        $cleaned = [regex]::Replace($cleaned, $pattern, '')
        # Remove legacy bare lines
        $cleaned = ($cleaned -split "`n" | Where-Object {
            $_ -notmatch [regex]::Escape($legacyMarker) -and $_ -notmatch '\/\.git-ai\/bin'
        }) -join "`n"
        if ($cleaned -ne $content) {
            $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
            [System.IO.File]::WriteAllText($rcFile, $cleaned, $utf8NoBom)
            Write-Success "Cleaned git-ai entries from $rcFile"
        }
    } catch {
        Write-Warn "Could not clean ${rcFile}: $($_.Exception.Message)"
    }
}

# 4. Revert global git trace2 config (only when it points at git-ai)
# Use the two-step Get-Command pattern; the null-conditional ?. operator is
# PowerShell 7+ only and causes a parse error on Windows PowerShell 5.1.
try {
    $gitCmd = Get-Command git -ErrorAction SilentlyContinue
    if ($gitCmd) {
        $gitExe = $gitCmd.Source
        $target = & $gitExe config --global --get trace2.eventTarget 2>$null
        if ($target -and $target -match '\.git-ai') {
            & $gitExe config --global --remove-section trace2 2>$null
            Write-Success 'Removed git trace2 config.'
        }
    }
} catch { }

# 5. Optionally remove ~/.git-ai data directory
if ($Purge) {
    $dataDir = Join-Path $HOME '.git-ai'
    if (Test-Path -LiteralPath $dataDir) {
        try {
            Remove-Item -Recurse -Force -LiteralPath $dataDir
            Write-Success "Removed $dataDir"
        } catch {
            Write-Warn "Could not remove ${dataDir}: $($_.Exception.Message)"
            $remaining += $dataDir
        }
    }
}

Write-Host ''
if ($remaining.Count -gt 0) {
    Write-Warn 'Some artifacts could not be removed automatically:'
    $remaining | ForEach-Object { Write-Host "  $_" }
    Write-Host ''
    Write-Host 'Please remove these manually.'
}

Write-Success 'git-ai uninstall complete.'
Write-Host 'Note: repo-local .git/ai directories are not tracked and were not removed.'
if (-not $Purge) {
    Write-Host 'Your config and local attribution data in ~/.git-ai are kept.'
    # $MyInvocation.MyCommand.Path is empty when the script runs via 'irm | iex'.
    $scriptPath = $MyInvocation.MyCommand.Path
    if ($scriptPath) {
        Write-Host "Remove with: $scriptPath -Yes -Purge"
    } else {
        Write-Host 'Remove with: irm https://usegitai.com/uninstall.ps1 | iex'
    }
}
