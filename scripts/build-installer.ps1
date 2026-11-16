param([switch]$SkipBuild)
$ErrorActionPreference='Stop'
$reqonRoot=Split-Path -Parent $PSScriptRoot
Push-Location $reqonRoot
try {
    $versionMatch=[regex]::Match((Get-Content Cargo.toml -Raw),'(?m)^version\s*=\s*"([0-9.]+)"')
    if (-not $versionMatch.Success) { throw 'Missing Cargo package version.' }
    $reqonVersion=$versionMatch.Groups[1].Value
    if (-not $SkipBuild) {
        & .\cargo-local.ps1 build --release --locked --bin reqon
        if ($LASTEXITCODE -ne 0) {throw 'GUI release build failed.'}
        & .\cargo-local.ps1 build --release --locked --no-default-features --bin reqon-cli
        if ($LASTEXITCODE -ne 0) {throw 'CLI release build failed.'}
    }
    $cliBinary=Join-Path $reqonRoot 'target\release\reqon-cli.exe'
    if (-not (Test-Path -LiteralPath $cliBinary)) {throw 'Build the headless release CLI first.'}
    $cliVersion=& $cliBinary --version
    if ($LASTEXITCODE -ne 0 -or $cliVersion -ne "reqon-cli $reqonVersion") {throw 'CLI binary version does not match Cargo.toml; rebuild first.'}
    $buildDist=Join-Path $reqonRoot "dist\windows-$reqonVersion\Reqon"
    New-Item -ItemType Directory -Path $buildDist -Force | Out-Null
    # Explicit allowlist: profiles, databases and development fixtures are never packaged.
    & .\scripts\package.ps1 -Destination (Join-Path $buildDist 'Reqon.exe')
    & .\scripts\package.ps1 -Binary cli -Destination (Join-Path $buildDist 'reqon-cli.exe')
    Copy-Item -LiteralPath docs\cli-runner.md -Destination (Join-Path $buildDist 'cli-runner.md')
    Copy-Item -LiteralPath THIRD_PARTY_NOTICES.md -Destination $buildDist
    Copy-Item -LiteralPath docs\upgrade-0.20.0.md -Destination (Join-Path $buildDist 'Upgrade-0.20.0.md')
    $releaseNotes=Join-Path $reqonRoot "docs\release-$reqonVersion.md"
    if (Test-Path -LiteralPath $releaseNotes) {Copy-Item -LiteralPath $releaseNotes -Destination (Join-Path $buildDist 'Release-notes.md')}
    $licenses=Join-Path $buildDist 'licenses'
    New-Item -ItemType Directory -Path $licenses -Force | Out-Null
    Copy-Item -LiteralPath docs\dependencies.json -Destination (Join-Path $licenses 'dependencies.json')
    Copy-Item -LiteralPath assets\BRUNO-LICENSE.txt -Destination (Join-Path $licenses 'interface-MIT.txt')
    $libraryLicenses=Join-Path $licenses 'libraries'
    New-Item -ItemType Directory -Path $libraryLicenses -Force | Out-Null
    Get-ChildItem -LiteralPath assets\licenses | ForEach-Object {Copy-Item -LiteralPath $_.FullName -Destination $libraryLicenses -Recurse -Force}
    Get-ChildItem -LiteralPath assets\fonts -File | Where-Object { $_.Extension -eq '.txt' } | ForEach-Object {Copy-Item -LiteralPath $_.FullName -Destination $licenses}
    $allowed=@('Reqon.exe','reqon-cli.exe','cli-runner.md','THIRD_PARTY_NOTICES.md','Upgrade-0.20.0.md','Release-notes.md','licenses')
    $unexpected=@(Get-ChildItem -LiteralPath $buildDist -Force | Where-Object {$_.Name -notin $allowed})
    if ($unexpected.Count) {throw 'Unexpected files in distribution staging directory; packaging stopped.'}
    foreach ($name in @('Reqon.exe','reqon-cli.exe')) {
        $info=(Get-Item -LiteralPath (Join-Path $buildDist $name)).VersionInfo
        if ($info.ProductVersion -ne $reqonVersion -or $info.OriginalFilename -ne $name) {throw "Incorrect Windows version resource for $name"}
    }
    if ((& (Join-Path $buildDist 'reqon-cli.exe') --version) -ne "reqon-cli $reqonVersion") {throw 'Packaged CLI failed version check.'}
    $iscc=(Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source
    if (-not $iscc) {
        $iscc=@("$env:LOCALAPPDATA\Programs\Inno Setup 7\ISCC.exe","${env:ProgramFiles}\Inno Setup 7\ISCC.exe","${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe") | Where-Object {Test-Path -LiteralPath $_} | Select-Object -First 1
    }
    if (-not $iscc) {throw 'Install Inno Setup to build the installer.'}
    & $iscc "/DAppVersion=$reqonVersion" "/DBuildDist=$buildDist" packaging\reqon.iss
    if ($LASTEXITCODE -ne 0) {throw 'Installer compilation failed.'}
    $zip=Join-Path $reqonRoot "dist\Reqon-$reqonVersion-windows-x86_64.zip"
    # ZIP uses the same per-user profile as the installer by default.
    Compress-Archive -LiteralPath $buildDist -DestinationPath $zip -Force
    & .\scripts\package-cli.ps1 -Version $reqonVersion
    $artifacts=@($zip,(Join-Path $reqonRoot "dist\Reqon-Setup-$reqonVersion-win64.exe"),(Join-Path $reqonRoot "dist\Reqon-CLI-$reqonVersion-windows-x86_64.zip"))
    $checksums=foreach($artifact in $artifacts){"$((Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash.ToLowerInvariant())  $([IO.Path]::GetFileName($artifact))"}
    $checksums | Set-Content -LiteralPath "dist\Reqon-$reqonVersion-SHA256SUMS.txt" -Encoding ascii
    $artifacts | ForEach-Object {Get-Item -LiteralPath $_ | Select-Object FullName,Length}
} finally {Pop-Location}
