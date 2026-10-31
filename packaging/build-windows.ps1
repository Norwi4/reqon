$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
python -m pip install -r requirements.txt
if ($LASTEXITCODE -ne 0) { throw 'Dependency installation failed.' }
python -m PyInstaller --noconfirm --clean --windowed --collect-data grpc_tools --name Getman main.py
if ($LASTEXITCODE -ne 0) { throw 'Application build failed.' }
Compress-Archive -Path dist/Getman -DestinationPath dist/Getman-windows.zip -Force
$iscc = (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source
if (-not $iscc) {
    $candidates = @(
        "$env:LOCALAPPDATA\Programs\Inno Setup 7\ISCC.exe",
        "${env:ProgramFiles}\Inno Setup 7\ISCC.exe",
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe"
    )
    $iscc = $candidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
}
if (-not $iscc) { throw 'Inno Setup compiler (ISCC.exe) is required to build the installer.' }
& $iscc packaging/getman.iss
if ($LASTEXITCODE -ne 0) { throw "ISCC failed: $LASTEXITCODE" }
