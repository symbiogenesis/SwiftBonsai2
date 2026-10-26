#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')

function Get-SpeechModelPlan {
    <#
    .SYNOPSIS
    Validates the complete pinned speech download plan without writing or networking.
    #>
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param(
        [Parameter(Mandatory)][string]$DataDirectory,
        [ValidateSet('All', 'Canary', 'QwenTts')][string]$Model = 'All'
    )
    $root = Resolve-ExternalPath -Path $DataDirectory
    $textFiles = @('config.json', 'generation_config.json', 'merges.txt', 'tokenizer_config.json', 'vocab.json')
    $identities = @(
        [pscustomobject]@{
            Kind = 'Canary'; Model = 'nvidia/canary-qwen-2.5b'
            Revision = 'b1469e1bba1cfe140205529c79c434ca47180960'
            Directory = 'canary'; Lock = '../tools/canary-asr/model.lock.json'
            Names = @('config.json', 'model.safetensors')
        },
        [pscustomobject]@{
            Kind = 'CanaryTokenizer'; Model = 'Qwen/Qwen3-1.7B'
            Revision = '70d244cc86ccca08cf5af4e1e306ecf908b1ad5e'
            Directory = 'canary-tokenizer'; Lock = '../tools/canary-asr/tokenizer.lock.json'
            Names = @($textFiles) + @('tokenizer.json')
        },
        [pscustomobject]@{
            Kind = 'QwenTts'; Model = 'Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign'
            Revision = '5ecdb67327fd37bb2e042aab12ff7391903235d3'
            Directory = 'qwen-tts'; Lock = '../tools/qwen-tts/model.lock.json'
            Names = @($textFiles) + @('model.safetensors', 'preprocessor_config.json',
                'speech_tokenizer/config.json', 'speech_tokenizer/configuration.json',
                'speech_tokenizer/model.safetensors', 'speech_tokenizer/preprocessor_config.json')
        }
    )
    $plans = [Collections.Generic.List[object]]::new()
    foreach ($identity in $identities) {
        if (($Model -eq 'Canary' -and $identity.Kind -eq 'QwenTts') -or
            ($Model -eq 'QwenTts' -and $identity.Kind -ne 'QwenTts')) { continue }
        $lock = Get-Content -LiteralPath (Join-Path $PSScriptRoot $identity.Lock) -Raw |
            ConvertFrom-Json -DateKind String -AsHashtable -NoEnumerate
        if ($lock -isnot [Collections.IDictionary] -or $lock.schema_version -ne 1 -or
            $lock.model -cne $identity.Model -or $lock.revision -cne $identity.Revision -or
            $lock.files -isnot [Collections.IDictionary] -or $lock.files.Count -ne $identity.Names.Count) {
            throw 'Invalid or unsupported immutable speech model lock.'
        }
        foreach ($name in $lock.files.Keys) {
            if ($name -cnotin $identity.Names) { throw 'Speech lock contains an unsupported artifact path.' }
        }
        $snapshot = Resolve-ExternalPath -Path (Join-Path $root -ChildPath 'speech', $identity.Directory, $identity.Revision)
        $files = [Collections.Generic.List[object]]::new()
        foreach ($name in ($identity.Names | Sort-Object -CaseSensitive)) {
            $artifact = $lock.files[$name]
            if ($artifact -isnot [Collections.IDictionary] -or
                ($artifact.bytes -isnot [long] -and $artifact.bytes -isnot [int]) -or
                $artifact.bytes -le 0 -or $artifact.sha256 -cnotmatch '^[a-f0-9]{64}$') {
                throw 'Speech artifacts require an exact positive byte length and lowercase SHA-256.'
            }
            $expectedUrl = "https://huggingface.co/$($identity.Model)/resolve/$($identity.Revision)/$name"
            if ($artifact.url -isnot [string] -or $artifact.url -cne $expectedUrl) {
                throw 'Speech artifact URL disagrees with its immutable model, revision or filename.'
            }
            $path = Resolve-ExternalPath -Path (Join-Path $snapshot $name)
            if (-not $path.StartsWith($snapshot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
                throw 'Speech artifact path escaped its snapshot.'
            }
            $files.Add([pscustomobject]@{
                Name = $name; Path = $path; Uri = [uri]$expectedUrl
                Bytes = [long]$artifact.bytes; Sha256 = [string]$artifact.sha256
            })
        }
        $plans.Add([pscustomobject]@{
            Kind = $identity.Kind; Model = $identity.Model; Revision = $identity.Revision
            DataDirectory = $root; SnapshotPath = $snapshot; Files = $files.ToArray()
        })
    }
    return $plans.ToArray()
}

function Assert-SpeechProvisionPlan {
    [CmdletBinding()]
    [OutputType([void])]
    param([Parameter(Mandatory)][object[]]$Plan)
    if ($Plan.Count -lt 1 -or $Plan.Count -gt 3) { throw 'Invalid speech snapshot selection.' }
    $kinds = @($Plan | ForEach-Object { $_.Kind })
    $selection = if ($kinds.Count -eq 1 -and $kinds[0] -ceq 'QwenTts') { 'QwenTts' }
        elseif ($kinds.Count -eq 2 -and 'Canary' -cin $kinds -and 'CanaryTokenizer' -cin $kinds) { 'Canary' }
        elseif ($kinds.Count -eq 3 -and 'Canary' -cin $kinds -and 'CanaryTokenizer' -cin $kinds -and 'QwenTts' -cin $kinds) { 'All' }
        else { throw 'Canary recognition requires both its model and tokenizer snapshots.' }
    $expected = @(Get-SpeechModelPlan -DataDirectory $Plan[0].DataDirectory -Model $selection)
    foreach ($snapshot in $expected) {
        $supplied = @($Plan | Where-Object { $_.Kind -ceq $snapshot.Kind })
        if ($supplied.Count -ne 1) { throw 'Speech snapshot kinds must be unique.' }
        $actual = $supplied[0]
        if ($actual.Model -cne $snapshot.Model -or $actual.Revision -cne $snapshot.Revision -or
            $actual.DataDirectory -cne $snapshot.DataDirectory -or $actual.SnapshotPath -cne $snapshot.SnapshotPath -or
            @($actual.Files).Count -ne $snapshot.Files.Count) {
            throw 'Speech snapshot plan disagrees with its reviewed lock.'
        }
        foreach ($file in $snapshot.Files) {
            $suppliedFile = @($actual.Files | Where-Object { $_.Name -ceq $file.Name })
            if ($suppliedFile.Count -ne 1 -or $suppliedFile[0].Path -cne $file.Path -or
                [string]$suppliedFile[0].Uri -cne [string]$file.Uri -or $suppliedFile[0].Bytes -ne $file.Bytes -or
                $suppliedFile[0].Sha256 -cne $file.Sha256) {
                throw 'Speech artifact plan disagrees with its reviewed lock.'
            }
        }
    }
}

function Test-SpeechSnapshotEntry {
    [CmdletBinding()]
    [OutputType([void])]
    param([Parameter(Mandatory)][pscustomobject]$Snapshot, [switch]$AllowPartial)
    $root = Resolve-ExternalPath -Path $Snapshot.SnapshotPath
    if (-not (Test-Path -LiteralPath $root)) { return }
    if (-not (Test-Path -LiteralPath $root -PathType Container)) { throw 'Speech snapshot path must be a directory.' }
    $names = @($Snapshot.Files | ForEach-Object { $_.Name })
    $folders = @($names | Where-Object { $_.Contains('/') } | ForEach-Object { $_.Substring(0, $_.LastIndexOf('/')) } | Select-Object -Unique)
    $documentation = @('README.md', 'LICENSE', 'LICENSE.txt', 'NOTICE', 'NOTICE.txt', '.gitattributes')
    $pending = [Collections.Generic.Stack[string]]::new()
    $pending.Push($root)
    while ($pending.Count -gt 0) {
        foreach ($entry in [IO.Directory]::EnumerateFileSystemEntries($pending.Pop())) {
            $item = Get-Item -LiteralPath $entry -Force
            if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Speech snapshots cannot contain reparse entries.' }
            $relative = [IO.Path]::GetRelativePath($root, $entry).Replace('\', '/')
            if ($item.PSIsContainer) {
                if ($relative -cnotin $folders) { throw "Unsupported speech snapshot directory: $relative" }
                $pending.Push($entry)
            } elseif ($relative -cnotin $names -and
                -not ($AllowPartial -and $relative.EndsWith('.partial', [StringComparison]::Ordinal) -and $relative.Substring(0, $relative.Length - 8) -cin $names) -and
                -not (-not $relative.Contains('/') -and $relative -cin $documentation)) {
                throw "Unexpected speech snapshot file: $relative. Preserve or remove it outside the snapshot before retrying; retained partial files are not ready model artifacts."
            }
        }
    }
}

function Invoke-SpeechModelProvision {
    <#
    .SYNOPSIS
    Downloads or verifies every artifact from a completely validated speech plan.
    #>
    [CmdletBinding()]
    [OutputType([void])]
    param([Parameter(Mandatory)][object[]]$Plan, [switch]$VerifyOnly)
    Assert-SpeechProvisionPlan -Plan $Plan
    foreach ($snapshot in $Plan) { Test-SpeechSnapshotEntry -Snapshot $snapshot -AllowPartial:(-not $VerifyOnly) }
    $count = 0
    $total = [int](($Plan | ForEach-Object { $_.Files.Count } | Measure-Object -Sum).Sum)
    try {
        foreach ($snapshot in $Plan) {
            foreach ($file in $snapshot.Files) {
                $size = if ($file.Bytes -ge 1GB) { '{0:N2} GiB' -f ($file.Bytes / 1GB) } else { '{0:N0} KiB' -f ($file.Bytes / 1KB) }
                $action = if ($VerifyOnly) { 'Verifying' } else { 'Downloading/verifying' }
                Write-Information -MessageData "$action $($snapshot.Model): $($file.Name) ($size)" -InformationAction Continue
                Write-Progress -Id 73 -Activity 'Preparing speech models' -Status "$($snapshot.Kind): $($file.Name)" -PercentComplete ([int](100 * $count / $total))
                if ($VerifyOnly) { Test-LockedArtifact -Path $file.Path -Bytes $file.Bytes -Sha256 $file.Sha256 }
                else { Receive-LockedArtifact -Uri $file.Uri -Path $file.Path -Bytes $file.Bytes -Sha256 $file.Sha256 | Out-Null }
                $count++
            }
        }
        foreach ($snapshot in $Plan) { Test-SpeechSnapshotEntry -Snapshot $snapshot }
    } finally { Write-Progress -Id 73 -Activity 'Preparing speech models' -Completed }
}

Export-ModuleMember -Function Get-SpeechModelPlan, Invoke-SpeechModelProvision
