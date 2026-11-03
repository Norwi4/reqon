param([string]$IsccPath)

$ErrorActionPreference = 'Stop'
$runnerRoot = Split-Path $PSScriptRoot -Parent
$runnerBinary = Join-Path $runnerRoot 'dist\reqon.exe'
$manifest = Get-Content -LiteralPath (Join-Path $runnerRoot 'Cargo.toml') -Raw
if ($manifest -notmatch '(?m)^version\s*=\s*"(\d+\.\d+\.\d+)"') { throw 'Could not read the runner version.' }
$runnerVersion = $Matches[1]
if (-not (Test-Path -LiteralPath $runnerBinary)) { throw 'Build dist/reqon.exe first with ci/build-windows.ps1.' }
$binaryVersion = & $runnerBinary --version
if ($LASTEXITCODE -ne 0 -or $binaryVersion -ne "reqon-runner-prototype $runnerVersion") {
    throw 'Release executable does not match Cargo.toml.'
}

if (-not $IsccPath) { $IsccPath = (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source }
if (-not $IsccPath) {
    $candidates = @(
        "$env:LOCALAPPDATA\Programs\Inno Setup 7\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 7\ISCC.exe",
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe"
    )
    $IsccPath = $candidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
}
if (-not $IsccPath) { throw 'Inno Setup compiler (ISCC.exe) is required to build the CLI installer.' }

& $IsccPath "/DAppVersion=$runnerVersion" (Join-Path $runnerRoot 'packaging\reqon-cli.iss')
if ($LASTEXITCODE -ne 0) { throw "CLI installer compilation failed: $LASTEXITCODE" }
$installer = Join-Path $runnerRoot "dist\Reqon-CLI-Setup-$runnerVersion-win64.exe"
$checksums = foreach ($artifact in @($runnerBinary, $installer)) {
    $hash = (Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $([IO.Path]::GetFileName($artifact))"
}
$checksums | Set-Content -LiteralPath (Join-Path $runnerRoot 'dist\SHA256SUMS-WINDOWS') -Encoding ascii
Write-Output "CLI installer: $installer"
