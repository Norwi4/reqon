param([string]$Version)
$ErrorActionPreference='Stop'
$reqonRoot=Split-Path -Parent $PSScriptRoot
if (-not $Version) {
    $Version=[regex]::Match((Get-Content (Join-Path $reqonRoot 'Cargo.toml') -Raw),'(?m)^version\s*=\s*"([0-9.]+)"').Groups[1].Value
}
if ($Version -notmatch '^\d+\.\d+\.\d+$') {throw 'Invalid release version.'}
$source=Join-Path $reqonRoot "dist\windows-$Version\Reqon"
$binary=Join-Path $source 'reqon-cli.exe'
if (-not (Test-Path -LiteralPath $binary)) {throw 'Package the Windows GUI and CLI first.'}
if ((Get-Item -LiteralPath $binary).VersionInfo.ProductVersion -ne $Version -or (& $binary --version) -ne "reqon-cli $Version") {throw 'The CLI version does not match the release.'}
$stage=Join-Path $reqonRoot "dist\cli-windows-$Version\Reqon-CLI"
New-Item -ItemType Directory -Path $stage -Force | Out-Null
$allowed=@('reqon-cli.exe','cli-runner.md','THIRD_PARTY_NOTICES.md','Release-notes.md','licenses')
foreach ($name in $allowed) {
    Copy-Item -LiteralPath (Join-Path $source $name) -Destination $stage -Recurse -Force
}
if (@(Get-ChildItem -LiteralPath $stage -Force | Where-Object {$_.Name -notin $allowed}).Count) {throw 'Unexpected files in CLI staging directory.'}
$archive=Join-Path $reqonRoot "dist\Reqon-CLI-$Version-windows-x86_64.zip"
Compress-Archive -LiteralPath $stage -DestinationPath $archive -Force
$digest=(Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
"$digest  $([IO.Path]::GetFileName($archive))" | Set-Content -LiteralPath (Join-Path $reqonRoot "dist\Reqon-CLI-$Version-SHA256SUMS.txt") -Encoding ascii
Get-Item -LiteralPath $archive | Select-Object FullName,Length
