[CmdletBinding()]
param([switch]$Live)
$ErrorActionPreference = 'Stop'
$collector = Join-Path $PSScriptRoot 'Get-RustNotices.ps1'
$tokens = $null
$parseErrors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($collector, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count) { throw 'Cannot parse the Rust notice collector' }
$definitions = @($ast.FindAll({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -in @('ConvertFrom-GitHubTreeContent','Get-GitHubTreeContent') }, $false))
if ($definitions.Count -ne 2) { throw 'GitHub tree downloader and validator not found' }
foreach ($definition in $definitions) { Invoke-Expression $definition.Extent.Text }

$treeUrl = 'https://api.github.com/repos/brendanzab/gl-rs/git/trees/ea503e8d5fb6d73c6030e6191ce738cd3bf3433e?recursive=1'
if ($Live) {
    $headers = @{ 'User-Agent'='VideoPlayer-license-collector-tests' }
    if ($env:GH_TOKEN) { $headers.Authorization = 'Bearer ' + $env:GH_TOKEN }
    $tree = Get-GitHubTreeContent -Uri $treeUrl -Headers $headers | ConvertFrom-Json
    if (@($tree.tree).Count -eq 0) { throw 'The renamed upstream repository tree is empty' }
    Write-Host "Live GitHub renamed-repository tree passed on PowerShell $($PSVersionTable.PSVersion)."
    return
}

Add-Type -TypeDefinition @'
using System;
public sealed class RustNoticeRedirectException : Exception {
    public object Response { get; private set; }
    public RustNoticeRedirectException(object response) : base("Test redirect") { Response = response; }
}
'@
$script:responses = New-Object System.Collections.Queue
$script:requests = New-Object System.Collections.ArrayList
$validTree = '{"sha":"ea503e8d5fb6d73c6030e6191ce738cd3bf3433e","truncated":false,"tree":[]}'
$headers = @{ Authorization='Bearer notice-test-token'; 'User-Agent'='notice-tests' }
function Invoke-WebRequest {
    param($Uri, $Headers, $MaximumRedirection, $TimeoutSec, $ErrorAction, [switch]$UseBasicParsing)
    if ($MaximumRedirection -ne 0) { throw 'Automatic redirects must be disabled before sending authentication' }
    [void]$script:requests.Add([pscustomobject]@{ Uri=[Uri]$Uri; Auth=$Headers.Authorization })
    if (!$script:responses.Count) { throw 'Unexpected request' }
    $response = $script:responses.Dequeue()
    if ($response -is [Exception]) { throw $response }
    if ($response.StatusCode -ne 200) { throw [RustNoticeRedirectException]::new($response) }
    return $response
}
function New-Redirect($Location, [switch]$LegacyHeaders, [int]$StatusCode=301) {
    $responseHeaders = if ($LegacyHeaders) {
        $collection = New-Object System.Net.WebHeaderCollection
        $collection['Location'] = $Location
        ,$collection
    } else { [pscustomobject]@{ Location=[Uri]$Location } }
    $response = [pscustomobject]@{ StatusCode=$StatusCode; Headers=$responseHeaders }
    Add-Member -InputObject $response -MemberType ScriptMethod -Name Dispose -Value {}
    return $response
}
function Reset-Requests {
    $script:responses.Clear()
    $script:requests.Clear()
}
function Assert-Throws($Action, $Pattern) {
    try { & $Action | Out-Null } catch {
        if ($_.Exception.Message -notmatch $Pattern) { throw }
        return
    }
    throw "Expected failure: $Pattern"
}
function Assert-AuthenticatedRequests([int]$Count) {
    if ($script:requests.Count -ne $Count) { throw "Expected $Count requests" }
    foreach ($request in $script:requests) {
        if ($request.Uri.Scheme -cne 'https' -or $request.Uri.DnsSafeHost -ine 'api.github.com' -or $request.Uri.Port -ne 443 -or $request.Uri.UserInfo -or $request.Auth -cne $headers.Authorization) {
            throw 'Authentication was missing or sent outside the trusted API origin'
        }
    }
}

foreach ($legacy in @($true,$false)) {
    Reset-Requests
    $script:responses.Enqueue((New-Redirect 'https://api.github.com/repositories/7655767/git/trees/ea503e8d5fb6d73c6030e6191ce738cd3bf3433e?recursive=1' -LegacyHeaders:$legacy))
    $script:responses.Enqueue([pscustomobject]@{ StatusCode=200; Content=$validTree })
    if ((Get-GitHubTreeContent -Uri $treeUrl -Headers $headers) -cne $validTree) { throw 'Repository tree content changed' }
    Assert-AuthenticatedRequests 2
}
Write-Host 'Same-origin redirects retain authentication with legacy and modern response headers.'

foreach ($hostile in @('http://api.github.com/tree','https://example.com/tree','https://api.github.com:444/tree','https://user@api.github.com/tree')) {
    Reset-Requests
    $script:responses.Enqueue((New-Redirect $hostile))
    Assert-Throws { Get-GitHubTreeContent -Uri $treeUrl -Headers $headers } 'outside'
    Assert-AuthenticatedRequests 1
    Reset-Requests
    Assert-Throws { Get-GitHubTreeContent -Uri $hostile -Headers $headers } 'outside'
    Assert-AuthenticatedRequests 0
}
Write-Host 'Hostile initial URLs and redirect targets are rejected before credentials can be sent.'

Reset-Requests
1..4 | ForEach-Object { $script:responses.Enqueue((New-Redirect $treeUrl)) }
Assert-Throws { Get-GitHubTreeContent -Uri $treeUrl -Headers $headers } 'redirect limit'
Assert-AuthenticatedRequests 4
Write-Host 'A redirect loop stops after three followed redirects.'

foreach ($status in @(401,403)) {
    Reset-Requests
    $script:responses.Enqueue((New-Redirect '/repositories/7655767/tree' -StatusCode 302))
    $script:responses.Enqueue((New-Redirect 'https://api.github.com/error' -StatusCode $status))
    Assert-Throws { Get-GitHubTreeContent -Uri $treeUrl -Headers $headers } 'Test redirect'
    Assert-AuthenticatedRequests 2
}
Reset-Requests
$script:responses.Enqueue((New-Redirect '/repositories/7655767/tree' -StatusCode 302))
$script:responses.Enqueue([InvalidOperationException]::new('Test transport error'))
Assert-Throws { Get-GitHubTreeContent -Uri $treeUrl -Headers $headers } 'Test transport error'
Assert-AuthenticatedRequests 2
Write-Host 'HTTP and transport failures after a relative redirect are propagated without another request.'

foreach ($invalid in @('{','{"message":"API rate limit exceeded"}','{"sha":"ea503e8d5fb6d73c6030e6191ce738cd3bf3433e","truncated":true,"tree":[]}')) {
    Reset-Requests
    $script:responses.Enqueue([pscustomobject]@{ StatusCode=200; Content=$invalid })
    Assert-Throws { Get-GitHubTreeContent -Uri $treeUrl -Headers $headers } 'JSON|invalid|truncated'
    Assert-AuthenticatedRequests 1
}
Write-Host 'Malformed, error and truncated JSON responses are rejected before caching.'
