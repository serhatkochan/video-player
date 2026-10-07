[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$id = [Guid]::NewGuid().ToString()
$prefix = "Software\VideoPlayerIntegrationTests\$id"
$testBase = [IO.Path]::GetFullPath((Join-Path $repo "dist/integration-tests/$id"))
$ps = Join-Path $env:WINDIR 'System32/WindowsPowerShell/v1.0/powershell.exe'
$script = Join-Path $repo 'packaging/Windows-Integration.ps1'
$registry = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser, [Microsoft.Win32.RegistryView]::Registry64)
$thumbnail = '{E357FCCD-A995-4576-B01F-234630154E96}'
$providerId = '{8C1D9ED4-6900-4D31-9EBB-570623A87973}'
function Assert([bool]$Condition, [string]$Message) { if (!$Condition) { throw $Message } }
function Read-Value([string]$Path, [string]$Name = '') {
    $key = $registry.OpenSubKey($Path)
    try { if ($key) { return $key.GetValue($Name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames) } }
    finally { if ($key) { $key.Dispose() } }
}
function Has-Value([string]$Path, [string]$Name = '') {
    $key = $registry.OpenSubKey($Path)
    try { return $key -and $key.GetValueNames() -contains $Name }
    finally { if ($key) { $key.Dispose() } }
}
function Write-Value([string]$Path, $Value, [string]$Name = '', [string]$Kind = 'String') {
    $key = $registry.CreateSubKey($Path)
    try { $key.SetValue($Name, $Value, [Microsoft.Win32.RegistryValueKind]::$Kind) }
    finally { $key.Dispose() }
}
function Context([string]$Scope, [string]$Scenario) {
    $root = $prefix
    $identity = 'User:CurrentUser'
    if ($Scope -eq 'Machine') { $root = "$prefix\Machine"; $identity = 'Machine:LocalMachine' }
    $base = Join-Path $testBase "$Scenario/$Scope"
    New-Item -ItemType Directory -Path $base -Force | Out-Null
    return [pscustomobject]@{ Scope=$Scope; Classes="$root\Classes"; Identity=$identity; Base=$base; State=(Join-Path $base 'integration-state.json') }
}
function Make-Payload($Context, [string]$Name) {
    $dir = Join-Path $Context.Base "versions/$Name"
    New-Item -ItemType Directory -Path $dir -Force | Out-Null
    foreach ($file in @('video-player.exe','video_player_thumbnail.dll','thumbnail-worker.exe','libmpv-2.dll','avformat-62.dll','avcodec-62.dll','avutil-60.dll','avfilter-11.dll','swscale-9.dll','swresample-6.dll')) {
        'test' | Set-Content -LiteralPath (Join-Path $dir $file)
    }
    return $dir
}
function Quote-Argument([string]$Value) {
    if ($Value.Contains('"')) { throw 'A test argument cannot contain a quote' }
    return '"' + $Value + '"'
}
function Run-Integration($Context, [string]$Action, [string]$Directory, [int]$Expected = 0, [string]$UserDirectory = '', [string]$ErrorPattern = '') {
    $arguments = @('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',(Quote-Argument $script),'-Action',$Action,'-Scope',$Context.Scope,'-InstallDirectory',(Quote-Argument $Directory),'-TestRegistryPrefix',(Quote-Argument $prefix))
    if ($Context.Scope -eq 'Machine') {
        if (!$UserDirectory) { $UserDirectory = Join-Path $testBase 'absent-user/versions/not-installed' }
        $arguments += @('-TestUserInstallDirectory',(Quote-Argument $UserDirectory))
    }
    $info = [Diagnostics.ProcessStartInfo]::new($ps, ($arguments -join ' '))
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($info)
    try {
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        $process.WaitForExit()
        Assert ($process.ExitCode -eq $Expected) "$($Context.Scope) $Action exited $($process.ExitCode), expected $Expected. $($stdout.Result) $($stderr.Result)"
        if ($ErrorPattern) {
            Assert (($stdout.Result + $stderr.Result) -match $ErrorPattern) "Integration failed for another reason; expected '$ErrorPattern'. $($stdout.Result) $($stderr.Result)"
        }
    } finally { $process.Dispose() }
}
function State-String([string]$Value) { return [pscustomobject]@{ Exists=$true; Kind='String'; Data=$Value } }
function Write-Journal($Context, $PreviousState, [object[]]$Writes, [string]$Directory, [bool]$Legacy = $false, $Handoff = $null) {
    if ($Legacy) {
        $absent = [pscustomobject]@{ Exists=$false; Kind=''; Data=$null }
        $Writes += [pscustomobject]@{ Path="$($Context.Classes)\CLSID\$providerId\InprocServer32"; Name=''; Value=$absent; New=(State-String (Join-Path $Directory 'video_player_thumbnail.dll')) }
        $Writes += [pscustomobject]@{ Path="$prefix\Uninstall"; Name='DisplayVersion'; Value=$absent; New=(State-String '0.1.0') }
        $journal = [pscustomobject]@{ JournalVersion=1; RegistryRoot=$Context.Classes; PreviousState=$PreviousState; Writes=$Writes }
    } else {
        $journal = [pscustomobject]@{ JournalVersion=2; InstallDirectory=$Directory; Version='0.1.0'; Scope=$Context.Scope; RegistryHive='CurrentUser'; RegistryIdentity=$Context.Identity; RegistryRoot=$Context.Classes; PreviousState=$PreviousState; Writes=$Writes; UserHandoff=$Handoff }
    }
    $journal | ConvertTo-Json -Depth 14 | Set-Content -LiteralPath "$($Context.State).pending" -Encoding UTF8
    return $journal
}
function Assert-ScopeState($Context) {
    $state = Get-Content -LiteralPath $Context.State -Raw | ConvertFrom-Json
    Assert ($state.SchemaVersion -eq 2) 'The committed backup must use schema 2'
    Assert ($state.Scope -ceq $Context.Scope) 'The backup lost its installation scope'
    Assert ($state.RegistryHive -ceq 'CurrentUser') 'The test backup must record its actual isolated hive'
    Assert ($state.RegistryIdentity -ceq $Context.Identity) 'The backup lost its logical registry identity'
    Assert ($state.RegistryRoot -ceq $Context.Classes) 'The backup lost its isolated classes root'
    return $state
}
function Assert-AppIcons($Context, [string]$Directory) {
    $root = $Context.Classes.Substring(0, $Context.Classes.Length - '\Classes'.Length)
    $icon = '"' + (Join-Path $Directory 'video-player.exe') + '",0'
    $application = "$($Context.Classes)\Applications\video-player.exe"
    Assert ((Read-Value "$root\Settings\Capabilities" 'ApplicationIcon') -ceq $icon) "$($Context.Scope) Default Apps icon does not point to the active executable"
    Assert ((Read-Value "$application\DefaultIcon") -ceq $icon) "$($Context.Scope) Open With icon does not point to the active executable"
    Assert ((Read-Value $application 'FriendlyAppName') -ceq 'Video Player') "$($Context.Scope) Open With application name is missing"
    Assert ((Read-Value "$application\shell\open\command") -ceq ('"' + (Join-Path $Directory 'video-player.exe') + '" "%1"')) "$($Context.Scope) Open With command cannot open a local file"
    foreach ($extension in @('.mp4','.m4v','.mkv','.mov','.webm','.avi','.wmv','.mp3','.aac','.m4a','.flac','.wav')) {
        Assert ((Read-Value "$($Context.Classes)\VideoPlayer$extension\DefaultIcon") -ceq $icon) "$($Context.Scope) $extension icon does not point to the active executable"
        Assert (Has-Value "$application\SupportedTypes" $extension) "$($Context.Scope) $extension is missing from Open With supported types"
    }
}
function Test-AppIconUpgrade($Context) {
    $first = Make-Payload $Context 'legacy-no-icon'
    $second = Make-Payload $Context 'with-icon'
    $root = $Context.Classes.Substring(0, $Context.Classes.Length - '\Classes'.Length)
    $capabilities = "$root\Settings\Capabilities"
    $application = "$($Context.Classes)\Applications\video-player.exe"
    Write-Value $capabilities 'Previous.ico,1' 'ApplicationIcon'
    Write-Value "$application\DefaultIcon" 'PreviousOpenWith.ico,2'
    Write-Value $application 'Previous application name' 'FriendlyAppName'
    Run-Integration $Context Install $first
    Assert-AppIcons $Context $first
    $state = Assert-ScopeState $Context
    $legacyEntries = @()
    foreach ($entry in $state.Entries) {
        if (($entry.Path -eq $capabilities -and $entry.Name -eq 'ApplicationIcon') -or $entry.Path -eq $application -or $entry.Path.StartsWith($application + '\', [StringComparison]::OrdinalIgnoreCase)) {
            if ($entry.Original.Exists) { Write-Value $entry.Path $entry.Original.Data $entry.Name $entry.Original.Kind }
            else {
                $key = $registry.OpenSubKey($entry.Path, $true)
                try { if ($key) { $key.DeleteValue($entry.Name, $false) } } finally { if ($key) { $key.Dispose() } }
            }
        } else { $legacyEntries += $entry }
    }
    $state.Entries = $legacyEntries
    $state | ConvertTo-Json -Depth 14 | Set-Content -LiteralPath $Context.State -Encoding UTF8
    Run-Integration $Context Install $second
    Assert-AppIcons $Context $second
    $upgraded = Assert-ScopeState $Context
    foreach ($legacyEntry in $legacyEntries) {
        $entry = $upgraded.Entries | Where-Object { $_.Path -eq $legacyEntry.Path -and $_.Name -eq $legacyEntry.Name } | Select-Object -First 1
        Assert (($entry.Original | ConvertTo-Json -Compress) -ceq ($legacyEntry.Original | ConvertTo-Json -Compress)) "$($Context.Scope) icon upgrade lost an original registry backup"
    }
    Write-Value "$application\DefaultIcon" 'ForeignAfterUpgrade.ico,0'
    Run-Integration $Context Uninstall $first 2
    Assert ((Read-Value $capabilities 'ApplicationIcon') -ceq ('"' + (Join-Path $second 'video-player.exe') + '",0')) "$($Context.Scope) stale uninstall removed the new app icon"
    Run-Integration $Context Uninstall $second
    Assert ((Read-Value $capabilities 'ApplicationIcon') -ceq 'Previous.ico,1') "$($Context.Scope) original Default Apps icon was not restored"
    Assert ((Read-Value "$application\DefaultIcon") -ceq 'ForeignAfterUpgrade.ico,0') "$($Context.Scope) uninstall clobbered a later foreign Open With icon"
    Assert ((Read-Value $application 'FriendlyAppName') -ceq 'Previous application name') "$($Context.Scope) original Open With name was not restored"
    Assert (!(Has-Value "$application\SupportedTypes" '.mkv')) "$($Context.Scope) uninstall left a newly owned supported type"
    Assert (!(Test-Path -LiteralPath $Context.State)) "$($Context.Scope) app icon uninstall left its backup"
    Write-Host "PASS: $($Context.Scope) Default Apps/Open With icons, legacy upgrade, original backups, stale uninstall, and foreign icon preservation."
}
function Test-ScopeLifecycle($Context) {
    $mkv = "$($Context.Classes)\.mkv\ShellEx\$thumbnail"
    $webm = "$($Context.Classes)\SystemFileAssociations\.webm\ShellEx\$thumbnail"
    $avi = "$($Context.Classes)\.avi\ShellEx\$thumbnail"
    $mp4 = "$($Context.Classes)\.mp4\ShellEx\$thumbnail"
    $choice = "$($Context.Classes)\.mkv\UserChoice"
    $foreignCommand = "$($Context.Classes)\VideoPlayer.mkv\shell\open\command"
    $first = Make-Payload $Context '0.1.0-first'
    $second = Make-Payload $Context '0.1.0-second'
    Write-Value $mkv ''
    Write-Value $webm ('OriginalProvider-' + $Context.Scope)
    Write-Value $mp4 ([byte[]]@(0, 3, 255, 17)) '' 'Binary'
    Write-Value $choice 'ForeignUserChoice' 'ProgId'
    Write-Journal $Context $null @([pscustomobject]@{ Path=$mkv; Name=''; Value=(State-String ''); New=(State-String $providerId) }) $first ($Context.Scope -eq 'User') | Out-Null
    Write-Value $mkv $providerId
    Run-Integration $Context Install $first
    Assert ((Read-Value $mkv) -ceq $providerId) "$($Context.Scope) initial provider not registered"
    Assert-ScopeState $Context | Out-Null
    Write-Value $avi 'ChangedBeforeUpgrade'
    Write-Value $foreignCommand 'ForeignPlayer.exe --open %1'
    $previous = Get-Content -LiteralPath $Context.State -Raw | ConvertFrom-Json
    $dllKey = "$($Context.Classes)\CLSID\$providerId\InprocServer32"
    $dllBefore = Join-Path $first 'video_player_thumbnail.dll'
    $dllAfter = Join-Path $second 'video_player_thumbnail.dll'
    Write-Journal $Context $previous @([pscustomobject]@{ Path=$dllKey; Name=''; Value=(State-String $dllBefore); New=(State-String $dllAfter) }) $second | Out-Null
    Write-Value $dllKey $dllAfter
    Remove-Item -LiteralPath $Context.State
    Run-Integration $Context Install $second
    Assert ((Read-Value $avi) -ceq 'ChangedBeforeUpgrade') "$($Context.Scope) upgrade overwrote another application"
    Assert-ScopeState $Context | Out-Null
    Run-Integration $Context Uninstall $first 2
    Assert ((Read-Value $mkv) -ceq $providerId) "$($Context.Scope) stale uninstaller clobbered upgrade"
    Write-Value $webm 'AnotherAppAfterUpgrade'
    Run-Integration $Context Uninstall $second
    Assert ((Has-Value $mkv) -and (Read-Value $mkv) -ceq '') "$($Context.Scope) existing empty default was not restored"
    Assert ((Read-Value $webm) -ceq 'AnotherAppAfterUpgrade') "$($Context.Scope) uninstall overwrote another application"
    Assert ([Convert]::ToBase64String([byte[]](Read-Value $mp4)) -ceq 'AAP/EQ==') "$($Context.Scope) original binary registry value was not restored"
    Assert (!(Has-Value "$($Context.Classes)\.m4v\ShellEx\$thumbnail")) "$($Context.Scope) originally missing association was not removed"
    Assert ((Read-Value $avi) -ceq 'ChangedBeforeUpgrade') "$($Context.Scope) uninstall overwrote provider preserved during upgrade"
    Assert ((Read-Value $foreignCommand) -ceq 'ForeignPlayer.exe --open %1') "$($Context.Scope) upgrade/uninstall overwrote a foreign open command"
    Assert ((Read-Value $choice 'ProgId') -ceq 'ForeignUserChoice') "$($Context.Scope) integration changed Windows user choice"
    Assert (!(Test-Path -LiteralPath $Context.State)) "$($Context.Scope) backup was not cleared after successful uninstall"
    Write-Host "PASS: $($Context.Scope) interrupted install/upgrade, stale uninstall, typed restoration, third-party ownership, and user-choice preservation."
}
function Test-JournalScopeGuards($Context) {
    $directory = Make-Payload $Context 'scope-guards'
    $path = "$($Context.Classes)\.m4v\ShellEx\$thumbnail"
    $write = [pscustomobject]@{ Path=$path; Name=''; Value=(State-String 'Before'); New=(State-String $providerId) }
    Write-Value $path $providerId
    foreach ($field in @('Scope','RegistryHive','RegistryIdentity')) {
        $journal = Write-Journal $Context $null @($write) $directory
        if ($field -eq 'RegistryHive') { $journal.$field = 'LocalMachine' } elseif ($field -eq 'Scope') { $journal.$field = 'User' } else { $journal.$field = 'User:CurrentUser' }
        $journal | ConvertTo-Json -Depth 14 | Set-Content -LiteralPath "$($Context.State).pending" -Encoding UTF8
        Run-Integration $Context Install $directory 1
        Assert ((Read-Value $path) -ceq $providerId) "A journal with wrong $field mutated the selected scope"
        Assert (Test-Path -LiteralPath "$($Context.State).pending") "A journal with wrong $field was discarded"
        Remove-Item -LiteralPath "$($Context.State).pending"
    }
    $userPath = "$prefix\Classes\.mov\ShellEx\$thumbnail"
    Write-Value $userPath 'UserScopeWitness'
    $crossScope = [pscustomobject]@{ Path=$userPath; Name=''; Value=(State-String 'WrongScopeRollback'); New=(State-String 'UserScopeWitness') }
    Write-Journal $Context $null @($crossScope) $directory | Out-Null
    Run-Integration $Context Install $directory 1
    Assert ((Read-Value $userPath) -ceq 'UserScopeWitness') 'A machine journal rolled back a user-scope value'
    Assert (Test-Path -LiteralPath "$($Context.State).pending") 'A cross-scope journal was discarded'
    Remove-Item -LiteralPath "$($Context.State).pending"
    Write-Host 'PASS: journal scope, physical hive, logical hive identity, and entry-path mismatch cannot modify another scope.'
}
function Test-UserToMachineMigration {
    $user = Context User 'migration'
    $machine = Context Machine 'migration'
    $userDirectory = Make-Payload $user 'legacy-user'
    $machineFirst = Make-Payload $machine 'machine-first'
    $machineSecond = Make-Payload $machine 'machine-second'
    $userMkv = "$($user.Classes)\.mkv\ShellEx\$thumbnail"
    $userWebm = "$($user.Classes)\SystemFileAssociations\.webm\ShellEx\$thumbnail"
    $machineMkv = "$($machine.Classes)\.mkv\ShellEx\$thumbnail"
    $userApp = "$prefix\Settings\Capabilities"
    $otherAccount = "$prefix\OtherAccount\Classes\.mkv\ShellEx\$thumbnail"
    Write-Value $userMkv 'OriginalUserProvider'
    Write-Value $userWebm 'OriginalUserWebmProvider'
    Write-Value $machineMkv 'OriginalMachineProvider'
    Write-Value $otherAccount 'UnrelatedAccountProvider'
    Run-Integration $user Install $userDirectory
    $legacyState = Assert-ScopeState $user
    $legacyState.SchemaVersion = 1
    foreach ($name in @('Scope','RegistryHive','RegistryIdentity','Version')) { $legacyState.PSObject.Properties.Remove($name) }
    foreach ($entry in $legacyState.Entries) {
        foreach ($name in @('OwnershipInstallDirectory','OwnershipVersion')) { $entry.PSObject.Properties.Remove($name) }
    }
    $legacyState | ConvertTo-Json -Depth 14 | Set-Content -LiteralPath $user.State -Encoding UTF8
    Write-Value $userWebm 'ForeignUserProvider'
    Write-Value $userApp 'ForeignApplicationName' 'ApplicationName'
    Run-Integration $machine Install $machineFirst 0 $userDirectory
    Assert-ScopeState $machine | Out-Null
    Assert ((Read-Value $userMkv) -ceq 'OriginalUserProvider') 'Machine migration did not restore the previous user provider'
    Assert ((Read-Value $userWebm) -ceq 'ForeignUserProvider') 'Machine migration overwrote a foreign user provider'
    Assert ((Read-Value $userApp 'ApplicationName') -ceq 'ForeignApplicationName') 'Machine migration overwrote foreign user settings'
    Assert (!(Has-Value "$($user.Classes)\CLSID\$providerId\InprocServer32")) 'Machine migration left the owned per-user COM registration masking the machine registration'
    Assert (!(Test-Path -LiteralPath $user.State)) 'Completed machine migration left a live user integration backup'
    Assert ((Read-Value $machineMkv) -ceq $providerId) 'Machine migration did not register its machine provider'
    Assert ((Read-Value $otherAccount) -ceq 'UnrelatedAccountProvider') 'Machine migration changed another account registration'
    Run-Integration $user Uninstall $userDirectory 2
    Assert ((Read-Value $machineMkv) -ceq $providerId) 'Stale user uninstaller modified machine integration'
    Run-Integration $machine Install $machineSecond 0 $userDirectory
    Run-Integration $machine Uninstall $machineFirst 2 $userDirectory
    Run-Integration $machine Uninstall $machineSecond 0 $userDirectory
    Assert ((Read-Value $machineMkv) -ceq 'OriginalMachineProvider') 'Machine uninstall did not restore its own original provider'
    Assert ((Read-Value $userMkv) -ceq 'OriginalUserProvider') 'Machine uninstall altered restored user integration'
    Assert ((Read-Value $userWebm) -ceq 'ForeignUserProvider') 'Machine uninstall altered a foreign user provider'
    Assert ((Read-Value $otherAccount) -ceq 'UnrelatedAccountProvider') 'Machine uninstall modified another account'
    Write-Host 'PASS: legacy user-to-machine migration restores owned values, preserves foreign values/accounts, and protects against stale uninstallers.'
}
function Test-InterruptedMachineHandoff {
    $user = Context User 'handoff-recovery'
    $machine = Context Machine 'handoff-recovery'
    $userDirectory = Make-Payload $user 'user-before-crash'
    $machineDirectory = Make-Payload $machine 'machine-before-crash'
    $userMkv = "$($user.Classes)\.mkv\ShellEx\$thumbnail"
    $userWebm = "$($user.Classes)\SystemFileAssociations\.webm\ShellEx\$thumbnail"
    $machinePath = "$($machine.Classes)\.m4v\ShellEx\$thumbnail"
    Write-Value $userMkv 'UserBeforeRecovery'
    Write-Value $userWebm 'UserWebmBeforeRecovery'
    Write-Value $machinePath 'MachineBeforeRecovery'
    Run-Integration $user Install $userDirectory
    $userState = Assert-ScopeState $user
    $handoffWrites = @()
    foreach ($valuePath in @($userMkv, $userWebm)) {
        $entry = $userState.Entries | Where-Object { $_.Path -eq $valuePath -and $_.Name -eq '' } | Select-Object -First 1
        Assert ($null -ne $entry) 'A recovery fixture must reference an owned user value'
        $handoffWrites += [pscustomobject]@{ Path=$entry.Path; Name=$entry.Name; Value=$entry.Owned; New=$entry.Original }
    }
    $handoff = [pscustomobject]@{ Scope='User'; RegistryHive='CurrentUser'; RegistryIdentity='User:CurrentUser'; RegistryRoot=$user.Classes; StatePath=$user.State; PreviousState=$userState; Writes=$handoffWrites }
    $machineWrites = @([pscustomobject]@{ Path=$machinePath; Name=''; Value=(State-String 'MachineBeforeRecovery'); New=(State-String $providerId) })
    Write-Value $machinePath $providerId
    Write-Value $userMkv 'UserBeforeRecovery'
    Write-Value $userWebm 'ForeignUserAfterCrash'
    Remove-Item -LiteralPath $user.State
    $handoff.StatePath = Join-Path $testBase 'other-account/integration-state.json'
    Write-Journal $machine $null $machineWrites $machineDirectory $false $handoff | Out-Null
    Run-Integration $machine Uninstall $machineDirectory 1 $userDirectory
    Assert ((Read-Value $machinePath) -ceq $providerId) 'An invalid handoff journal mutated machine values before full validation'
    Assert (!(Test-Path -LiteralPath $user.State)) 'An invalid handoff journal created a user backup'
    Assert (Test-Path -LiteralPath "$($machine.State).pending") 'An invalid handoff journal was discarded'
    $handoff.StatePath = $user.State
    Write-Journal $machine $null $machineWrites $machineDirectory $false $handoff | Out-Null
    Run-Integration $machine Uninstall $machineDirectory 2 $userDirectory
    Assert ((Read-Value $machinePath) -ceq 'MachineBeforeRecovery') 'Interrupted handoff failed to restore the machine value'
    Assert ((Read-Value $userMkv) -ceq $providerId) 'Interrupted handoff failed to reinstate user ownership'
    Assert ((Read-Value $userWebm) -ceq 'ForeignUserAfterCrash') 'Interrupted handoff replaced a later foreign user change'
    $restored = Assert-ScopeState $user
    Assert ($restored.InstallDirectory -ceq $userDirectory) 'Interrupted handoff restored the wrong user installation'
    Assert ($restored.Entries.Count -eq $userState.Entries.Count) 'Interrupted handoff lost user backup entries'
    Assert (!(Test-Path -LiteralPath "$($machine.State).pending")) 'Successful handoff recovery left its journal'
    Run-Integration $user Uninstall $userDirectory
    Assert ((Read-Value $userMkv) -ceq 'UserBeforeRecovery') 'Restored user backup did not preserve the original provider'
    Assert ((Read-Value $userWebm) -ceq 'ForeignUserAfterCrash') 'Restored user uninstaller replaced a foreign change'
    Write-Host 'PASS: interrupted machine/user handoff restores both scopes and user backup; invalid state paths and later foreign changes are preserved.'
}
function Test-BackupScopeGuards {
    $context = Context Machine 'backup-guards'
    $directory = Make-Payload $context 'machine-backup'
    $providerPath = "$($context.Classes)\.mkv\ShellEx\$thumbnail"
    Run-Integration $context Install $directory
    $originalJson = Get-Content -LiteralPath $context.State -Raw
    foreach ($field in @('Scope','RegistryHive','RegistryIdentity','RegistryRoot')) {
        $state = $originalJson | ConvertFrom-Json
        switch ($field) {
            'Scope' { $state.Scope = 'User' }
            'RegistryHive' { $state.RegistryHive = 'LocalMachine' }
            'RegistryIdentity' { $state.RegistryIdentity = 'User:CurrentUser' }
            'RegistryRoot' { $state.RegistryRoot = "$prefix\Classes" }
        }
        $state | ConvertTo-Json -Depth 14 | Set-Content -LiteralPath $context.State -Encoding UTF8
        Run-Integration $context Uninstall $directory 1
        Assert ((Read-Value $providerPath) -ceq $providerId) "An incompatible $field backup modified machine integration"
        Assert (Test-Path -LiteralPath $context.State) "An incompatible $field backup was discarded"
    }
    foreach ($entriesCase in @('Empty','Missing')) {
        $state = $originalJson | ConvertFrom-Json
        if ($entriesCase -eq 'Empty') { $state.Entries = @() }
        else { $state.PSObject.Properties.Remove('Entries') }
        $state | ConvertTo-Json -Depth 14 | Set-Content -LiteralPath $context.State -Encoding UTF8
        Run-Integration $context Uninstall $directory 1 '' 'no recoverable entries'
        Assert ((Read-Value $providerPath) -ceq $providerId) "A backup with $entriesCase entries modified machine integration"
        Assert (Test-Path -LiteralPath $context.State) "A backup with $entriesCase entries was discarded"
    }
    $userPath = "$prefix\Classes\.mkv\ShellEx\$thumbnail"
    Write-Value $userPath 'UserBackupWitness'
    $state = $originalJson | ConvertFrom-Json
    $entry = $state.Entries | Where-Object { $_.Path -eq $providerPath -and $_.Name -eq '' } | Select-Object -First 1
    $entry.Path = $userPath
    $entry.Owned = State-String 'UserBackupWitness'
    $state | ConvertTo-Json -Depth 14 | Set-Content -LiteralPath $context.State -Encoding UTF8
    Run-Integration $context Uninstall $directory 1
    Assert ((Read-Value $userPath) -ceq 'UserBackupWitness') 'A machine backup restored a user-scope entry'
    Assert ((Read-Value $providerPath) -ceq $providerId) 'A cross-scope backup modified machine integration'
    $state = $originalJson | ConvertFrom-Json
    $state.Entries += $state.Entries[0]
    $state | ConvertTo-Json -Depth 14 | Set-Content -LiteralPath $context.State -Encoding UTF8
    Run-Integration $context Uninstall $directory 1
    Assert ((Read-Value $providerPath) -ceq $providerId) 'A duplicate-entry backup changed machine integration'
    Assert (Test-Path -LiteralPath $context.State) 'A duplicate-entry backup was discarded'
    $originalJson | Set-Content -LiteralPath $context.State -Encoding UTF8
    Run-Integration $context Uninstall $directory
    Write-Host 'PASS: backup scope/hive/root/identity, empty or missing entries, foreign paths, and duplicate entries are rejected before uninstall changes either scope.'
}
function Test-NewerUserInstallAfterHandoffCrash {
    $user = Context User 'newer-user-recovery'
    $machine = Context Machine 'newer-user-recovery'
    $oldUserDirectory = Make-Payload $user 'old-user'
    $newUserDirectory = Make-Payload $user 'new-user'
    $machineDirectory = Make-Payload $machine 'machine-crash'
    $userMkv = "$($user.Classes)\.mkv\ShellEx\$thumbnail"
    $userDll = "$($user.Classes)\CLSID\$providerId\InprocServer32"
    $machinePath = "$($machine.Classes)\.m4v\ShellEx\$thumbnail"
    Write-Value $userMkv 'OriginalBeforeNewerUser'
    Write-Value $machinePath 'MachineBeforeNewerUser'
    Run-Integration $user Install $oldUserDirectory
    $oldState = Assert-ScopeState $user
    $handoffWrites = @()
    foreach ($valuePath in @($userMkv, $userDll)) {
        $entry = $oldState.Entries | Where-Object { $_.Path -eq $valuePath -and $_.Name -eq '' } | Select-Object -First 1
        Assert ($null -ne $entry) 'Newer-user recovery fixture requires an owned old user value'
        $handoffWrites += [pscustomobject]@{ Path=$entry.Path; Name=$entry.Name; Value=$entry.Owned; New=$entry.Original }
        if ($entry.Original.Exists) {
            Assert ($entry.Original.Kind -eq 'String') 'The newer-user fixture expects a string original'
            Write-Value $valuePath $entry.Original.Data
        } else {
            $key = $registry.OpenSubKey($valuePath, $true)
            try { if ($key) { $key.DeleteValue('', $false) } } finally { if ($key) { $key.Dispose() } }
        }
    }
    Remove-Item -LiteralPath $user.State
    $handoff = [pscustomobject]@{ Scope='User'; RegistryHive='CurrentUser'; RegistryIdentity='User:CurrentUser'; RegistryRoot=$user.Classes; StatePath=$user.State; PreviousState=$oldState; Writes=$handoffWrites }
    $machineWrites = @([pscustomobject]@{ Path=$machinePath; Name=''; Value=(State-String 'MachineBeforeNewerUser'); New=(State-String $providerId) })
    Write-Value $machinePath $providerId
    Write-Journal $machine $null $machineWrites $machineDirectory $false $handoff | Out-Null
    Run-Integration $user Install $newUserDirectory
    $newStateJson = Get-Content -LiteralPath $user.State -Raw
    Run-Integration $machine Uninstall $machineDirectory 2 $oldUserDirectory
    Assert ((Get-Content -LiteralPath $user.State -Raw) -ceq $newStateJson) 'Machine crash recovery replaced a newer user backup'
    Assert ((Read-Value $userDll) -ceq (Join-Path $newUserDirectory 'video_player_thumbnail.dll')) 'Machine crash recovery replaced a newer user DLL registration'
    Assert ((Read-Value $machinePath) -ceq 'MachineBeforeNewerUser') 'Newer-user recovery failed to restore machine integration'
    Run-Integration $user Uninstall $oldUserDirectory 2
    Run-Integration $user Uninstall $newUserDirectory
    Assert ((Read-Value $userMkv) -ceq 'OriginalBeforeNewerUser') 'Preserved newer-user backup lost its original provider'
    Assert (!(Has-Value $userDll)) 'Preserved newer-user backup lost the originally missing DLL value'
    Write-Host 'PASS: a user reinstall after a machine handoff crash keeps its newer backup, DLL registration, and safe uninstall.'
}
function Test-ReparseHandoffSource {
    $machine = Context Machine 'junction-guard'
    $directory = Make-Payload $machine 'machine'
    $target = Join-Path $testBase 'junction-target'
    $junction = Join-Path $testBase 'junction-user'
    $witness = Join-Path $target 'integration-state.json'
    $machineProvider = "$($machine.Classes)\.mkv\ShellEx\$thumbnail"
    New-Item -ItemType Directory -Path $target -Force | Out-Null
    'untouched outside-installation state' | Set-Content -LiteralPath $witness -Encoding UTF8
    $before = Get-Content -LiteralPath $witness -Raw
    Write-Value $machineProvider 'ForeignMachineBeforeJunction'
    New-Item -ItemType Junction -Path $junction -Target $target | Out-Null
    try {
        $sourceDirectory = Join-Path $junction 'versions/old-user'
        Run-Integration $machine Install $directory 1 $sourceDirectory 'reparse point'
        Assert ((Get-Content -LiteralPath $witness -Raw) -ceq $before) 'A junction handoff changed the outside-installation state'
        Assert ((Read-Value $machineProvider) -ceq 'ForeignMachineBeforeJunction') 'A junction handoff modified machine integration'
        Assert (!(Test-Path -LiteralPath $machine.State)) 'A junction handoff created machine integration state'
    } finally {
        $resolvedJunction = [IO.Path]::GetFullPath($junction)
        $allowed = $testBase + [IO.Path]::DirectorySeparatorChar
        if (!$resolvedJunction.StartsWith($allowed, [StringComparison]::OrdinalIgnoreCase)) { throw 'Refusing unsafe junction cleanup' }
        $item = Get-Item -LiteralPath $junction -Force
        if (!($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Expected a test junction during cleanup' }
        [IO.Directory]::Delete($resolvedJunction)
    }
    Assert (Test-Path -LiteralPath $witness) 'Deleting the test junction removed its target'
    Write-Host 'PASS: junction source-account backups are rejected before registry/file changes; the outside-installation witness remains intact.'
}
try {
    Test-ScopeLifecycle (Context User 'lifecycle')
    Test-ScopeLifecycle (Context Machine 'lifecycle')
    Test-AppIconUpgrade (Context User 'app-icons')
    Test-AppIconUpgrade (Context Machine 'app-icons')
    Test-JournalScopeGuards (Context Machine 'guards')
    Test-UserToMachineMigration
    Test-InterruptedMachineHandoff
    Test-BackupScopeGuards
    Test-NewerUserInstallAfterHandoffCrash
    Test-ReparseHandoffSource
} finally {
    $registry.DeleteSubKeyTree($prefix, $false)
    $registry.Dispose()
    $allowedBase = [IO.Path]::GetFullPath((Join-Path $repo 'dist/integration-tests')) + [IO.Path]::DirectorySeparatorChar
    if (!$testBase.StartsWith($allowedBase, [StringComparison]::OrdinalIgnoreCase)) { throw 'Refusing unsafe test cleanup' }
    if (Test-Path -LiteralPath $testBase) { Remove-Item -LiteralPath $testBase -Recurse -Force }
}
