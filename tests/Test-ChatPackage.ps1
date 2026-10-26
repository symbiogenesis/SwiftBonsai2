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
    # The actual copied launcher dependency closure runs with synthetic native
    # operations. No compiler, checkout files, model, microphone or process runs.
    $first = New-ChatPackage -OutputDirectory (Join-Path $scratch 'package-launcher') -Version '0.3.0' -SourceRevision ('c' * 40) -Binary $binary -ThirdPartyNoticePath $notices
    $root = Join-Path $scratch 'launcher-installation'
    $installed = Set-ChatPackage -InstallationDirectory $root -PackageDirectory $first.Directory -ManifestSha256 $first.ManifestSha256
    $data = Join-Path $scratch 'data'
    [void][IO.Directory]::CreateDirectory($data)
    $native = Import-Module (Join-Path $installed.Directory 'scripts/Bonsai.Native.psm1') -Force -PassThru
    & $native {
        $script:Calls = [Collections.Generic.List[object]]::new()
        function script:Invoke-BonsaiNativeProcess {
            [CmdletBinding()]
            [OutputType([pscustomobject])]
            param([string]$Executable, [string[]]$ArgumentList, [string]$WorkingDirectory, [int]$TimeoutSeconds, [string]$LogPath)
            $script:Calls.Add([pscustomobject]@{ Executable = $Executable; Arguments = $ArgumentList; WorkingDirectory = $WorkingDirectory; TimeoutSeconds = $TimeoutSeconds; LogPath = $LogPath })
            $output = ''
            if ([IO.Path]::GetFileName($Executable) -ceq 'bonsai-tools.exe') {
                switch ($ArgumentList[0]) {
                    '--version' { $output = '{"Name":"bonsai-tools","Version":"0.1.0","ProtocolVersion":5,"Commands":["credentials","speech-models"]}' }
                    'speech-models' { $output = '{"Plans":[],"CanaryModelPath":"canary.gguf","TtsModelPath":"talker.gguf","TtsCodecPath":"codec.gguf","TotalBytes":1}' }
                    'credentials' {
                        $request = Get-Content -LiteralPath $ArgumentList[2] -Raw | ConvertFrom-Json
                        $key = Join-Path $request.parentDirectory 'fixture.key'
                        [IO.File]::WriteAllText($key, ('a' * 64))
                        $output = ConvertTo-Json -InputObject @{ FrontendKeyFile = $key }
                    }
                    default { throw 'Unexpected packaged maintenance command.' }
                }
            } elseif ([IO.Path]::GetFileName($Executable) -cne 'bonsai-chat.exe') { throw 'Packaged launcher attempted a build or unexpected executable.' }
            return [pscustomobject]@{ ExitCode = 0; StandardOutput = $output; StandardError = '' }
        }
    }
    $launcher = Join-Path $installed.Directory 'scripts/Start-InstalledChat.ps1'
    & $launcher -InstallationDirectory $root -DataDirectory $data -NoSpeechModels | Out-Null
    $calls = @(& $native { $script:Calls.ToArray() })
    Assert ($calls.Count -eq 1 -and [IO.Path]::GetFileName($calls[0].Executable) -ceq 'bonsai-chat.exe') 'Text-only packaged launch used extra dependencies.'
    Assert ($calls[0].Arguments -cnotcontains '--canary-executable') 'Text-only packaged launch enabled speech.'
    & $launcher -InstallationDirectory $root -DataDirectory $data | Out-Null
    $calls = @(& $native { $script:Calls.ToArray() })
    $chat = $calls[-1]
    Assert ($chat.Arguments -ccontains '--canary-executable' -and $chat.Arguments -ccontains '--tts-executable') 'Owned speech package paths were omitted.'
    foreach ($call in $calls) {
        Assert ($call.Executable.StartsWith((Join-Path $installed.Directory 'bin'), [StringComparison]::OrdinalIgnoreCase)) 'Packaged launch used a compiler or external executable.'
        Assert ([IO.Path]::IsPathFullyQualified($call.WorkingDirectory) -and -not $call.WorkingDirectory.StartsWith($installed.Directory, [StringComparison]::OrdinalIgnoreCase)) 'Native process did not retain its external scratch lane.'
    }
    Assert (@($calls | Where-Object { $_.Arguments[0] -ceq 'speech-models' }).Count -eq 1) 'Packaged speech provisioning did not use the supplied tool.'
    Assert (@($calls | Where-Object { $_.Arguments[0] -ceq 'credentials' }).Count -eq 2) 'Packaged credentials did not use the supplied tool.'
    & $launcher -InstallationDirectory $root -DataDirectory $data -NoTts | Out-Null
    $calls = @(& $native { $script:Calls.ToArray() })
    Assert ($calls[-1].Arguments -ccontains '--canary-executable' -and $calls[-1].Arguments -cnotcontains '--tts-executable') 'Packaged NoTts setup lost input or retained output.'
}
"Chat package $Case checks passed. Evidence: $scratch"
