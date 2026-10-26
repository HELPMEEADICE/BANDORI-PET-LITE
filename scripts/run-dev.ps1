#!/usr/bin/env pwsh
<#
.SYNOPSIS
    Builds BANDORI-PET-LITE into .\run and prepares a runnable installation.

.DESCRIPTION
    Creates .\run\ with:
      bandori-pet.exe, bandori-ui.exe   (from target\debug or target\release)
      assets\band.json, assets\outfit.json
      models\<character>.zst            (copied from -ModelsFrom)
      config.json                       (seeded for the first character)

.PARAMETER ModelsFrom
    Directory containing the Bandori `.zst` archives.

.PARAMETER Characters
    Archive stems to copy (default: kasumi, anon, asuka).

.PARAMETER Release
    Use the release build instead of the debug build.
#>
param(
    [string]$ModelsFrom = "C:\oss\BANDORI-PET-REV\models",
    [string[]]$Characters = @("kasumi", "anon", "asuka"),
    [switch]$Release
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$profile = if ($Release) { "release" } else { "debug" }
$runDir = Join-Path $root "run"

Write-Host "== build ($profile) ==" -ForegroundColor Cyan
Push-Location $root
cargo build --bin bandori-pet --bin bandori-ui $(if ($Release) { '--release' })
Pop-Location

Write-Host "== staging $runDir ==" -ForegroundColor Cyan
New-Item -ItemType Directory -Force -Path $runDir, "$runDir\assets", "$runDir\models" | Out-Null
Copy-Item "$root\target\$profile\bandori-pet.exe" $runDir -Force
Copy-Item "$root\target\$profile\bandori-ui.exe" $runDir -Force
Copy-Item "$root\assets\band.json", "$root\assets\outfit.json" "$runDir\assets" -Force

$first = $null
foreach ($character in $Characters) {
    $source = Join-Path $ModelsFrom "$character.zst"
    if (-not (Test-Path $source)) {
        Write-Warning "missing archive: $source"
        continue
    }
    if ($null -eq $first) { $first = $character }
    Copy-Item $source "$runDir\models\" -Force
}

if ($null -eq $first) {
    Write-Warning "no archives copied; start the app and pick a model in the settings window"
    exit 1
}

$configPath = Join-Path $runDir "config.json"
if (-not (Test-Path $configPath)) {
    @{
        schema_version = 1
        character      = $first
        costume        = ""
        language       = "zh_CN"
        drag_locked    = $false
        live2d_scale   = 100
        models         = @()
    } | ConvertTo-Json -Depth 6 | Set-Content $configPath -Encoding utf8
}

Write-Host ""
Write-Host "ready. start with:" -ForegroundColor Green
Write-Host "  `$env:BANDORI_PET_ROOT = '$runDir'"
Write-Host "  & '$runDir\bandori-pet.exe'"
Write-Host ""
Write-Host "the pet writes config.json on first frame; the settings window is spawned automatically."
