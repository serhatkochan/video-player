[CmdletBinding()]
param([Parameter(Mandatory)][string]$AcceptanceFile, [Parameter(Mandatory)][string]$Version, [Parameter(Mandatory)][string]$StageDirectory)
$ErrorActionPreference = 'Stop'
if (!(Test-Path -LiteralPath $AcceptanceFile)) { throw 'A completed hardware/clean-Windows acceptance attestation is required for a public release' }
$report = Get-Content -LiteralPath $AcceptanceFile -Raw | ConvertFrom-Json
if ($report.version -ne $Version -or !$report.tester -or !$report.testedCommit -or !$report.testDate) { throw 'Incomplete or mismatched acceptance attestation' }
$currentCommit = (& git -C (Split-Path $PSScriptRoot -Parent) rev-parse HEAD 2>$null | Out-String).Trim()
if ($LASTEXITCODE -ne 0 -or $report.testedCommit -cnotmatch '^[a-f0-9]{40}$') { throw 'Acceptance must identify a valid source commit' }
if ($currentCommit -ne $report.testedCommit) {
    & git -C (Split-Path $PSScriptRoot -Parent) diff --quiet $report.testedCommit HEAD -- . ':(exclude)docs/acceptance/**'
    if ($LASTEXITCODE -ne 0) { throw 'Source changed after acceptance; only committing the acceptance report is allowed' }
}
& git -C (Split-Path $PSScriptRoot -Parent) diff --quiet HEAD -- . ':(exclude)docs/acceptance/**'
if ($LASTEXITCODE -ne 0) { throw 'Uncommitted source changes cannot be released using an older acceptance report' }
foreach ($case in @('cleanWindows11','vlcCoexistence','defaultAppThumbnails','codecMatrix','hdr10PhysicalDisplay','sdrToneMapping','displaySwitching','subtitleAndAudio','corruptMediaIsolation','installUpgradeUninstall','performanceMeasured')) {
    if ($report.results.$case -ne 'pass') { throw "Acceptance requirement has not passed: $case" }
}
$rustNotices = Get-Content (Join-Path $StageDirectory 'licenses/rust/dependency-index.json') -Raw | ConvertFrom-Json
if (@($rustNotices | Where-Object { $_.noticeFileCount -eq 0 }).Count) { throw 'Some Rust dependencies still lack their upstream license texts' }
$sources = Join-Path $StageDirectory 'sources'
$sourceManifest = Join-Path $sources 'corresponding-sources.json'
if (!(Test-Path -LiteralPath $sourceManifest)) { throw 'Complete corresponding source archives are required; upstream links alone are insufficient' }
$sourceReport = Get-Content -LiteralPath $sourceManifest -Raw | ConvertFrom-Json
if ($sourceReport.schemaVersion -ne 1 -or !$sourceReport.reviewedBy -or !$sourceReport.complete) { throw 'Corresponding sources have not been reviewed as complete' }
$runtimeManifest = Get-Content (Join-Path $StageDirectory 'runtime-manifest.json') -Raw | ConvertFrom-Json
foreach ($asset in $runtimeManifest.assets) {
    $entry = $sourceReport.runtimes | Where-Object id -eq $asset.id | Select-Object -First 1
    if (!$entry -or $entry.binaryArchiveSha256 -ne $asset.sha256 -or !$entry.includesAllStaticDependencies -or !$entry.includesBuildScripts -or !$entry.archives) { throw "Incomplete corresponding sources: $($asset.id)" }
    foreach ($archive in $entry.archives) {
        $path = [IO.Path]::GetFullPath((Join-Path $sources $archive.file))
        $safePrefix = [IO.Path]::GetFullPath($sources).TrimEnd('\') + '\'
        if (!$path.StartsWith($safePrefix, [StringComparison]::OrdinalIgnoreCase) -or !(Test-Path -LiteralPath $path)) { throw 'Invalid source archive path' }
        if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $archive.sha256) { throw "Source checksum mismatch: $($archive.file)" }
    }
}
Write-Host 'Public release acceptance and corresponding source gates passed.'
