@echo off
cd /d "%~dp0"
if exist "%~dp0Reqon.exe" (
    start "" "%~dp0Reqon.exe"
    exit /b 0
)
echo Build Reqon first: powershell -File cargo-local.ps1 build --release --locked
pause
