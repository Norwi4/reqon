param([Parameter(ValueFromRemainingArguments=$true)][string[]]$CargoArgs)
$ErrorActionPreference = 'Stop'
# The bundled SQLite only needs the core Win32 headers.
if (-not $env:LIBSQLITE3_FLAGS) { $env:LIBSQLITE3_FLAGS = '-DWIN32_LEAN_AND_MEAN' }
# Prefer an existing installation. The optional local fallback changes this
# process environment only and never edits user/system PATH.
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $toolRoot = Join-Path $env:LOCALAPPDATA 'ReqonBuild'
    $reqonToolchain = '1.99.1-x86_64-pc-windows-gnu'
    if (-not (Test-Path -LiteralPath (Join-Path $toolRoot 'rustup\toolchains\1.99.1-x86_64-pc-windows-gnu'))) {
        $toolRoot = Join-Path $env:LOCALAPPDATA 'Temp\reqon-rust-tools'
        $reqonToolchain = 'stable-x86_64-pc-windows-gnu'
    }
    $cargoBin = Join-Path $toolRoot 'cargo\bin'
    if (-not (Test-Path -LiteralPath (Join-Path $cargoBin 'cargo.exe'))) {
        $cargoBin = Join-Path $toolRoot ('rustup\toolchains\' + $reqonToolchain + '\bin')
    }
    if (-not (Test-Path -LiteralPath (Join-Path $cargoBin 'cargo.exe'))) {
        throw 'Install Rust and a C compiler, then run cargo directly. See README.md.'
    }
    $env:CARGO_HOME = Join-Path $PSScriptRoot '.cargo-home'
    $env:RUSTUP_HOME = Join-Path $toolRoot 'rustup'
    $env:RUSTUP_TOOLCHAIN = $reqonToolchain
    $llvm = Get-ChildItem -LiteralPath $toolRoot -Directory -Filter 'llvm-mingw-*-ucrt-x86_64' | Sort-Object Name -Descending | Select-Object -First 1
    if (-not $llvm) { throw 'The local LLVM MinGW compiler is missing.' }
    $compilerBin = Join-Path $llvm.FullName 'bin'
    $rustTools = Join-Path $env:RUSTUP_HOME ('toolchains\' + $reqonToolchain + '\lib\rustlib\x86_64-pc-windows-gnu\bin\self-contained')
    $env:PATH = $cargoBin + ';' + $rustTools + ';' + $compilerBin + ';' + $env:PATH
    $env:CC = (Join-Path $compilerBin 'x86_64-w64-mingw32-gcc.exe').Replace('\','/')
    $env:AR = (Join-Path $compilerBin 'llvm-ar.exe').Replace('\','/')
    $reqonMake = Join-Path $compilerBin 'mingw32-make.exe'
    if (Test-Path -LiteralPath $reqonMake) {
        # openssl-src invokes literal `make`, not the MAKE environment variable.
        $reqonMakeBin = Join-Path $toolRoot 'make-for-rust'
        New-Item -ItemType Directory -Path $reqonMakeBin -Force | Out-Null
        $reqonMakeAlias = Join-Path $reqonMakeBin 'make.exe'
        if (-not (Test-Path -LiteralPath $reqonMakeAlias)) {
            Copy-Item -LiteralPath $reqonMake -Destination $reqonMakeAlias
        }
        $env:PATH = $reqonMakeBin + ';' + $env:PATH
        # openssl-src does not forward Cargo's jobserver on Windows.
        if (-not $env:MAKEFLAGS) { $env:MAKEFLAGS = '-j4' }
    }
    # The GNU OpenSSL build needs an MSYS Perl (Unix paths). Git's compact Perl
    # omits Locale::Maketext::Simple; the optional local module directory supplies
    # that pure-Perl module without modifying Git or the system installation.
    $reqonPerl = Join-Path $env:ProgramFiles 'Git\usr\bin\perl.exe'
    $reqonPerlLibrary = Join-Path $toolRoot 'perl-build-lib'
    if (Test-Path -LiteralPath $reqonPerl) {
        # OpenSSL embeds this in Makefile shell commands without quoting.
        $env:PERL = 'perl'
        if (Test-Path -LiteralPath $reqonPerlLibrary) {
            $reqonPerlPosix = '/' + $reqonPerlLibrary.Substring(0,1).ToLowerInvariant() + '/' + $reqonPerlLibrary.Substring(3).Replace('\','/')
            $env:PERL5LIB = $reqonPerlPosix
            $env:MSYS2_ENV_CONV_EXCL = (@($env:MSYS2_ENV_CONV_EXCL,'PERL5LIB') | Where-Object { $_ }) -join ';'
        }
        $env:PATH += ';' + (Split-Path -Parent $reqonPerl)
    }
    $dlltool = Join-Path $compilerBin 'llvm-dlltool.exe'
    # GNU ld can produce broken IAT entries when raw-dylib imports from newer
    # windows-sys crates are mixed with the bundled GNU import libraries.
    # Keep Rust's GNU CRT/libraries, use LLD for linking and LLVM for C sources.
    $linkDir = Join-Path $toolRoot 'lld-for-rust'
    New-Item -ItemType Directory -Path $linkDir -Force | Out-Null
    $reqonLldSource = Join-Path $compilerBin 'ld.lld.exe'
    $reqonLldTarget = Join-Path $linkDir 'ld.exe'
    # The engine and desktop can compile independently. Do not rewrite a shared
    # linker executable while the other Cargo build is using it.
    if (-not (Test-Path -LiteralPath $reqonLldTarget) -or
        (Get-FileHash -LiteralPath $reqonLldSource).Hash -ne (Get-FileHash -LiteralPath $reqonLldTarget).Hash) {
        Copy-Item -LiteralPath $reqonLldSource -Destination $reqonLldTarget -Force
    }
    $env:CARGO_ENCODED_RUSTFLAGS = @('-C', ('dlltool=' + $dlltool), '-C', ('linker=' + (Join-Path $rustTools 'x86_64-w64-mingw32-gcc.exe')), '-C', 'link-self-contained=yes', '-C', ('link-arg=-B' + $linkDir)) -join [char]31
}
Push-Location $PSScriptRoot
try {
    # Cargo writes ordinary progress to stderr. Windows PowerShell wraps it in
    # ErrorRecord when callers capture 2>&1; it must not terminate the build.
    $ErrorActionPreference = 'Continue'
    & cargo @CargoArgs
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
} finally { Pop-Location }
