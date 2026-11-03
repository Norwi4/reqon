$ErrorActionPreference = 'Stop'
Push-Location (Join-Path $PSScriptRoot '..')
try {
    & .\cargo-local.ps1 build --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed' }
    & .\cargo-local.ps1 test --locked
    if ($LASTEXITCODE -ne 0) { throw 'Rust tests failed' }
    python -m unittest discover -s tests -p 'test_*.py' -v
    if ($LASTEXITCODE -ne 0) { throw 'Acceptance failed' }
    New-Item -ItemType Directory -Path dist -Force | Out-Null
    Copy-Item -LiteralPath target/release/reqon-prototype.exe -Destination dist/reqon.exe
    $runnerVersion = & .\dist\reqon.exe --version
    if ($runnerVersion -ne 'reqon-runner-prototype 0.14.1') { throw 'Unexpected runner version' }
    $runnerHash = (Get-FileHash -LiteralPath dist/reqon.exe -Algorithm SHA256).Hash.ToLowerInvariant()
    "$runnerHash  reqon.exe" | Set-Content -Encoding ascii dist/SHA256SUMS
    & (Join-Path $PSScriptRoot 'package-windows.ps1')
} finally { Pop-Location }
