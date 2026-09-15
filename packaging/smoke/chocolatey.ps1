param(
    [Parameter(Mandatory = $true)][string]$Binary,
    [Parameter(Mandatory = $true)][ValidateSet('x64', 'arm64')][string]$Architecture
)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$binaryPath = (Resolve-Path -LiteralPath $Binary).Path
$smokeRoot = Join-Path $env:RUNNER_TEMP 'git-ai-chocolatey-smoke'
$fixturePath = Join-Path $smokeRoot 'fixtures'
$packageSource = Join-Path $smokeRoot 'packages'
$packageRoot = Join-Path $env:ChocolateyInstall 'lib/git-ai'
$shimRoot = Join-Path $env:ChocolateyInstall 'bin'
$shimPath = Join-Path $shimRoot 'git-ai.exe'
$expectedVersion = & $binaryPath --version
if ($LASTEXITCODE -ne 0) { throw 'The native binary did not start' }
if (Test-Path -LiteralPath $packageRoot) { throw 'git-ai must not already be installed' }
$originalShims = @(Get-ChildItem -LiteralPath $shimRoot -File | ForEach-Object { $_.Name })

python packaging/smoke/fixtures.py --binary $binaryPath --architecture $Architecture --output-dir $fixturePath
if ($LASTEXITCODE -ne 0) { throw 'Failed to prepare smoke-only artifacts' }
foreach ($version in @('1.2.3', '1.2.4')) {
    python packaging/managers/generate.py --repository woud420/git-ai --version $version --tag "v$version" --assets-dir $fixturePath --output-dir $packageSource
    if ($LASTEXITCODE -ne 0) { throw "Package generation failed for $version" }
}

function Get-UserState {
    $state = python packaging/smoke/user-state.py
    if ($LASTEXITCODE -ne 0) { throw 'Could not snapshot user configuration' }
    return $state
}

function Assert-Package([string]$Version) {
    $installedBinary = Join-Path $packageRoot 'tools/git-ai.exe'
    $marker = Join-Path $packageRoot 'tools/git-ai-package-manager'
    if ((Get-Content -LiteralPath $marker -Raw).Trim() -ne 'chocolatey') {
        throw 'Missing Chocolatey ownership marker'
    }
    if ((Get-FileHash -LiteralPath $installedBinary).Hash -ne (Get-FileHash -LiteralPath $binaryPath).Hash) {
        throw 'Chocolatey selected the wrong architecture payload'
    }
    $actualVersion = & $shimPath --version
    if ($LASTEXITCODE -ne 0 -or $actualVersion -ne $expectedVersion) {
        throw "The Chocolatey shim did not launch the installed binary: $actualVersion"
    }
    $listed = choco list --exact git-ai --limit-output
    if ($LASTEXITCODE -ne 0 -or $listed -notcontains "git-ai|$Version") {
        throw "Chocolatey did not register version $Version"
    }
    $newShims = @(Get-ChildItem -LiteralPath $shimRoot -File | Where-Object { $_.Name -notin $originalShims } | ForEach-Object { $_.Name })
    if ($newShims.Count -ne 1 -or $newShims[0] -ne 'git-ai.exe') {
        throw "Expected only the git-ai shim; found: $newShims"
    }
    if (Get-ChildItem -LiteralPath $packageRoot -Recurse -Filter git.exe) {
        throw 'The package must not install a Git wrapper'
    }
}

$before = Get-UserState
choco install git-ai --version=1.2.3 --source=$packageSource --yes --no-progress --limit-output
if ($LASTEXITCODE -ne 0) { throw 'Chocolatey install failed' }
if ((Get-UserState) -ne $before) { throw 'Package installation changed user integration settings' }
Assert-Package '1.2.3'

$before = Get-UserState
choco upgrade git-ai --version=1.2.4 --source=$packageSource --yes --no-progress --limit-output
if ($LASTEXITCODE -ne 0) { throw 'Chocolatey upgrade failed' }
if ((Get-UserState) -ne $before) { throw 'Package upgrade changed user integration settings' }
Assert-Package '1.2.4'

$upgradeOutput = & $shimPath upgrade --force 2>&1 | Out-String
if ($LASTEXITCODE -ne 1 -or $upgradeOutput -notmatch 'choco upgrade git-ai') {
    throw "Self-update must redirect to Chocolatey: $upgradeOutput"
}
$backgroundOutput = & $shimPath upgrade --background 2>&1 | Out-String
if ($LASTEXITCODE -ne 0 -or $backgroundOutput.Trim()) { throw 'Background self-update must quietly skip managed binaries' }

$before = Get-UserState
choco uninstall git-ai --yes --no-progress --limit-output
if ($LASTEXITCODE -ne 0) { throw 'Chocolatey uninstall failed' }
if (Test-Path -LiteralPath $shimPath) { throw 'Chocolatey left its shim installed' }
if (Test-Path -LiteralPath $packageRoot) { throw 'Chocolatey left its package installed' }
if ((Get-UserState) -ne $before) { throw 'Package removal changed user integration settings' }
