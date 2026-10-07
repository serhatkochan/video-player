[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
function ConvertFrom-GitHubTreeContent {
    param([string]$Content)
    try { $tree = $Content | ConvertFrom-Json } catch { throw 'GitHub API returned invalid JSON' }
    if ($tree.sha -cnotmatch '^[0-9a-f]{40}$' -or $tree.PSObject.Properties.Name -notcontains 'tree' -or $tree.truncated -ne $false) {
        throw 'GitHub API returned an invalid or truncated repository tree'
    }
    return $tree
}
function Get-GitHubTreeContent {
    param([Uri]$Uri, [hashtable]$Headers)
    for ($redirects = 0; $redirects -le 3; $redirects++) {
        if (!$Uri.IsAbsoluteUri -or $Uri.Scheme -cne 'https' -or $Uri.DnsSafeHost -ine 'api.github.com' -or $Uri.Port -ne 443 -or $Uri.UserInfo) {
            throw 'Refusing a GitHub API redirect outside https://api.github.com:443'
        }
        $redirect = $null
        $response = $null
        $requestErrors = @()
        try {
            $response = Invoke-WebRequest -UseBasicParsing -MaximumRedirection 0 -TimeoutSec 60 -Headers $Headers -Uri $Uri -ErrorAction SilentlyContinue -ErrorVariable requestErrors
        } catch { $requestErrors = @($_) }
        if (!$response -and $requestErrors.Count) { $response = $requestErrors[0].Exception.Response }
        if (!$response) {
            if ($requestErrors.Count) { throw $requestErrors[0] }
            throw 'GitHub API returned no response'
        }
        if ([int]$response.StatusCode -in @(301,302,303,307,308)) {
            try {
                if ($response.Headers -is [System.Net.WebHeaderCollection]) {
                    $redirect = $response.Headers['Location']
                } else {
                    $redirect = $response.Headers.Location
                }
                if (!$redirect) { throw 'GitHub API redirect has no Location header' }
                $Uri = [Uri]::new($Uri, [string]$redirect)
            } finally {
                if ($response.BaseResponse) { $response.BaseResponse.Dispose() } else { $response.Dispose() }
            }
        }
        if ($redirect) {
            if ($redirects -eq 3) { throw 'GitHub API redirect limit exceeded' }
            continue
        }
        if ($requestErrors.Count) { throw $requestErrors[0] }
        $content = [string]$response.Content
        ConvertFrom-GitHubTreeContent -Content $content | Out-Null
        return $content
    }
}
$repo = Split-Path $PSScriptRoot -Parent
Push-Location $repo
try {
    $metadata = (& cargo metadata --format-version 1 --locked --filter-platform x86_64-pc-windows-msvc | Out-String)
    if ($LASTEXITCODE -ne 0) { throw 'Cannot obtain locked Rust dependency metadata' }
    $graph = $metadata | ConvertFrom-Json
    $resolved = @($graph.resolve.nodes | ForEach-Object id)
    $packages = $graph.packages | Where-Object { $_.source -and $resolved -contains $_.id }
    $output = Join-Path $repo 'runtime/licenses/rust'
    New-Item -ItemType Directory -Force -Path $output | Out-Null
    $cache = Join-Path $repo 'runtime/license-cache'
    New-Item -ItemType Directory -Force -Path $cache | Out-Null
    $headers = @{ 'User-Agent'='VideoPlayer-license-collector' }
    if ($env:GH_TOKEN) { $headers['Authorization'] = 'Bearer ' + $env:GH_TOKEN }
    $entries = foreach ($package in $packages) {
        $source = Split-Path $package.manifest_path -Parent
        $destination = Join-Path $output "$($package.name)-$($package.version)"
        New-Item -ItemType Directory -Force -Path $destination | Out-Null
        $files = @(Get-ChildItem -LiteralPath $source -File -Recurse | Where-Object { $_.Name.ToLowerInvariant() -cmatch '^(license|licence|copying|notice)([._-]|$)' })
        if ($package.license_file) {
            $specific = Join-Path $source $package.license_file
            if (Test-Path -LiteralPath $specific) { $files += Get-Item -LiteralPath $specific }
        }
        foreach ($file in $files | Sort-Object FullName -Unique) {
            $relative = $file.FullName.Substring($source.Length + 1)
            $target = Join-Path $destination $relative
            New-Item -ItemType Directory -Force -Path (Split-Path $target -Parent) | Out-Null
            Copy-Item -LiteralPath $file.FullName -Destination $target -Force
        }
        $upstreamNotices = 0
        $vcsPath = Join-Path $source '.cargo_vcs_info.json'
        if ($files.Count -eq 0 -and $package.repository -cmatch '^https://github[.]com/([^/]+/[^/]+)' -and (Test-Path -LiteralPath $vcsPath)) {
            $repository = $Matches[1].TrimEnd('/').Replace('.git','')
            $commit = (Get-Content -LiteralPath $vcsPath -Raw | ConvertFrom-Json).git.sha1
            $cachedRepo = Join-Path $cache ($repository.Replace('/','-') + '-' + $commit)
            New-Item -ItemType Directory -Force -Path $cachedRepo | Out-Null
            $treeFile = Join-Path $cachedRepo 'tree.json'
            if (!(Test-Path -LiteralPath $treeFile)) {
                $treeContent = Get-GitHubTreeContent -Headers $headers -Uri "https://api.github.com/repos/$repository/git/trees/${commit}?recursive=1"
                $treeContent | Set-Content -LiteralPath ($treeFile + '.partial') -Encoding UTF8
                Move-Item -LiteralPath ($treeFile + '.partial') -Destination $treeFile -Force
            }
            $tree = ConvertFrom-GitHubTreeContent -Content (Get-Content -LiteralPath $treeFile -Raw)
            $notices = $tree.tree | Where-Object {
                $name = (Split-Path $_.path -Leaf).ToLowerInvariant()
                $_.type -eq 'blob' -and ($name -cmatch '^(license|licence|copying|notice)([._-]|$)' -or $name -cin @('ofl.txt','ufl.txt') -or $name.EndsWith('.license', [StringComparison]::Ordinal))
            }
            foreach ($notice in $notices) {
                $cached = Join-Path $cachedRepo $notice.path
                if (!(Test-Path -LiteralPath $cached)) {
                    New-Item -ItemType Directory -Force -Path (Split-Path $cached -Parent) | Out-Null
                    Invoke-WebRequest -Uri "https://raw.githubusercontent.com/$repository/$commit/$($notice.path)" -OutFile $cached
                }
                $target = Join-Path $destination ('upstream/' + $notice.path)
                New-Item -ItemType Directory -Force -Path (Split-Path $target -Parent) | Out-Null
                Copy-Item -LiteralPath $cached -Destination $target -Force
                $upstreamNotices++
            }
            [pscustomobject]@{ repository=$repository; commit=$commit; notices=@($notices.path) } | ConvertTo-Json -Depth 4 |
                Set-Content -LiteralPath (Join-Path $destination 'upstream-license-provenance.json') -Encoding UTF8
        }
        [pscustomobject]@{ name=$package.name; version=$package.version; license=$package.license; repository=$package.repository; noticeFileCount=($files.Count + $upstreamNotices) }
    }
    $entries | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $output 'dependency-index.json') -Encoding UTF8
    Write-Host "Collected Rust dependency notices for $($packages.Count) locked packages."
} finally { Pop-Location }
