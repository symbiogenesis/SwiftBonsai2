#requires -Version 7.6
[CmdletBinding()]
[OutputType([string])]
param([ValidateSet('All', 'Lifecycle', 'Rejections', 'Launcher')][string]$Case = 'All')
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if ($Case -ceq 'All' -and $env:GITHUB_ACTIONS -cne 'true') { throw 'Complete suites run only in GitHub Actions. Select a focused case locally.' }
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.ChatPackage.psm1')
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-ChatPackage-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($scratch)
$image = Join-Path $scratch 'fixture.exe'
$stream = [IO.File]::Open($image, [IO.FileMode]::CreateNew)
$writer = [IO.BinaryWriter]::new($stream)
try {
    $stream.SetLength(512)
    $writer.Write([uint16]0x5a4d); $stream.Position = 0x3c; $writer.Write([int]0x80)
    $stream.Position = 0x80; $writer.Write([uint32]0x00004550); $writer.Write([uint16]0xaa64)
} finally { $writer.Dispose() }
$notices = Join-Path $scratch 'notices.txt'
[IO.File]::WriteAllText($notices, 'Synthetic notices for package lifecycle fixtures; not distribution notices.')
$binary = @{ 'bonsai-chat' = $image; 'bonsai-broker' = $image; 'bonsai-asr' = $image; 'bonsai-tts' = $image; 'bonsai-tools' = $image }
$first = New-ChatPackage -OutputDirectory (Join-Path $scratch 'package-one') -Version '0.1.0' -SourceRevision ('a' * 40) -Binary $binary -ThirdPartyNoticePath $notices
$root = Join-Path $scratch 'installation'

if ($Case -cin @('All', 'Lifecycle')) {
    $one = Set-ChatPackage -InstallationDirectory $root -PackageDirectory $first.Directory -ManifestSha256 $first.ManifestSha256
    Assert ($one.Id -ceq $first.Id -and $one.Directory -cne $first.Directory) 'Install did not publish an immutable version.'
    $pointerPath = Join-Path $root 'active.json'
    $initial = [IO.File]::ReadAllText($pointerPath)
    [void](Set-ChatPackage -InstallationDirectory $root -PackageDirectory $first.Directory -ManifestSha256 $first.ManifestSha256)
    Assert ([IO.File]::ReadAllText($pointerPath) -ceq $initial) 'Idempotent install changed rollback state.'
    $uppercase = Set-ChatPackage -InstallationDirectory $root -PackageDirectory $first.Directory -ManifestSha256 $first.ManifestSha256.ToUpperInvariant()
    Assert ($uppercase.Id -ceq $first.Id -and (Get-InstalledChatPackage -InstallationDirectory $root).Id -ceq $first.Id -and
        [IO.File]::ReadAllText($pointerPath) -ceq $initial) 'Uppercase manifest hex created an unreadable selection or changed rollback state.'
    Assert-Failure { Set-ChatPackage -InstallationDirectory $root -Rollback } 'First install had an invented previous version.'
    $second = New-ChatPackage -OutputDirectory (Join-Path $scratch 'package-two') -Version '0.2.0' -SourceRevision ('b' * 40) -Binary $binary -ThirdPartyNoticePath $notices
    [void](Set-ChatPackage -InstallationDirectory $root -PackageDirectory $second.Directory -ManifestSha256 $second.ManifestSha256)
    Assert ((Get-InstalledChatPackage -InstallationDirectory $root).Id -ceq $second.Id) 'Update did not select the new version.'
    Assert ([IO.Directory]::Exists($one.Directory)) 'Update removed the rollback version.'
    [void](Set-ChatPackage -InstallationDirectory $root -Rollback)
    Assert ((Get-InstalledChatPackage -InstallationDirectory $root).Id -ceq $first.Id) 'Rollback did not restore the previous package.'
    [void](Set-ChatPackage -InstallationDirectory $root -Rollback)
    Assert ((Get-InstalledChatPackage -InstallationDirectory $root).Id -ceq $second.Id) 'Rollback failed to retain the displaced version.'
    Assert ([IO.File]::ReadAllText((Join-Path $one.Directory 'THIRD-PARTY-NOTICES.txt')) -ceq [IO.File]::ReadAllText($notices)) 'Supplied notices were changed or omitted.'
}

