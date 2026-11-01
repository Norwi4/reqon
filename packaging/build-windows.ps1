$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
python -m pip install -r requirements.txt
if ($LASTEXITCODE -ne 0) { throw 'Dependency installation failed.' }
python -m PyInstaller --noconfirm --clean --windowed --collect-data grpc_tools --name Reqon --add-data 'assets/fonts;assets/fonts' --copy-metadata PySocks --copy-metadata httpcore --copy-metadata jsonschema --collect-data jsonschema_specifications --icon Reqon_Icon_Pack_Approved/windows/reqon.ico --version-file packaging/reqon-version-info.txt --add-data 'Reqon_Icon_Pack_Approved/windows/reqon.ico;Reqon_Icon_Pack_Approved/windows' --add-data 'Reqon_Icon_Pack_Approved/png/transparent;Reqon_Icon_Pack_Approved/png/transparent' --add-data 'reqon-ui-states v2/dark;reqon-ui-states v2/dark' --add-data 'reqon-ui-states v2/manifest.json;reqon-ui-states v2' main.py
if ($LASTEXITCODE -ne 0) { throw 'Application build failed.' }
Compress-Archive -Path dist/Reqon -DestinationPath dist/Reqon-windows.zip -Force
if ($LASTEXITCODE -ne 0) { throw 'Portable archive creation failed.' }
$reqonVersion = python -c 'from ui_brand import VERSION; print(VERSION)'
if ($LASTEXITCODE -ne 0 -or $reqonVersion -notmatch '^\d+\.\d+\.\d+$') { throw 'Could not read the application version.' }
Copy-Item -LiteralPath dist/Reqon-windows.zip -Destination "dist/Reqon-$reqonVersion-windows-x86_64.zip" -Force
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
& $iscc packaging/reqon.iss
if ($LASTEXITCODE -ne 0) { throw "ISCC failed: $LASTEXITCODE" }
