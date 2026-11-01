@echo off
setlocal
cd /d "%~dp0"
where node >nul 2>nul
if errorlevel 1 (
  echo Node.js 22 or newer is required.
  pause
  exit /b 1
)
node src/build.mjs
if errorlevel 1 exit /b 1
echo Open http://127.0.0.1:4173/ in your browser.
echo Press Ctrl+C to stop the website.
node server.mjs
pause
