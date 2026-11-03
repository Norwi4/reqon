param([Parameter(Mandatory = $true)][string]$IsccPath)

# Exercise the real installer/uninstaller against an isolated registry key.
# The real user and machine PATH values must remain byte-for-byte unchanged.
$ErrorActionPreference = 'Stop'
$runnerRoot = Split-Path $PSScriptRoot -Parent
$testId = [Guid]::NewGuid().ToString('N')
$testRoot = Join-Path $runnerRoot "build\installer-tests\$testId"
$installDir = Join-Path $testRoot 'CLI with spaces'
$testKey = "Software\Reqon\InstallerTests\$testId"
$environmentKey = "$testKey\Environment"
$stateKey = "$testKey\State"
$appId = "Reqon.CLI.Test.$testId"
$originalUserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$originalMachinePath = [Environment]::GetEnvironmentVariable('Path', 'Machine')
New-Item -ItemType Directory -Path $testRoot -Force | Out-Null
$environment = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($environmentKey)
$env:REQON_INSTALL_TEST_DIR = $installDir

function Assert-Equal($Actual, $Expected, [string]$Message) {
    if ($Actual -cne $Expected) { throw "$Message`nExpected: $Expected`nActual: $Actual" }
}

function Read-TestPath {
    $environment.GetValue('Path', $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
}

function Invoke-Installer([string]$Executable, [string[]]$Arguments) {
    # -Wait waits for the uninstaller's child process as well as its launcher.
    $process = Start-Process -FilePath $Executable -ArgumentList $Arguments -WindowStyle Hidden -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "Installer failed with exit code $($process.ExitCode): $Executable" }
}

function Install-Test([string]$Label) {
    Invoke-Installer (Join-Path $testRoot 'reqon-cli-installer-check.exe') @(
        '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/LANG=english',
        "/DIR=`"$installDir`"", "/LOG=`"$testRoot\$Label-install.log`""
    )
}

function Uninstall-Test([string]$Label) {
    $registration = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey("Software\Microsoft\Windows\CurrentVersion\Uninstall\${appId}_is1")
    if (-not $registration) { throw 'Test uninstall registration is missing.' }
    try { $uninstaller = $registration.GetValue('UninstallString').Trim('"') } finally { $registration.Dispose() }
    if ([IO.Path]::GetDirectoryName($uninstaller) -ne $installDir) { throw 'Uninstaller is outside the isolated test directory.' }
    Invoke-Installer $uninstaller @(
        '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/LOG=`"$testRoot\$Label-uninstall.log`""
    )
    if (Test-Path -LiteralPath (Join-Path $installDir 'reqon.exe')) { throw 'Uninstall left reqon.exe behind.' }
}

try {
    & $IsccPath "/DAppId=$appId" "/DEnvironmentKey=$environmentKey" "/DStateKey=$stateKey" "/O$testRoot" '/Freqon-cli-installer-check' (Join-Path $runnerRoot 'packaging\reqon-cli.iss')
    if ($LASTEXITCODE -ne 0) { throw 'Test installer compilation failed.' }

    $baseline = 'C:\Existing Tools;;%SystemRoot%\System32;C:\Other;'
    $environment.SetValue('Path', $baseline, [Microsoft.Win32.RegistryValueKind]::ExpandString)
    Install-Test 'fresh'
    Assert-Equal (Read-TestPath) ($baseline + ';' + $installDir) 'Fresh install did not append to PATH correctly.'
    Assert-Equal ($environment.GetValueKind('Path')) ([Microsoft.Win32.RegistryValueKind]::ExpandString) 'PATH registry type changed.'
    $version = & (Join-Path $installDir 'reqon.exe') --version
    Assert-Equal $version 'reqon-runner-prototype 0.14.1' 'Installed binary did not run.'
    Install-Test 'upgrade'
    Assert-Equal (Read-TestPath) ($baseline + ';' + $installDir) 'Upgrade duplicated PATH.'
    $environment.SetValue('Path', (Read-TestPath) + ';D:\Added Later', [Microsoft.Win32.RegistryValueKind]::ExpandString)
    Uninstall-Test 'owned'
    Assert-Equal (Read-TestPath) ($baseline + ';D:\Added Later') 'Uninstall lost unrelated or subsequently added entries.'
    Write-Output 'PASS: install, upgrade, uninstall, unrelated entries, REG_EXPAND_SZ.'

    $baseline = 'C:\Tools;"%REQON_INSTALL_TEST_DIR%\";D:\Other'
    $environment.SetValue('Path', $baseline, [Microsoft.Win32.RegistryValueKind]::ExpandString)
    Install-Test 'preexisting'
    Assert-Equal (Read-TestPath) $baseline 'An equivalent pre-existing PATH entry was duplicated.'
    Uninstall-Test 'preexisting'
    Assert-Equal (Read-TestPath) $baseline 'Uninstall removed a pre-existing entry.'
    Write-Output 'PASS: preserve pre-existing entry with environment variable, quotes and trailing slash.'

    $environment.SetValue('Path', '', [Microsoft.Win32.RegistryValueKind]::String)
    Install-Test 'empty'
    Assert-Equal (Read-TestPath) $installDir 'Empty PATH was not handled.'
    Assert-Equal ($environment.GetValueKind('Path')) ([Microsoft.Win32.RegistryValueKind]::String) 'REG_SZ was not preserved.'
    Uninstall-Test 'empty'
    Assert-Equal (Read-TestPath) '' 'Uninstall did not remove the sole owned entry.'
    Write-Output 'PASS: empty PATH and REG_SZ.'

    $environment.DeleteValue('Path')
    Install-Test 'missing'
    Assert-Equal (Read-TestPath) $installDir 'Missing PATH was not handled.'
    Uninstall-Test 'missing'
    Assert-Equal (Read-TestPath) '' 'Uninstall did not clear the new PATH entry.'
    Write-Output 'PASS: missing PATH.'
} finally {
    if (Test-Path -LiteralPath (Join-Path $installDir 'reqon.exe')) { Uninstall-Test 'cleanup' }
    $environment.Dispose()
    [Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree($testKey, $false)
    Remove-Item Env:\REQON_INSTALL_TEST_DIR -ErrorAction SilentlyContinue
    Assert-Equal ([Environment]::GetEnvironmentVariable('Path', 'User')) $originalUserPath 'Real user PATH was changed by the test.'
    Assert-Equal ([Environment]::GetEnvironmentVariable('Path', 'Machine')) $originalMachinePath 'Machine PATH was changed by the test.'
}
Write-Output "Installer checks passed. Logs: $testRoot"
