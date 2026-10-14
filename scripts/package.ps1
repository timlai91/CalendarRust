[CmdletBinding()]
param([switch]$SkipChecks)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Push-Location $root
try {
    if (!$SkipChecks) {
        cargo fmt --all --check
        if ($LASTEXITCODE) { throw 'Formatting check failed' }
        cargo test --locked --quiet
        if ($LASTEXITCODE) { throw 'Tests failed' }
        cargo clippy --locked --all-targets -- -D warnings
        if ($LASTEXITCODE) { throw 'Clippy failed' }
        cargo run --locked --quiet --bin verify-calendar -- 'assets\calendar-data' 'tests\fixtures\hko-calendar.json.gz' 'tests\fixtures\comparison-report.json'
        if ($LASTEXITCODE) { throw 'Calendar reference comparison failed' }
    }
    & (Join-Path $PSScriptRoot 'collect-licenses.ps1')
    cargo build --release --locked --target x86_64-pc-windows-msvc --bin CalendarRust
    if ($LASTEXITCODE) { throw 'Release build failed' }
    $metadata = cargo metadata --locked --no-deps --format-version 1 | ConvertFrom-Json
    if ($LASTEXITCODE) { throw 'Metadata failed' }
    $version = ($metadata.packages | Where-Object name -eq 'calendar-rust').version
    $name = "CalendarRust-$version-windows-x64"
    $destination = Join-Path $root "dist\$name"
    if (Test-Path -LiteralPath $destination) { throw "Preserving existing release directory: $destination" }
    New-Item -ItemType Directory -Path $destination -Force | Out-Null
    $exe = Join-Path $root 'target\x86_64-pc-windows-msvc\release\CalendarRust.exe'
    $stream = [IO.File]::OpenRead($exe)
    try {
        $reader = [IO.BinaryReader]::new($stream)
        $stream.Position = 0x3c
        $offset = $reader.ReadInt32()
        $stream.Position = $offset
        if ($reader.ReadUInt32() -ne 0x4550 -or $reader.ReadUInt16() -ne 0x8664) { throw 'Not an x64 PE executable' }
        $stream.Position = $offset + 24
        if ($reader.ReadUInt16() -ne 0x20b) { throw 'Not PE32+' }
        $stream.Position = $offset + 24 + 68
        if ($reader.ReadUInt16() -ne 2) { throw 'Not a Windows GUI executable' }
    } finally { $stream.Dispose() }
    Copy-Item -LiteralPath $exe -Destination $destination
    foreach ($entry in @('assets','licenses','README.md')) {
        Copy-Item -LiteralPath (Join-Path $root $entry) -Destination $destination -Recurse
    }
    Copy-Item -LiteralPath 'tests\fixtures\comparison-report.json' -Destination (Join-Path $destination 'assets\calendar-data\verification-report.json')
    $hashes = @{}
    foreach ($file in Get-ChildItem -LiteralPath $destination -File -Recurse) {
        $relative = $file.FullName.Substring($destination.Length + 1)
        $hashes[$relative] = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    [IO.File]::WriteAllText((Join-Path $destination 'SHA256.json'), ($hashes | ConvertTo-Json -Depth 3), [Text.UTF8Encoding]::new($false))
    $zip = Join-Path $root "dist\$name.zip"
    Compress-Archive -LiteralPath $destination -DestinationPath $zip
    Write-Output "Windows x64 GUI release (unsigned): $zip"
} finally { Pop-Location }