if ($Case -cin @('All', 'Rejections')) {
    Assert-Failure { Get-CheckedChatPackage -PackageDirectory $first.Directory -ManifestSha256 ('0' * 64) } 'A package with an unreviewed manifest hash was accepted.'
    Assert-Failure { New-ChatPackage -OutputDirectory $first.Directory -Version '0.1.0' -SourceRevision ('a' * 40) -Binary $binary -ThirdPartyNoticePath $notices } 'An existing package was replaced.'
    $extra = Join-Path $first.Directory 'extra'
    [void][IO.Directory]::CreateDirectory($extra)
    Assert-Failure { Get-CheckedChatPackage -PackageDirectory $first.Directory -ManifestSha256 $first.ManifestSha256 } 'An unlisted package directory was accepted.'
    [IO.Directory]::Delete($extra, $false)
    [void](Set-ChatPackage -InstallationDirectory $root -PackageDirectory $first.Directory -ManifestSha256 $first.ManifestSha256)
    $pointerPath = Join-Path $root 'active.json'
    $initial = [IO.File]::ReadAllText($pointerPath)
    $guard = [IO.File]::Open((Join-Path $root 'package.lock'), [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    try { Assert-Failure { Set-ChatPackage -InstallationDirectory $root -PackageDirectory $first.Directory -ManifestSha256 $first.ManifestSha256 } 'Concurrent activation ignored the installation lock.' }
    finally { $guard.Dispose() }
    $file = Join-Path $first.Directory 'bin/bonsai-chat.exe'
    [IO.File]::WriteAllText($file, 'tampered')
    Assert-Failure { Set-ChatPackage -InstallationDirectory $root -PackageDirectory $first.Directory -ManifestSha256 $first.ManifestSha256 } 'Tampered source bytes were installed.'
    Assert ([IO.File]::ReadAllText($pointerPath) -ceq $initial) 'Rejected update changed the active selection.'
    $installed = Get-InstalledChatPackage -InstallationDirectory $root
    [IO.File]::WriteAllText((Join-Path $installed.Directory 'NOTICE'), 'tampered')
    Assert-Failure { Get-InstalledChatPackage -InstallationDirectory $root } 'Changed installed bytes were accepted for launch.'
    $second = New-ChatPackage -OutputDirectory (Join-Path $scratch 'rejection-update') -Version '0.2.1' -SourceRevision ('d' * 40) -Binary $binary -ThirdPartyNoticePath $notices
    [void](Set-ChatPackage -InstallationDirectory $root -PackageDirectory $second.Directory -ManifestSha256 $second.ManifestSha256)
    $updated = [IO.File]::ReadAllText($pointerPath)
    Assert-Failure { Set-ChatPackage -InstallationDirectory $root -Rollback } 'Rollback selected tampered previous bytes.'
    Assert ([IO.File]::ReadAllText($pointerPath) -ceq $updated) 'Rejected rollback changed the active selection.'
    $manifestPath = Join-Path $first.Directory 'package.json'
    $manifest = [IO.File]::ReadAllText($manifestPath) | ConvertFrom-Json -AsHashtable
    $manifest.files[0].name = '../escape.exe'
    [IO.File]::WriteAllText($manifestPath, (ConvertTo-Json -InputObject $manifest -Depth 8))
    $digest = (Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).Hash.ToLowerInvariant()
    Assert-Failure { Get-CheckedChatPackage -PackageDirectory $first.Directory -ManifestSha256 $digest } 'A manifest-controlled path escaped the fixed layout.'
    [IO.File]::WriteAllText($pointerPath, '{"schemaVersion":1,"active":{"id":"../escape","manifestSha256":"' + ('a' * 64) + '"},"previous":null}')
    Assert-Failure { Get-InstalledChatPackage -InstallationDirectory $root } 'An activation pointer escaped version storage.'
    Assert-Failure { Set-ChatPackage -InstallationDirectory 'relative-output' -PackageDirectory $first.Directory -ManifestSha256 $first.ManifestSha256 } 'Relative installation storage was accepted.'
    Assert-Failure { Set-ChatPackage -InstallationDirectory '%LOCALAPPDATA%/Bonsai' -PackageDirectory $first.Directory -ManifestSha256 $first.ManifestSha256 } 'An unresolved environment variable was accepted.'
}

if ($Case -cin @('All', 'Launcher')) {
    # Exercise the real copied PowerShell dependency closure. Rust owns startup
    # behavior; this bridge check supplies no checkout, compiler or model.
    $first = New-ChatPackage -OutputDirectory (Join-Path $scratch 'package-launcher') -Version '0.3.0' -SourceRevision ('c' * 40) -Binary $binary -ThirdPartyNoticePath $notices
    $root = Join-Path $scratch 'launcher-installation'
    $installed = Set-ChatPackage -InstallationDirectory $root -PackageDirectory $first.Directory -ManifestSha256 $first.ManifestSha256
    $data = Join-Path $scratch 'data'
    [void][IO.Directory]::CreateDirectory($data)
    # The synthetic PE cannot execute. Keep the real trust bootstrap, replacing
    # only its final process boundary with the inert full-package checker.
    $packages = Import-Module (Join-Path $installed.Directory 'scripts/Bonsai.ChatPackage.psm1') -Force -PassThru
    & $packages {
        $script:Inspections = 0
        function script:Invoke-ChatPackageInspection {
            [CmdletBinding()]
            [OutputType([hashtable])]
            param([string]$ToolPath, [string]$InstallationDirectory)
            $script:Inspections++
            $selection = Read-ChatPackageSelection -InstallationDirectory $InstallationDirectory
            $directory = Join-Path -Path $InstallationDirectory -ChildPath 'versions' -AdditionalChildPath $selection.active.id
            if ($ToolPath -cne (Join-Path $directory 'bin/bonsai-tools.exe')) { throw 'Inspection selected an unexpected maintenance executable.' }
            $package = Get-CheckedChatPackage -PackageDirectory $directory -ManifestSha256 $selection.active.manifestSha256
            return @{ Directory = $package.Directory; ManifestSha256 = $package.ManifestSha256; Manifest = $package.Manifest; Id = $package.Id }
        }
    }
    $tool = Join-Path $installed.Directory 'bin/bonsai-tools.exe'
    $originalTool = [IO.File]::ReadAllBytes($tool)
    [IO.File]::WriteAllText($tool, 'tampered')
    Assert-Failure { Get-InstalledChatPackage -InstallationDirectory $root -Native } 'A changed maintenance executable ran before its trust bootstrap.'
    Assert ((& $packages { $script:Inspections }) -eq 0) 'An unverified maintenance executable reached the process boundary.'
    [IO.File]::WriteAllBytes($tool, $originalTool)
    # Keep the single-instance gate in this fixture without changing inherited
    # profile variables or competing with the user's running chat session.
    $common = Import-Module (Join-Path $installed.Directory 'scripts/Bonsai.Common.psm1') -Force -PassThru
    & $common {
        param([string]$GuardDirectory)
        $script:OriginalResolve = ${function:Resolve-ExternalPath}
        $script:GuardDirectory = $GuardDirectory
        function script:Resolve-ExternalPath {
            [CmdletBinding()]
            [OutputType([string])]
            param([string]$Path)
            if ($Path -ceq (Join-Path $env:LOCALAPPDATA 'SwiftBonsai2')) { $Path = $script:GuardDirectory }
            & $script:OriginalResolve -Path $Path
        }
    } (Join-Path $scratch 'instance-guard')
    $native = Import-Module (Join-Path $installed.Directory 'scripts/Bonsai.Native.psm1') -Force -PassThru
    & $native {
        $script:Calls = [Collections.Generic.List[object]]::new()
        function script:Invoke-BonsaiNativeProcess {
            [CmdletBinding()]
            [OutputType([pscustomobject])]
            param([string]$Executable, [string[]]$ArgumentList, [string]$WorkingDirectory, [int]$TimeoutSeconds, [string]$LogPath)
            $request = if ($ArgumentList[0] -ceq 'chat-launch') { Get-Content -LiteralPath $ArgumentList[2] -Raw | ConvertFrom-Json -AsHashtable } else { $null }
            $script:Calls.Add(@{ Executable = $Executable; Arguments = $ArgumentList; WorkingDirectory = $WorkingDirectory; TimeoutSeconds = $TimeoutSeconds; LogPath = $LogPath; Request = $request })
            if ([IO.Path]::GetFileName($Executable) -cne 'bonsai-tools.exe') { throw 'Packaged launcher attempted a build or unexpected executable.' }
            $output = switch ($ArgumentList[0]) {
                '--version' { '{"Name":"bonsai-tools","Version":"0.1.0","ProtocolVersion":5,"StartupProtocolVersion":1,"Commands":["chat-launch"]}' }
                'chat-launch' { '{"Status":"completed"}' }
                default { throw 'Unexpected packaged maintenance command.' }
            }
            return [pscustomobject]@{ ExitCode = 0; StandardOutput = $output; StandardError = '' }
        }
    }
    $launcher = Join-Path $installed.Directory 'scripts/Start-InstalledChat.ps1'
    $originalDirectory = [Environment]::CurrentDirectory
    & $launcher -InstallationDirectory $root -DataDirectory $data -NoSpeechModels | Out-Null
    $calls = @(& $native { $script:Calls.ToArray() })
    Assert ($calls.Count -eq 2 -and $calls[-1].Request.NoSpeechModels -and -not $calls[-1].Request.ContainsKey('AsrPath')) 'Text-only package options changed at the Rust bridge.'
    & $launcher -InstallationDirectory $root -DataDirectory $data -UseRepackedPtq1 -NoAbliterateLora | Out-Null
    $calls = @(& $native { $script:Calls.ToArray() })
    $request = $calls[-1].Request
    Assert ($request.UseRepackedPtq1 -and $request.NoAbliterateLora -and
        $request.ChatPath -ceq (Join-Path $installed.Directory 'bin/bonsai-chat.exe') -and
        $request.AsrPath -ceq (Join-Path $installed.Directory 'bin/bonsai-asr.exe') -and
        $request.TtsPath -ceq (Join-Path $installed.Directory 'bin/bonsai-tts.exe')) 'Package paths or explicit model/adapter selections were not forwarded.'
    & $launcher -InstallationDirectory $root -DataDirectory $data -NoTts -Verbose:$false | Out-Null
    $calls = @(& $native { $script:Calls.ToArray() })
    Assert ($calls[-1].Request.NoTts -and $calls[-1].Request.ContainsKey('AsrPath') -and
        -not $calls[-1].Request.ContainsKey('TtsPath') -and -not $calls[-1].Request.ContainsKey('Verbose')) 'NoTts or common PowerShell parameter forwarding changed.'
    foreach ($call in $calls) {
        Assert ($call.Executable.StartsWith((Join-Path $installed.Directory 'bin'), [StringComparison]::OrdinalIgnoreCase)) 'Packaged launch used a compiler or external executable.'
        Assert ([IO.Path]::IsPathFullyQualified($call.WorkingDirectory) -and -not $call.WorkingDirectory.StartsWith($installed.Directory, [StringComparison]::OrdinalIgnoreCase)) 'Native process escaped its external scratch lane.'
    }
    Assert ([Environment]::CurrentDirectory -ceq $originalDirectory) 'The startup bridge changed the caller native working directory.'
    $before = $calls.Count
    $guard = [IO.FileStream]::new((Join-Path -Path $scratch -ChildPath 'instance-guard' -AdditionalChildPath 'chat.lock'), [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    try { & $launcher -InstallationDirectory $root -DataDirectory $data | Out-Null }
    finally { $guard.Dispose() }
    Assert ((& $native { $script:Calls.Count }) -eq $before) 'A duplicate started maintenance work before returning.'
}
"Chat package $Case checks passed. Evidence: $scratch"
