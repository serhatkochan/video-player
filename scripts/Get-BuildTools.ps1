[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$tools = Join-Path $repo 'tools'
New-Item -ItemType Directory -Force -Path $tools | Out-Null
$archive = Join-Path $tools 'nsis-3.11.zip'
$expected = 'c7d27f780ddb6cffb4730138cd1591e841f4b7edb155856901cdf5f214394fa1'
if (!(Test-Path -LiteralPath $archive)) {
    # Tauri mirrors the byte-identical official SourceForge distribution.
    Invoke-WebRequest -Uri 'https://github.com/tauri-apps/binary-releases/releases/download/nsis-3.11/nsis-3.11.zip' -OutFile $archive
}
if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expected) { throw 'NSIS compiler checksum mismatch' }
if (!(Test-Path -LiteralPath (Join-Path $tools 'nsis-3.11/makensis.exe'))) {
    & tar.exe -xf $archive -C $tools
    if ($LASTEXITCODE -ne 0) { throw 'Could not extract NSIS compiler' }
}
Write-Host 'Portable NSIS 3.11 matches the official SourceForge distribution SHA-256.'
