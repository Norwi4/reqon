param([string]$Name = 'Reqon')
$ErrorActionPreference = 'Stop'
if ([string]::IsNullOrWhiteSpace($Name) -or $Name.IndexOfAny([IO.Path]::GetInvalidFileNameChars()) -ge 0) {
    throw 'Shortcut name must be a valid filename.'
}
$reqonRoot = Split-Path -Parent $PSScriptRoot
$binary = Join-Path $reqonRoot 'Reqon.exe'
if (-not (Test-Path -LiteralPath $binary)) { throw 'Run scripts/package.ps1 first.' }
$desktop = [Environment]::GetFolderPath('Desktop')
$shortcutPath = Join-Path $desktop ($Name + '.lnk')
$shell = New-Object -ComObject WScript.Shell
$shortcut = $shell.CreateShortcut($shortcutPath)
if ((Test-Path -LiteralPath $shortcutPath) -and $shortcut.TargetPath -ne $binary) {
    throw 'An unrelated shortcut already has this name. Use a different -Name.'
}
$shortcut.TargetPath = $binary
$shortcut.WorkingDirectory = $reqonRoot
$shortcut.Description = 'Reqon - native Rust API client'
# A versioned icon path also refreshes existing shortcuts whose EXE icon was
# cached by Explorer before the brand update. The EXE embeds the same ICO.
$shortcut.IconLocation = (Join-Path $reqonRoot 'assets\brand\circular-v1\windows\reqon.ico') + ',0'
$shortcut.Save()
Get-Item -LiteralPath $shortcutPath | Select-Object FullName,LastWriteTime
