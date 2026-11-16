[CmdletBinding()]
param([switch]$SkipBuild)
$ErrorActionPreference = 'Stop'
$reqonRoot = Split-Path $PSScriptRoot -Parent
Push-Location $reqonRoot
try {
    $version = [regex]::Match((Get-Content Cargo.toml -Raw), '(?m)^version = "([^"]+)"').Groups[1].Value
    if (-not $version) { throw 'Missing package version' }
    $image = "reqon-native-linux-build:$version"
    New-Item -ItemType Directory -Path dist -Force | Out-Null
    # Docker writes normal progress to stderr. Windows PowerShell must not turn
    # that progress into a terminating error when the build log is redirected.
    $ErrorActionPreference = 'Continue'
    & docker build -t $image -f packaging/Dockerfile.linux packaging
    $buildExit = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    if ($buildExit -ne 0) { throw 'Linux build image failed' }
    $command = if ($SkipBuild) { 'bash scripts/package-linux.sh /target/release/reqon /out' } else { 'cargo build --release --locked -j 6 --bin reqon && cargo build --release --locked -j 6 --no-default-features --bin reqon-cli && bash scripts/package-linux.sh /target/release/reqon /out' }
    $ErrorActionPreference = 'Continue'
    & docker run --rm --mount "type=bind,source=$reqonRoot,target=/src,readonly" --mount "type=bind,source=$reqonRoot/dist,target=/out" --mount 'type=volume,source=reqon-native-cargo,target=/cargo' --mount 'type=volume,source=reqon-native-target,target=/target' $image sh -c $command
    $packageExit = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    if ($packageExit -ne 0) { throw 'Linux build or packaging failed' }
} finally { Pop-Location }
