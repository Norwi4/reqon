param([string]$Destination,[ValidateSet('gui','cli')][string]$Binary='gui')
$ErrorActionPreference = 'Stop'
$reqonRoot = Split-Path -Parent $PSScriptRoot
$binaryName = if ($Binary -eq 'cli') { 'reqon-cli.exe' } else { 'Reqon.exe' }
$reqonBinary = Join-Path $reqonRoot ("target\release\" + $binaryName)
if (-not (Test-Path -LiteralPath $reqonBinary)) {
    throw 'Build first: .\cargo-local.ps1 build --release --locked'
}
if (-not $Destination) { $Destination = Join-Path $reqonRoot $binaryName }
$Destination = [System.IO.Path]::GetFullPath($Destination)
$stagedBinary = Join-Path (Split-Path -Parent $Destination) ('reqon-package-' + [guid]::NewGuid().ToString('N') + '.exe')
try {
    Copy-Item -LiteralPath $reqonBinary -Destination $stagedBinary
    & (Join-Path $PSScriptRoot 'embed-windows-icon.ps1') -Executable $stagedBinary -Icon (Join-Path $reqonRoot 'assets\brand\circular-v1\windows\reqon.ico')
    $versionMatch = [regex]::Match((Get-Content -LiteralPath (Join-Path $reqonRoot 'Cargo.toml') -Raw),'(?m)^version\s*=\s*"([0-9.]+)"')
    if (-not $versionMatch.Success) { throw 'Package version is missing.' }
    $description = if ($Binary -eq 'cli') { 'Reqon CLI collection runner' } else { 'Reqon' }
    & (Join-Path $PSScriptRoot 'embed-windows-version.ps1') -Executable $stagedBinary -Version $versionMatch.Groups[1].Value -Description $description -OriginalFilename $binaryName
    # The updater copies this EXE as Reqon-update-helper.exe. Without an explicit
    # execution level Windows can classify that filename as an elevated installer.
    & (Join-Path $PSScriptRoot 'embed-windows-manifest.ps1') -Executable $stagedBinary
    Copy-Item -LiteralPath $stagedBinary -Destination $Destination -Force
} finally {
    if (Test-Path -LiteralPath $stagedBinary) { Remove-Item -LiteralPath $stagedBinary }
}
Get-FileHash -LiteralPath $Destination -Algorithm SHA256
