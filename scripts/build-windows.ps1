[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [ValidatePattern('^v\d+\.\d+\.\d+([+-][0-9A-Za-z.-]+)?$')]
    [string]$Version,

    [Parameter(Position = 1)]
    [ValidateSet('true', 'false')]
    [string]$CreateUpdaterArtifacts = 'false'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Assert-Command([string]$Name) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "Required command was not found: $Name"
    }
}

function Invoke-Native([string]$Command, [string[]]$Arguments) {
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed with exit code ${LASTEXITCODE}: $Command $($Arguments -join ' ')"
    }
}

if ($env:OS -ne 'Windows_NT') {
    throw 'This script can only run on Windows.'
}

foreach ($command in @('git', 'node', 'npm', 'cargo')) {
    Assert-Command $command
}

$createUpdater = $CreateUpdaterArtifacts -eq 'true'
if ($createUpdater -and [string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY)) {
    throw 'Updater artifacts were requested, but TAURI_SIGNING_PRIVATE_KEY is not set.'
}

$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Set-Location $repositoryRoot
if (-not (Test-Path -LiteralPath '.git' -PathType Container)) {
    throw "Not a Git repository: $repositoryRoot"
}

Write-Host "Fetching tag: $Version"
Invoke-Native 'git' @('fetch', '--force', '--tags', 'origin')
Invoke-Native 'git' @('rev-parse', '--verify', '--quiet', "refs/tags/$Version^{commit}")

Write-Warning 'All uncommitted changes, untracked files, and ignored build files in this repository will be permanently deleted.'
Invoke-Native 'git' @('reset', '--hard', 'HEAD')
Invoke-Native 'git' @('clean', '-fdx')
Invoke-Native 'git' @('checkout', '--detach', '--force', $Version)
Invoke-Native 'git' @('reset', '--hard', $Version)

$expectedVersion = $Version.Substring(1)
$package = Get-Content -LiteralPath 'package.json' -Raw | ConvertFrom-Json
if ($package.version -ne $expectedVersion) {
    throw "Version mismatch: tag is $Version but package.json is $($package.version)"
}

Write-Host "Installing locked dependencies for $Version"
Invoke-Native 'npm' @('ci')
Invoke-Native 'npm' @('test')

$temporaryConfig = $null
try {
    $arguments = @('run', 'tauri', '--', 'build', '--bundles', 'nsis')
    if (-not $createUpdater) {
        $temporaryConfig = Join-Path ([IO.Path]::GetTempPath()) "gitsynctools-build-$PID.json"
        $json = '{"bundle":{"createUpdaterArtifacts":false}}'
        [IO.File]::WriteAllText($temporaryConfig, $json, [Text.UTF8Encoding]::new($false))
        $arguments += @('--config', $temporaryConfig)
    }

    Write-Host "Building Windows $Version (updater artifacts: $createUpdater)"
    Invoke-Native 'npm' $arguments
}
finally {
    if ($temporaryConfig -and (Test-Path -LiteralPath $temporaryConfig)) {
        Remove-Item -LiteralPath $temporaryConfig -Force
    }
}

$installer = Get-ChildItem -Path 'src-tauri\target\release\bundle\nsis' -Filter '*.exe' |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First 1
if ($installer) {
    Write-Host "Build completed: $($installer.FullName)"
} else {
    Write-Host 'Build completed. Check src-tauri\target\release\bundle\nsis.'
}
