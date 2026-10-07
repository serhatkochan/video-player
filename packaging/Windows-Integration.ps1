[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('Install','Uninstall')][string]$Action,
    [Parameter(Mandatory)][string]$InstallDirectory,
    [string]$Version = '0.1.0',
    [ValidateSet('User','Machine')][string]$Scope = 'User',
    [string]$TestRegistryPrefix,
    [string]$TestUserInstallDirectory
)
$ErrorActionPreference = 'Stop'
function Assert-NoReparse([string]$Path) {
    $candidates = [Collections.Generic.List[string]]::new()
    $candidate = [IO.Path]::GetFullPath($Path)
    while ($candidate) {
        if ($candidates.Count -ge 128) { throw 'Installation path exceeds the safe ancestor limit' }
        $candidates.Add($candidate)
        $candidate = Split-Path $candidate -Parent
    }
    # Inspect parents before children so a writable junction is never followed.
    for ($index = $candidates.Count - 1; $index -ge 0; $index--) {
        try { $item = Get-Item -LiteralPath $candidates[$index] -Force -ErrorAction Stop }
        catch {
            if ($_.CategoryInfo.Category -eq 'ObjectNotFound') { continue }
            throw
        }
        if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Refusing to follow a reparse point in an installation or integration backup path' }
    }
}
$clsid = '{8C1D9ED4-6900-4D31-9EBB-570623A87973}'
$thumbnailInterface = '{E357FCCD-A995-4576-B01F-234630154E96}'
$videoExtensions = @('.mp4','.m4v','.mkv','.mov','.webm','.avi','.wmv')
$mediaExtensions = $videoExtensions + @('.mp3','.aac','.m4a','.flac','.wav')
$InstallDirectory = [IO.Path]::GetFullPath($InstallDirectory).TrimEnd('\')
$versionsDirectory = Split-Path $InstallDirectory -Parent
if ((Split-Path $versionsDirectory -Leaf) -ne 'versions') { throw 'Expected an immutable installation in a versions directory' }
$baseDirectory = Split-Path $versionsDirectory -Parent
$statePath = Join-Path $baseDirectory 'integration-state.json'
$userBaseDirectory = Join-Path $env:LOCALAPPDATA 'Programs\VideoPlayer'
$logicalHive = if ($Scope -eq 'Machine') { 'LocalMachine' } else { 'CurrentUser' }
$registryIdentity = "$($Scope):$logicalHive"
$physicalHive = $logicalHive
if ($TestRegistryPrefix) {
    if ($TestRegistryPrefix -notmatch '^Software\\VideoPlayerIntegrationTests\\[0-9a-f-]{36}$') { throw 'Invalid isolated test registry prefix' }
    $physicalHive = 'CurrentUser'
    $registryPrefix = if ($Scope -eq 'Machine') { "$TestRegistryPrefix\Machine" } else { $TestRegistryPrefix }
    $classes = "$registryPrefix\Classes"
    $settings = "$registryPrefix\Settings"
    $registeredApplications = "$registryPrefix\RegisteredApplications"
    $uninstall = "$registryPrefix\Uninstall"
    $userClasses = "$TestRegistryPrefix\Classes"
    $userSettings = "$TestRegistryPrefix\Settings"
    $userRegisteredApplications = "$TestRegistryPrefix\RegisteredApplications"
    $userUninstall = "$TestRegistryPrefix\Uninstall"
    # Isolated tests never read or retire an actual user's installation.
    $userBaseDirectory = $null
    if ($TestUserInstallDirectory) {
        $testUserVersions = Split-Path ([IO.Path]::GetFullPath($TestUserInstallDirectory).TrimEnd('\')) -Parent
        if ((Split-Path $testUserVersions -Leaf) -ne 'versions') { throw 'Invalid isolated user installation directory' }
        $userBaseDirectory = Split-Path $testUserVersions -Parent
        if ($userBaseDirectory -eq $baseDirectory -and $Scope -eq 'Machine') { throw 'User and machine test payloads must be separate' }
    }
} else {
    if ($TestUserInstallDirectory) { throw 'TestUserInstallDirectory requires an isolated registry prefix' }
    $programFiles = if ($env:ProgramW6432) { $env:ProgramW6432 } else { $env:ProgramFiles }
    $expectedBase = if ($Scope -eq 'Machine') { Join-Path $programFiles 'VideoPlayer' } else { $userBaseDirectory }
    if ($baseDirectory -ne $expectedBase) { throw 'Refusing to modify integration for an unexpected installation directory' }
    if ($Scope -eq 'Machine') {
        $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
        try {
            $principal = [Security.Principal.WindowsPrincipal]::new($identity)
            if (!$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Machine integration requires an administrator token' }
        } finally { $identity.Dispose() }
    }
    $classes = $userClasses = 'Software\Classes'
    $settings = $userSettings = 'Software\VideoPlayer'
    $registeredApplications = $userRegisteredApplications = 'Software\RegisteredApplications'
    $uninstall = $userUninstall = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\VideoPlayer'
}
Assert-NoReparse $InstallDirectory
foreach ($backupSuffix in @('','.pending','.pending-write','.next')) { Assert-NoReparse "$statePath$backupSuffix" }
if ($Scope -eq 'Machine' -and $userBaseDirectory) {
    $checkedUserState = Join-Path $userBaseDirectory 'integration-state.json'
    foreach ($backupSuffix in @('','.pending','.handoff-next')) { Assert-NoReparse "$checkedUserState$backupSuffix" }
}
$registry = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::$physicalHive, [Microsoft.Win32.RegistryView]::Registry64)
$userRegistry = $null
if ($Scope -eq 'Machine') {
    $userRegistry = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser, [Microsoft.Win32.RegistryView]::Registry64)
}
function Get-ValueState([string]$Path, [string]$Name, $RegistryHandle = $registry) {
    $key = $RegistryHandle.OpenSubKey($Path)
    try {
        if (!$key -or $key.GetValueNames() -notcontains $Name) { return [pscustomobject]@{ Exists=$false; Kind=''; Data=$null } }
        $kind = $key.GetValueKind($Name).ToString()
        $data = $key.GetValue($Name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
        if ($kind -in @('None','Binary')) { $data = [Convert]::ToBase64String([byte[]]$data) }
        return [pscustomobject]@{ Exists=$true; Kind=$kind; Data=$data }
    } finally { if ($key) { $key.Dispose() } }
}
function Set-ValueState([string]$Path, [string]$Name, $State, $RegistryHandle = $registry) {
    if (!$State.Exists) {
        $key = $RegistryHandle.OpenSubKey($Path, $true)
        try { if ($key) { $key.DeleteValue($Name, $false) } } finally { if ($key) { $key.Dispose() } }
        return
    }
    $data = $State.Data
    switch ($State.Kind) {
        { $_ -in @('None','Binary') } { $data = [Convert]::FromBase64String([string]$data) }
        'DWord' { $data = [int]$data }
        'QWord' { $data = [long]$data }
        'MultiString' { $data = [string[]]$data }
        default { $data = [string]$data }
    }
    $key = $RegistryHandle.CreateSubKey($Path)
    try { $key.SetValue($Name, $data, [Microsoft.Win32.RegistryValueKind]::$($State.Kind)) } finally { $key.Dispose() }
}
function Same-State($A, $B) {
    return ($A.Exists -eq $B.Exists -and $A.Kind -eq $B.Kind -and (($A.Data | ConvertTo-Json -Compress) -ceq ($B.Data | ConvertTo-Json -Compress)))
}
function Remove-EmptyKey([string]$Path, $RegistryHandle = $registry, [string]$ClassesRoot = $classes, [string]$SettingsRoot = $settings, [string]$UninstallRoot = $uninstall) {
    while ($Path.StartsWith("$ClassesRoot\", [StringComparison]::OrdinalIgnoreCase) -or $Path.StartsWith("$SettingsRoot\", [StringComparison]::OrdinalIgnoreCase) -or $Path -eq $UninstallRoot) {
        $key = $RegistryHandle.OpenSubKey($Path)
        if (!$key) { break }
        $empty = $key.ValueCount -eq 0 -and $key.SubKeyCount -eq 0
        $key.Dispose()
        if (!$empty) { break }
        $RegistryHandle.DeleteSubKey($Path, $false)
        $Path = $Path.Substring(0, $Path.LastIndexOf('\'))
    }
}
function Get-Manifest([string]$Directory, [string]$ManifestVersion, [string]$ClassesRoot, [string]$SettingsRoot, [string]$ApplicationsRoot, [string]$UninstallRoot) {
    if ($ManifestVersion -notmatch '^[0-9A-Za-z.+_-]{1,64}$') { throw 'Invalid integration version' }
    $writes = [Collections.Generic.List[object]]::new()
    function Add-Write([string]$Path, [string]$Name, [string]$Data, [string]$Kind='String') {
        $writes.Add([pscustomobject]@{ Path=$Path; Name=$Name; Value=[pscustomobject]@{ Exists=$true; Kind=$Kind; Data=$Data } })
    }
    $exe = Join-Path $Directory 'video-player.exe'
    $provider = Join-Path $Directory 'video_player_thumbnail.dll'
    Add-Write "$ClassesRoot\CLSID\$clsid" '' 'Video Player Thumbnail Provider'
    Add-Write "$ClassesRoot\CLSID\$clsid" 'AppID' $clsid
    Add-Write "$ClassesRoot\CLSID\$clsid\InprocServer32" '' $provider
    Add-Write "$ClassesRoot\CLSID\$clsid\InprocServer32" 'ThreadingModel' 'Apartment'
    Add-Write "$ClassesRoot\AppID\$clsid" 'DllSurrogate' ''
    Add-Write "$SettingsRoot\Capabilities" 'ApplicationName' 'Video Player'
    Add-Write "$SettingsRoot\Capabilities" 'ApplicationDescription' 'Open-source video and audio player with Explorer thumbnails.'
    Add-Write $ApplicationsRoot 'Video Player' "$SettingsRoot\Capabilities"
    Add-Write "$SettingsRoot\Capabilities" 'ApplicationIcon' ('"' + $exe + '",0')
    $application = "$ClassesRoot\Applications\video-player.exe"
    Add-Write $application 'FriendlyAppName' 'Video Player'
    Add-Write "$application\DefaultIcon" '' ('"' + $exe + '",0')
    Add-Write "$application\shell\open\command" '' ('"' + $exe + '" "%1"')
    foreach ($ext in $mediaExtensions) {
        Add-Write "$application\SupportedTypes" $ext ''
    }
    foreach ($ext in $mediaExtensions) {
        $progId = 'VideoPlayer' + $ext
        Add-Write "$SettingsRoot\Capabilities\FileAssociations" $ext $progId
        Add-Write "$ClassesRoot\$progId" '' ('Video Player ' + $ext + ' media')
        Add-Write "$ClassesRoot\$progId\DefaultIcon" '' ('"' + $exe + '",0')
        Add-Write "$ClassesRoot\$progId\shell\open\command" '' ('"' + $exe + '" "%1"')
        Add-Write "$ClassesRoot\$ext\OpenWithProgids" $progId '' 'None'
        if ($ext -in $videoExtensions) {
            Add-Write "$ClassesRoot\$progId\ShellEx\$thumbnailInterface" '' $clsid
            Add-Write "$ClassesRoot\$ext\ShellEx\$thumbnailInterface" '' $clsid
            Add-Write "$ClassesRoot\SystemFileAssociations\$ext\ShellEx\$thumbnailInterface" '' $clsid
        }
    }
    Add-Write $UninstallRoot 'DisplayName' 'Video Player'
    Add-Write $UninstallRoot 'DisplayVersion' $ManifestVersion
    Add-Write $UninstallRoot 'Publisher' 'Serhat Kochan'
    Add-Write $UninstallRoot 'InstallLocation' $Directory
    Add-Write $UninstallRoot 'DisplayIcon' $exe
    Add-Write $UninstallRoot 'UninstallString' ('"' + (Join-Path $Directory 'uninstall.exe') + '"')
    Add-Write $UninstallRoot 'QuietUninstallString' ('"' + (Join-Path $Directory 'uninstall.exe') + '" /S')
    return $writes.ToArray()
}
function Assert-ValueState($Value) {
    if (!$Value -or $Value.Exists -isnot [bool]) { throw 'Invalid backed-up registry value' }
    if (!$Value.Exists) {
        if ($Value.Kind -ne '' -or $null -ne $Value.Data) { throw 'Invalid absent registry value' }
        return
    }
    switch ($Value.Kind) {
        { $_ -in @('None','Binary') } { $null = [Convert]::FromBase64String([string]$Value.Data) }
        'DWord' { $null = [int]$Value.Data }
        'QWord' { $null = [long]$Value.Data }
        'MultiString' { if ($Value.Data -isnot [array]) { throw 'Invalid registry string array' } }
        { $_ -in @('String','ExpandString') } { if ($Value.Data -isnot [string]) { throw 'Invalid registry string' } }
        default { throw 'Invalid registry value kind' }
    }
}
function Assert-Directory([string]$Directory, [string]$Base) {
    $full = [IO.Path]::GetFullPath($Directory).TrimEnd('\')
    if ($Directory -ne $full -or (Split-Path (Split-Path $full -Parent) -Leaf) -ne 'versions' -or (Split-Path (Split-Path $full -Parent) -Parent) -ne $Base) {
        throw 'Integration state refers to an unexpected installation directory'
    }
}
function Get-StateVersion($State, [string]$UninstallRoot) {
    if ($State.Version) { return [string]$State.Version }
    return [string](($State.Entries | Where-Object { $_.Path -eq $UninstallRoot -and $_.Name -eq 'DisplayVersion' } | Select-Object -First 1).Owned.Data)
}
function Assert-Identity($Record, [string]$ExpectedScope, [string]$ExpectedHive, [string]$ExpectedIdentity, [string]$ClassesRoot, [string]$VersionProperty) {
    if ($Record.RegistryRoot -ne $ClassesRoot) { throw 'Integration registry root does not match this installation' }
    if ($Record.$VersionProperty -eq 1) {
        if ($ExpectedScope -ne 'User' -or $ExpectedHive -ne 'CurrentUser' -or $Record.Scope -or $Record.RegistryHive -or $Record.RegistryIdentity) { throw 'Legacy integration records are valid only for the original user scope' }
    } elseif ($Record.$VersionProperty -ne 2 -or $Record.Scope -ne $ExpectedScope -or $Record.RegistryHive -ne $ExpectedHive -or $Record.RegistryIdentity -ne $ExpectedIdentity) {
        throw 'Integration scope or registry hive does not match this installation'
    }
}
function Assert-State($State, [string]$ExpectedScope, [string]$ExpectedHive, [string]$ExpectedIdentity, [string]$Base, [string]$ClassesRoot, [string]$SettingsRoot, [string]$ApplicationsRoot, [string]$UninstallRoot) {
    Assert-Identity $State $ExpectedScope $ExpectedHive $ExpectedIdentity $ClassesRoot 'SchemaVersion'
    Assert-Directory $State.InstallDirectory $Base
    if (!$State.Entries) { throw 'Integration backup has no recoverable entries; preserving it for recovery' }
    $manifest = @(Get-Manifest $State.InstallDirectory (Get-StateVersion $State $UninstallRoot) $ClassesRoot $SettingsRoot $ApplicationsRoot $UninstallRoot)
    $ownershipManifests = @{}
    $seen = @{}
    foreach ($entry in @($State.Entries)) {
        $expected = $manifest | Where-Object { $_.Path -eq $entry.Path -and $_.Name -eq $entry.Name } | Select-Object -First 1
        $entryId = $entry.Path + [char]0 + $entry.Name
        if (!$expected -or $seen.ContainsKey($entryId)) { throw 'Integration backup contains an unexpected registry path or value name' }
        $seen[$entryId] = $true
        Assert-ValueState $entry.Original
        Assert-ValueState $entry.Owned
        $ownershipDirectory = if ($entry.OwnershipInstallDirectory) { [string]$entry.OwnershipInstallDirectory } else { [string]$State.InstallDirectory }
        $ownershipVersion = if ($entry.OwnershipVersion) { [string]$entry.OwnershipVersion } else { Get-StateVersion $State $UninstallRoot }
        Assert-Directory $ownershipDirectory $Base
        $ownershipId = $ownershipDirectory + [char]0 + $ownershipVersion
        if (!$ownershipManifests.ContainsKey($ownershipId)) {
            $ownershipManifests[$ownershipId] = @(Get-Manifest $ownershipDirectory $ownershipVersion $ClassesRoot $SettingsRoot $ApplicationsRoot $UninstallRoot)
        }
        $ownedWrite = $ownershipManifests[$ownershipId] | Where-Object { $_.Path -eq $entry.Path -and $_.Name -eq $entry.Name } | Select-Object -First 1
        if (!(Same-State $entry.Owned $ownedWrite.Value)) { throw 'Integration backup does not prove ownership of this registry value' }
    }
}
function Notify-Shell {
    if ($TestRegistryPrefix) { return }
    Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class VideoPlayerShell { [DllImport("shell32.dll")] public static extern void SHChangeNotify(uint e, uint f, IntPtr a, IntPtr b); }'
    [VideoPlayerShell]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)
}
function Restore-UserHandoffState($Handoff) {
    Assert-NoReparse $Handoff.StatePath
    Assert-NoReparse "$($Handoff.StatePath).handoff-next"
    $previousJson = $Handoff.PreviousState | ConvertTo-Json -Depth 10 -Compress
    if (Test-Path -LiteralPath $Handoff.StatePath) {
        try { $currentJson = Get-Content -LiteralPath $Handoff.StatePath -Raw | ConvertFrom-Json | ConvertTo-Json -Depth 10 -Compress }
        catch { Write-Host 'WARNING: Preserving a user integration backup changed after the interrupted machine migration.'; return }
        if ($currentJson -cne $previousJson) {
            Write-Host 'WARNING: Preserving a newer user integration backup after the interrupted machine migration.'
            return
        }
    }
    $Handoff.PreviousState | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath "$($Handoff.StatePath).handoff-next" -Encoding UTF8
    Move-Item -LiteralPath "$($Handoff.StatePath).handoff-next" -Destination $Handoff.StatePath -Force
}
$sidIdentity = [Security.Principal.WindowsIdentity]::GetCurrent()
try { $currentSid = $sidIdentity.User.Value } finally { $sidIdentity.Dispose() }
$mutexSuffix = if ($TestRegistryPrefix) { '-' + ($TestRegistryPrefix -split '\\')[-1] } else { '' }
$userMutexName = "Global\VideoPlayer-Integration-User-$currentSid-8C1D9ED4$mutexSuffix"
$mutexName = if ($Scope -eq 'Machine') { "Global\VideoPlayer-Integration-Machine-8C1D9ED4$mutexSuffix" } else { $userMutexName }
$mutex = [Threading.Mutex]::new($false, $mutexName)
$handoffMutex = $null
$lockAcquired = $false
$handoffLockAcquired = $false
try {
    try { $lockAcquired = $mutex.WaitOne(30000) } catch [Threading.AbandonedMutexException] { $lockAcquired = $true }
    if (!$lockAcquired) { throw 'Another installation is updating Windows integration' }
    if ($Scope -eq 'Machine') {
        $handoffMutex = [Threading.Mutex]::new($false, $userMutexName)
        try { $handoffLockAcquired = $handoffMutex.WaitOne(30000) } catch [Threading.AbandonedMutexException] { $handoffLockAcquired = $true }
        if (!$handoffLockAcquired) { throw 'A user installation is updating Windows integration' }
    }
    Assert-NoReparse $InstallDirectory
    foreach ($backupSuffix in @('','.pending','.pending-write','.next')) { Assert-NoReparse "$statePath$backupSuffix" }
    if ($Scope -eq 'Machine' -and $userBaseDirectory) {
        foreach ($backupSuffix in @('','.pending','.handoff-next')) { Assert-NoReparse "$(Join-Path $userBaseDirectory 'integration-state.json')$backupSuffix" }
    }
    if (Test-Path -LiteralPath "$statePath.pending") {
        $journal = Get-Content -LiteralPath "$statePath.pending" -Raw | ConvertFrom-Json
        Assert-Identity $journal $Scope $physicalHive $registryIdentity $classes 'JournalVersion'
        $pendingDirectory = $journal.InstallDirectory
        $pendingVersion = $journal.Version
        if ($journal.JournalVersion -eq 1) {
            $providerWrite = $journal.Writes | Where-Object { $_.Path -eq "$classes\CLSID\$clsid\InprocServer32" -and $_.Name -eq '' } | Select-Object -First 1
            $pendingDirectory = if ($providerWrite) { Split-Path $providerWrite.New.Data -Parent } else { $InstallDirectory }
            $pendingVersion = ($journal.Writes | Where-Object { $_.Path -eq $uninstall -and $_.Name -eq 'DisplayVersion' } | Select-Object -First 1).New.Data
            if (!$pendingVersion) { $pendingVersion = $Version }
        }
        Assert-Directory $pendingDirectory $baseDirectory
        $manifest = @(Get-Manifest $pendingDirectory $pendingVersion $classes $settings $registeredApplications $uninstall)
        if (!$journal.Writes) { throw 'Installation journal has no recoverable writes' }
        $seen = @{}
        foreach ($item in $journal.Writes) {
            $expected = $manifest | Where-Object { $_.Path -eq $item.Path -and $_.Name -eq $item.Name } | Select-Object -First 1
            $itemId = $item.Path + [char]0 + $item.Name
            if (!$expected -or $seen.ContainsKey($itemId) -or !(Same-State $item.New $expected.Value)) { throw 'Installation journal contains an unexpected registry write' }
            $seen[$itemId] = $true
            Assert-ValueState $item.Value
            Assert-ValueState $item.New
        }
        if ($journal.PreviousState) { Assert-State $journal.PreviousState $Scope $physicalHive $registryIdentity $baseDirectory $classes $settings $registeredApplications $uninstall }
        if ($journal.UserHandoff) {
            $handoff = $journal.UserHandoff
            if ($Scope -ne 'Machine' -or !$userBaseDirectory -or $handoff.Scope -ne 'User' -or $handoff.RegistryHive -ne 'CurrentUser' -or $handoff.RegistryIdentity -ne 'User:CurrentUser' -or $handoff.RegistryRoot -ne $userClasses -or $handoff.StatePath -ne (Join-Path $userBaseDirectory 'integration-state.json')) { throw 'Installation journal contains an incompatible user handoff' }
            Assert-State $handoff.PreviousState 'User' 'CurrentUser' 'User:CurrentUser' $userBaseDirectory $userClasses $userSettings $userRegisteredApplications $userUninstall
            $seen = @{}
            foreach ($item in @($handoff.Writes)) {
                $entry = $handoff.PreviousState.Entries | Where-Object { $_.Path -eq $item.Path -and $_.Name -eq $item.Name } | Select-Object -First 1
                $itemId = $item.Path + [char]0 + $item.Name
                if (!$entry -or $seen.ContainsKey($itemId) -or !(Same-State $item.Value $entry.Owned) -or !(Same-State $item.New $entry.Original)) { throw 'Installation journal contains an unproven user handoff write' }
                $seen[$itemId] = $true
            }
        }
        # Validate the complete journal before modifying either registry hive.
        foreach ($item in $journal.Writes) {
            if (Same-State (Get-ValueState $item.Path $item.Name) $item.New) {
                Set-ValueState $item.Path $item.Name $item.Value
                Remove-EmptyKey $item.Path
            }
        }
        if ($journal.UserHandoff) {
            foreach ($item in @($journal.UserHandoff.Writes)) {
                if (Same-State (Get-ValueState $item.Path $item.Name $userRegistry) $item.New) {
                    Set-ValueState $item.Path $item.Name $item.Value $userRegistry
                }
            }
            Restore-UserHandoffState $journal.UserHandoff
        }
        if ($journal.PreviousState) {
            $journal.PreviousState | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $statePath -Encoding UTF8
        } elseif (Test-Path -LiteralPath $statePath) { Remove-Item -LiteralPath $statePath }
        Remove-Item -LiteralPath "$statePath.pending"
        if (Test-Path -LiteralPath "$statePath.next") { Remove-Item -LiteralPath "$statePath.next" }
        Write-Host 'Recovered an interrupted integration update without replacing later third-party changes.'
    }
    $state = $null
    if (Test-Path -LiteralPath $statePath) {
        $state = Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json
        Assert-State $state $Scope $physicalHive $registryIdentity $baseDirectory $classes $settings $registeredApplications $uninstall
    }
    if ($Action -eq 'Uninstall') {
        if (!$state -or $state.InstallDirectory -ne $InstallDirectory) { Write-Output 'STALE_INSTALL'; exit 2 }
        foreach ($entry in @($state.Entries) | Sort-Object { $_.Path.Length } -Descending) {
            if (Same-State (Get-ValueState $entry.Path $entry.Name) $entry.Owned) {
                Set-ValueState $entry.Path $entry.Name $entry.Original
                Remove-EmptyKey $entry.Path
            }
        }
        Remove-Item -LiteralPath $statePath
        Notify-Shell
        Write-Output 'UNINSTALLED'
        exit 0
    }
    foreach ($file in @('video-player.exe','video_player_thumbnail.dll','thumbnail-worker.exe','libmpv-2.dll','avformat-62.dll','avcodec-62.dll','avutil-60.dll','avfilter-11.dll','swscale-9.dll','swresample-6.dll')) {
        if (!(Test-Path -LiteralPath (Join-Path $InstallDirectory $file))) { throw "Incomplete installation: $file" }
    }
    $writes = @(Get-Manifest $InstallDirectory $Version $classes $settings $registeredApplications $uninstall)
    $entries = [Collections.Generic.List[object]]::new()
    # Keep the committed state immutable for journal rollback.
    if ($state) {
        foreach ($entry in (($state | ConvertTo-Json -Depth 10 | ConvertFrom-Json).Entries)) {
            if (!$entry.OwnershipInstallDirectory) { $entry | Add-Member -NotePropertyName OwnershipInstallDirectory -NotePropertyValue $state.InstallDirectory -Force }
            if (!$entry.OwnershipVersion) { $entry | Add-Member -NotePropertyName OwnershipVersion -NotePropertyValue (Get-StateVersion $state $uninstall) -Force }
            $entries.Add($entry)
        }
    }
    $rollback = [Collections.Generic.List[object]]::new()
    foreach ($write in $writes) {
        $current = Get-ValueState $write.Path $write.Name
        $previous = $entries | Where-Object { $_.Path -eq $write.Path -and $_.Name -eq $write.Name } | Select-Object -First 1
        if ($previous -and !(Same-State $current $previous.Owned)) {
            Write-Host "Preserving a registry value changed by another application: $($write.Path) [$($write.Name)]"
            continue
        }
        if (!$previous) {
            $previous = [pscustomobject]@{ Path=$write.Path; Name=$write.Name; Original=$current; Owned=$write.Value; OwnershipInstallDirectory=$InstallDirectory; OwnershipVersion=$Version }
            $entries.Add($previous)
        } else {
            $previous.Owned = $write.Value
            $previous.OwnershipInstallDirectory = $InstallDirectory
            $previous.OwnershipVersion = $Version
        }
        $rollback.Add([pscustomobject]@{ Path=$write.Path; Name=$write.Name; Value=$current; New=$write.Value })
    }
    $userHandoff = $null
    if ($Scope -eq 'Machine' -and $userBaseDirectory) {
        $userStatePath = Join-Path $userBaseDirectory 'integration-state.json'
        if (Test-Path -LiteralPath "$userStatePath.pending") { throw 'Finish or recover the interrupted user installation before migrating it' }
        if (Test-Path -LiteralPath $userStatePath) {
            $userState = Get-Content -LiteralPath $userStatePath -Raw | ConvertFrom-Json
            Assert-State $userState 'User' 'CurrentUser' 'User:CurrentUser' $userBaseDirectory $userClasses $userSettings $userRegisteredApplications $userUninstall
            $handoffWrites = [Collections.Generic.List[object]]::new()
            foreach ($entry in @($userState.Entries) | Sort-Object { $_.Path.Length } -Descending) {
                if (Same-State (Get-ValueState $entry.Path $entry.Name $userRegistry) $entry.Owned) {
                    $handoffWrites.Add([pscustomobject]@{ Path=$entry.Path; Name=$entry.Name; Value=$entry.Owned; New=$entry.Original })
                } else { Write-Host "Preserving a user registry value changed by another application: $($entry.Path) [$($entry.Name)]" }
            }
            $userHandoff = [pscustomobject]@{ Scope='User'; RegistryHive='CurrentUser'; RegistryIdentity='User:CurrentUser'; RegistryRoot=$userClasses; StatePath=$userStatePath; PreviousState=$userState; Writes=@($handoffWrites.ToArray()) }
        }
    }
    $next = [pscustomobject]@{ SchemaVersion=2; Scope=$Scope; RegistryHive=$physicalHive; RegistryIdentity=$registryIdentity; RegistryRoot=$classes; InstallDirectory=$InstallDirectory; Version=$Version; Entries=@($entries.ToArray()) }
    New-Item -ItemType Directory -Force -Path $baseDirectory | Out-Null
    $journal = [pscustomobject]@{ JournalVersion=2; Scope=$Scope; RegistryHive=$physicalHive; RegistryIdentity=$registryIdentity; RegistryRoot=$classes; InstallDirectory=$InstallDirectory; Version=$Version; PreviousState=$state; Writes=@($rollback.ToArray()); UserHandoff=$userHandoff }
    $journal | ConvertTo-Json -Depth 14 | Set-Content -LiteralPath "$statePath.pending-write" -Encoding UTF8
    Move-Item -LiteralPath "$statePath.pending-write" -Destination "$statePath.pending" -Force
    try {
        foreach ($item in $rollback) { Set-ValueState $item.Path $item.Name $item.New }
        if ($userHandoff) {
            foreach ($item in @($userHandoff.Writes)) {
                if (Same-State (Get-ValueState $item.Path $item.Name $userRegistry) $item.Value) {
                    Set-ValueState $item.Path $item.Name $item.New $userRegistry
                    Remove-EmptyKey $item.Path $userRegistry $userClasses $userSettings $userUninstall
                } else { Write-Host "Preserving a user registry value changed during migration: $($item.Path) [$($item.Name)]" }
            }
            Assert-NoReparse $userHandoff.StatePath
            Remove-Item -LiteralPath $userHandoff.StatePath
        }
        $next | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath "$statePath.next" -Encoding UTF8
        Move-Item -LiteralPath "$statePath.next" -Destination $statePath -Force
        Remove-Item -LiteralPath "$statePath.pending"
    } catch {
        # The durable journal remains available if rollback itself is interrupted.
        foreach ($item in $rollback) {
            if (Same-State (Get-ValueState $item.Path $item.Name) $item.New) {
                Set-ValueState $item.Path $item.Name $item.Value
                Remove-EmptyKey $item.Path
            }
        }
        if ($userHandoff) {
            foreach ($item in @($userHandoff.Writes)) {
                if (Same-State (Get-ValueState $item.Path $item.Name $userRegistry) $item.New) { Set-ValueState $item.Path $item.Name $item.Value $userRegistry }
            }
            Restore-UserHandoffState $userHandoff
        }
        throw
    }
    if ($Scope -eq 'Machine') {
        if ($userHandoff -and !$TestRegistryPrefix) {
            $shortcutPath = Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::Programs)) 'Video Player.lnk'
            if (Test-Path -LiteralPath $shortcutPath) {
                $shortcutShell = $shortcut = $null
                try {
                    $shortcutShell = New-Object -ComObject WScript.Shell
                    $shortcut = $shortcutShell.CreateShortcut($shortcutPath)
                    if ($shortcut.TargetPath -eq (Join-Path $userHandoff.PreviousState.InstallDirectory 'video-player.exe') -and !$shortcut.Arguments) {
                        Assert-NoReparse $shortcutPath
                        Remove-Item -LiteralPath $shortcutPath
                    }
                } catch { Write-Host 'WARNING: The retired user shortcut could not be removed; the machine installation remains registered.' }
                finally {
                    if ($shortcut) { [void][Runtime.InteropServices.Marshal]::ReleaseComObject($shortcut) }
                    if ($shortcutShell) { [void][Runtime.InteropServices.Marshal]::ReleaseComObject($shortcutShell) }
                }
            }
        }
        foreach ($ext in $videoExtensions) {
            $hasUserProvider = $false
            foreach ($providerPath in @("$userClasses\SystemFileAssociations\$ext\ShellEx\$thumbnailInterface", "$userClasses\$ext\ShellEx\$thumbnailInterface", "$userClasses\VideoPlayer$ext\ShellEx\$thumbnailInterface")) {
                if ((Get-ValueState $providerPath '' $userRegistry).Exists) { $hasUserProvider = $true }
            }
            if ($hasUserProvider) { Write-Host "WARNING: A preserved user thumbnail registration for $ext can take precedence over the machine provider." }
        }
        if ((Get-ValueState "$userClasses\CLSID\$clsid\InprocServer32" '' $userRegistry).Exists) { Write-Host 'WARNING: A preserved user COM registration can take precedence over the machine provider.' }
    }
    Notify-Shell
    Write-Output 'INSTALLED'
} finally {
    $registry.Dispose()
    if ($userRegistry) { $userRegistry.Dispose() }
    if ($handoffLockAcquired) { $handoffMutex.ReleaseMutex() }
    if ($handoffMutex) { $handoffMutex.Dispose() }
    if ($lockAcquired) { $mutex.ReleaseMutex() }
    $mutex.Dispose()
}
