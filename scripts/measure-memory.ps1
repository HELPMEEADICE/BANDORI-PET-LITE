<#
Measure a release executable using an isolated copy of the supplied config.
The real models and configuration are never changed. Close any running pet first
because the production executable uses a process-wide single-instance mutex.
#>
param(
    [string]$Executable = "$PSScriptRoot\..\target\release\bandori-pet.exe",
    [string]$ConfigPath = "$PSScriptRoot\..\run\config.json",
    [string]$ModelsRoot = "$PSScriptRoot\..\run\models",
    [string]$Label = 'memory',
    [ValidateRange(10, 60)][int]$Seconds = 16
)

$ErrorActionPreference = 'Stop'
$Executable = (Resolve-Path -LiteralPath $Executable).Path
$ConfigPath = (Resolve-Path -LiteralPath $ConfigPath).Path
$ModelsRoot = (Resolve-Path -LiteralPath $ModelsRoot).Path
if ($Label -notmatch '^[a-zA-Z0-9_-]+$') { throw 'Label must contain only letters, numbers, _ or -' }
$workspace = (Resolve-Path -LiteralPath "$PSScriptRoot\..").Path
$instanceRoot = Join-Path $workspace "run\memory-qa\$Label-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path "$instanceRoot\models" -Force | Out-Null
Copy-Item -LiteralPath $ConfigPath -Destination "$instanceRoot\config.json"
foreach ($archive in Get-ChildItem -LiteralPath $ModelsRoot -Filter '*.zst') {
    $target = Join-Path "$instanceRoot\models" $archive.Name
    try {
        New-Item -ItemType HardLink -Path $target -Target $archive.FullName | Out-Null
    } catch {
        Copy-Item -LiteralPath $archive.FullName -Destination $target
    }
}

$oldRoot = $env:BANDORI_PET_ROOT
$oldExit = $env:BANDORI_PET_EXIT_AFTER_SECS
$process = $null
try {
    $env:BANDORI_PET_ROOT = $instanceRoot
    $env:BANDORI_PET_EXIT_AFTER_SECS = "$Seconds"
    $process = Start-Process -FilePath $Executable -WorkingDirectory $workspace -WindowStyle Hidden -PassThru
    $samples = @()
    $elapsed = [Diagnostics.Stopwatch]::StartNew()
    while (!$process.HasExited) {
        $process.Refresh()
        $samples += [pscustomobject]@{
            Seconds = [math]::Round($elapsed.Elapsed.TotalSeconds, 2)
            WorkingMiB = [math]::Round($process.WorkingSet64 / 1MB, 2)
            PrivateMiB = [math]::Round($process.PrivateMemorySize64 / 1MB, 2)
            PeakWorkingMiB = [math]::Round($process.PeakWorkingSet64 / 1MB, 2)
        }
        if ($elapsed.Elapsed.TotalSeconds -gt $Seconds + 15) { throw 'measurement timed out' }
        Start-Sleep -Milliseconds 500
    }
    $samples | ConvertTo-Json | Set-Content "$instanceRoot\samples.json"
    $stable = @($samples | Where-Object Seconds -GE 5)
    if ($stable.Count -lt 8 -or $process.ExitCode -ne 0) {
        throw "Application exited early (code $($process.ExitCode)); check $instanceRoot\logs and close any existing pet instance."
    }
    $working = @($stable.WorkingMiB | Sort-Object)
    $private = @($stable.PrivateMiB | Sort-Object)
    [pscustomobject]@{
        Label = $Label
        WorkingMiB = $working[[int][math]::Floor($working.Count / 2)]
        PrivateMiB = $private[[int][math]::Floor($private.Count / 2)]
        PeakWorkingMiB = ($samples.PeakWorkingMiB | Measure-Object -Maximum).Maximum
        Output = $instanceRoot
    } | ConvertTo-Json | Tee-Object -FilePath "$instanceRoot\result.json"
} finally {
    if ($process -and !$process.HasExited) { $process.Kill(); $process.WaitForExit() }
    $env:BANDORI_PET_ROOT = $oldRoot
    $env:BANDORI_PET_EXIT_AFTER_SECS = $oldExit
}
