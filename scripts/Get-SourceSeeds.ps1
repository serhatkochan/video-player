[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$manifestPath = Join-Path $repo 'packaging/source-seeds-manifest.json'
$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
$sources = Join-Path $repo 'runtime/sources'
New-Item -ItemType Directory -Force -Path $sources | Out-Null
foreach ($asset in $manifest.assets) {
    $archive = Join-Path $sources $asset.file
    if (!(Test-Path -LiteralPath $archive)) { Invoke-WebRequest -Uri $asset.url -OutFile $archive }
    if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $asset.sha256) {
        throw "Source snapshot checksum mismatch: $($asset.id)"
    }
}
Copy-Item -LiteralPath $manifestPath -Destination (Join-Path $sources 'source-seeds-manifest.json') -Force
Write-Host 'Exact primary media source and builder snapshots verified. Full static dependency source audit remains required.'
