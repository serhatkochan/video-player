[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$testRoot = Join-Path $repo ('target/release-gate-tests/' + [Guid]::NewGuid().ToString('N'))
$sources = Join-Path $testRoot 'stage/sources'
New-Item -ItemType Directory -Force -Path $sources,(Join-Path $testRoot 'stage/licenses/rust') | Out-Null
$sourceFile = Join-Path $sources 'fixture-source.txt'
'Synthetic regression fixture; not a real corresponding source attestation.' | Set-Content -LiteralPath $sourceFile
$sourceHash = (Get-FileHash -LiteralPath $sourceFile -Algorithm SHA256).Hash.ToLowerInvariant()
$commit = '1234567890123456789012345678901234567890'
# Isolate the Git checks so these fixtures exercise the release policy, even in a dirty checkout.
function git {
    $global:LASTEXITCODE = 0
    if ($args -contains 'rev-parse') { $commit }
}
function Write-Json([string]$Path,$Value) {
    $Value | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $Path -Encoding UTF8
}
Write-Json (Join-Path $testRoot 'stage/licenses/rust/dependency-index.json') @(@{noticeFileCount=1})
Write-Json (Join-Path $testRoot 'stage/runtime-manifest.json') @{assets=@(@{id='fixture';sha256=$sourceHash})}
$sourceReport = @{schemaVersion=1;complete=$true;reviewedBy='regression fixture';runtimes=@(@{id='fixture';binaryArchiveSha256=$sourceHash;includesAllStaticDependencies=$true;includesBuildScripts=$true;archives=@(@{file='fixture-source.txt';sha256=$sourceHash})})}
$sourceReportPath = Join-Path $sources 'corresponding-sources.json'
$reportPath = Join-Path $testRoot 'acceptance.json'
$hardwareCases = @('cleanWindows11','vlcCoexistence','defaultAppThumbnails','codecMatrix','hdr10PhysicalDisplay','sdrToneMapping','displaySwitching','subtitleAndAudio','corruptMediaIsolation','installUpgradeUninstall','performanceMeasured')
$results = @{}
foreach ($case in $hardwareCases) { $results[$case] = 'not-tested' }
foreach ($case in @('automatedQuality','runtimeSmoke','thumbnailWorkerSmoke','codecSamplePlayback','performanceMeasured')) { $results[$case] = 'pass' }
$report = @{version='0.1.0';tester='regression fixture';testedCommit=$commit;testDate='2026-10-07';channel='preview';limitations=@('Synthetic fixture with hardware cases explicitly untested.');results=$results}
function Assert-Gate([string]$Name,[string]$Channel,[bool]$Accepted,[string]$ErrorPattern='') {
    Write-Json $reportPath $report
    Write-Json $sourceReportPath $sourceReport
    $failure = $null
    try {
        & (Join-Path $PSScriptRoot 'Test-ReleaseGate.ps1') -AcceptanceFile $reportPath -Version '0.1.0' -StageDirectory (Join-Path $testRoot 'stage') -Channel $Channel
    } catch { $failure = $_.Exception.Message }
    if ($Accepted -and $failure) { throw "$Name unexpectedly failed: $failure" }
    if (!$Accepted -and (!$failure -or $failure -notmatch $ErrorPattern)) { throw "$Name did not reject the expected condition: $failure" }
    Write-Host "$Name passed."
}
Assert-Gate 'Explicit preview' preview $true
$report.channel = 'stable'
Assert-Gate 'Untested hardware cannot be stable' stable $false 'cleanWindows11'
$report.channel = 'preview'
$report.limitations = @()
Assert-Gate 'Preview limits required' preview $false 'hardware acceptance limits'
$report.limitations = @('Hardware not yet tested.')
$results.runtimeSmoke = 'not-tested'
Assert-Gate 'Preview runtime smoke required' preview $false 'runtimeSmoke'
$results.runtimeSmoke = 'pass'
$sourceReport.complete = $false
Assert-Gate 'Preview cannot bypass complete sources' preview $false 'reviewed as complete'
$sourceReport.complete = $true
$sourceReport.runtimes[0].archives[0].sha256 = ('0' * 64)
Assert-Gate 'Source hash enforced' preview $false 'checksum mismatch'
$sourceReport.runtimes[0].archives[0].sha256 = $sourceHash
$sourceReport.runtimes[0].archives[0].file = '../escape.txt'
Assert-Gate 'Source path traversal rejected' preview $false 'Invalid source archive path'
$sourceReport.runtimes[0].archives[0].file = 'fixture-source.txt'
foreach ($case in $hardwareCases) { $results[$case] = 'pass' }
$report.channel = 'stable'
Assert-Gate 'Stable retains complete hardware gate' stable $true
Write-Host 'Release policy regression checks passed. All attestations were synthetic fixtures under target/.'
