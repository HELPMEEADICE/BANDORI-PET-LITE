[CmdletBinding()]
param(
    [ValidateSet('x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc')]
    [string]$Target = 'x86_64-pc-windows-msvc',
    [ValidatePattern('^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$')]
    [Parameter(Mandatory)]
    [string]$Version,
    [switch]$SkipBuild,
    [string]$InnoCompiler = $env:INNO_SETUP_COMPILER
)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$arch = $Target.Split('-')[0]
$msiArch = if ($arch -eq 'aarch64') { 'arm64' } else { 'x64' }
$upgradeCode = if ($arch -eq 'aarch64') { '550F4E6E-8739-4512-A161-8F557108E6A7' } else { '4EC945EC-28D3-4ED3-9396-74109E46371C' }
$msiVersion = $Version.Split('-')[0]
$parts = $msiVersion.Split('.')
if ([int]$parts[0] -gt 255 -or [int]$parts[1] -gt 255 -or [int]$parts[2] -gt 65535) {
    throw 'Version exceeds MSI limits (255.255.65535)'
}
if (-not $InnoCompiler) {
    $InnoCompiler = Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6/ISCC.exe'
}
if (-not (Test-Path -LiteralPath $InnoCompiler -PathType Leaf)) { throw "Missing Inno Setup 6 compiler: $InnoCompiler" }
if (-not (Get-Command wix -ErrorAction SilentlyContinue)) { throw 'Install WiX 4: dotnet tool install --global wix --version 4.0.6' }

Push-Location $root
try {
    if (-not $SkipBuild) {
        # Keep the CRT in the executables, including ARM64 portable builds.
        $previousRustFlags = $env:RUSTFLAGS
        try {
            $env:RUSTFLAGS = "$previousRustFlags -C target-feature=+crt-static".Trim()
            cargo build --release --locked --target $Target --bin bandori-pet --bin bandori-ui
            if ($LASTEXITCODE -ne 0) { throw 'Cargo build failed' }
        } finally { $env:RUSTFLAGS = $previousRustFlags }
    }
    $targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
    $binDir = Join-Path $targetDir "$Target/release"
    foreach ($binary in @('bandori-pet.exe', 'bandori-ui.exe')) {
        $binaryPath = Join-Path $binDir $binary
        if (-not (Test-Path -LiteralPath $binaryPath -PathType Leaf)) { throw "Missing executable: $binaryPath" }
        $reader = [IO.BinaryReader]::new([IO.File]::OpenRead($binaryPath))
        try {
            if ($reader.ReadUInt16() -ne 0x5A4D) { throw "Not a Windows executable: $binaryPath" }
            $reader.BaseStream.Position = 0x3C
            $reader.BaseStream.Position = $reader.ReadUInt32()
            if ($reader.ReadUInt32() -ne 0x00004550) { throw "Invalid PE header: $binaryPath" }
            $expectedMachine = if ($arch -eq 'aarch64') { 0xAA64 } else { 0x8664 }
            if ($reader.ReadUInt16() -ne $expectedMachine) { throw "Executable architecture does not match $Target : $binaryPath" }
        } finally { $reader.Dispose() }
    }
    $output = Join-Path $root "dist/windows-$Target-release"
    $packages = Join-Path $output 'packages'
    $staging = Join-Path $output ([guid]::NewGuid().ToString())
    $payload = Join-Path $staging 'BANDORI-PET-LITE'
    New-Item -ItemType Directory -Force -Path "$payload/assets/icons", $packages | Out-Null
    try {
        Copy-Item -LiteralPath "$binDir/bandori-pet.exe", "$binDir/bandori-ui.exe", "$root/LICENSE", "$root/NOTICE.md" -Destination $payload
        Copy-Item -LiteralPath "$root/assets/band.json", "$root/assets/outfit.json" -Destination "$payload/assets"
        Copy-Item -LiteralPath "$root/assets/icons/bandori.png", "$root/assets/icons/bandori.ico" -Destination "$payload/assets/icons"
        $name = "BANDORI-PET-LITE-$Version-windows-$arch"
        Compress-Archive -LiteralPath $payload -DestinationPath "$packages/$name.zip" -Force
        & $InnoCompiler "/DAppVersion=$Version" "/DAppArch=$arch" "/DSourceDir=$root" "/DPayloadDir=$payload" "/DOutputDir=$packages" "/DPackageName=$name" "$root/packaging/windows/setup.iss"
        if ($LASTEXITCODE -ne 0) { throw 'Inno Setup compilation failed' }
        wix build "$root/packaging/windows/product.wxs" -arch $msiArch -d "MsiVersion=$msiVersion" -d "AppArch=$arch" -d "UpgradeCode=$upgradeCode" -d "SourceDir=$root" -d "PayloadDir=$payload" -out "$packages/$name.msi" -pdbtype none
        if ($LASTEXITCODE -ne 0) { throw 'WiX compilation failed' }
        # Same-version upgrades are intentional; the package only supports per-user installs.
        wix msi validate "$packages/$name.msi" -sice ICE61 -sice ICE91
        if ($LASTEXITCODE -ne 0) { throw 'MSI validation failed' }
        foreach ($suffix in @('.zip', '.msi', '-setup.exe')) {
            if (-not (Test-Path -LiteralPath "$packages/$name$suffix" -PathType Leaf)) { throw "Missing package: $name$suffix" }
        }
        Write-Host "Packages: $packages"
    } finally {
        # This GUID directory was created above; never delete the user's install/data directory.
        $resolvedStaging = [IO.Path]::GetFullPath($staging)
        $resolvedOutput = [IO.Path]::GetFullPath($output) + [IO.Path]::DirectorySeparatorChar
        if (-not $resolvedStaging.StartsWith($resolvedOutput, [StringComparison]::OrdinalIgnoreCase)) { throw 'Staging path is outside the package output directory' }
        Remove-Item -LiteralPath $resolvedStaging -Recurse -Force
    }
} finally { Pop-Location }
