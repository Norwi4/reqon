param([Parameter(ValueFromRemainingArguments=$true)][string[]]$CargoArgs)
$ErrorActionPreference = 'Stop'
# Prefer an existing installation. This fallback was prepared for the experiment;
# it changes this process environment only and never edits user/system PATH.
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $toolRoot = Join-Path $env:LOCALAPPDATA 'Temp\reqon-rust-tools'
    $cargoBin = Join-Path $toolRoot 'cargo\bin'
    if (-not (Test-Path -LiteralPath (Join-Path $cargoBin 'cargo.exe'))) {
        throw 'Install Rust and a C compiler, then run cargo directly. See README.md.'
    }
    $env:CARGO_HOME = Join-Path $toolRoot 'cargo'
    $env:RUSTUP_HOME = Join-Path $toolRoot 'rustup'
    $llvm = Get-ChildItem -LiteralPath $toolRoot -Directory -Filter 'llvm-mingw-*-ucrt-x86_64' | Sort-Object Name -Descending | Select-Object -First 1
    if (-not $llvm) { throw 'The local LLVM MinGW compiler is missing.' }
    $compilerBin = Join-Path $llvm.FullName 'bin'
    $rustTools = Join-Path $env:RUSTUP_HOME 'toolchains\stable-x86_64-pc-windows-gnu\lib\rustlib\x86_64-pc-windows-gnu\bin\self-contained'
    $env:PATH = $cargoBin + ';' + $rustTools + ';' + $compilerBin + ';' + $env:PATH
    $env:CC = Join-Path $compilerBin 'x86_64-w64-mingw32-gcc.exe'
    $env:AR = Join-Path $compilerBin 'llvm-ar.exe'
    $dlltool = Join-Path $compilerBin 'llvm-dlltool.exe'
    # GNU ld can produce broken IAT entries when raw-dylib imports from newer
    # windows-sys crates are mixed with the bundled GNU import libraries.
    # Keep Rust's GNU CRT/libraries, use LLD for linking and LLVM for C sources.
    $linkDir = Join-Path $toolRoot 'lld-for-rust'
    New-Item -ItemType Directory -Path $linkDir -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $compilerBin 'ld.lld.exe') -Destination (Join-Path $linkDir 'ld.exe') -Force
    $env:CARGO_ENCODED_RUSTFLAGS = @('-C', ('dlltool=' + $dlltool), '-C', ('linker=' + (Join-Path $rustTools 'x86_64-w64-mingw32-gcc.exe')), '-C', 'link-self-contained=yes', '-C', ('link-arg=-B' + $linkDir)) -join [char]31
}
Push-Location $PSScriptRoot
try {
    & cargo @CargoArgs
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
} finally { Pop-Location }
