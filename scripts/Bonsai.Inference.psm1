#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')

function Read-BonsaiInferenceLock {
    <# .SYNOPSIS
    Reads the exact source, archive and executable identities of Prism Inference.
    #>
    [CmdletBinding()]
    [OutputType([Collections.IDictionary])]
    param([string]$Path = (Join-Path $PSScriptRoot '../config/prism-inference.lock.json'))
    $lock = Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json -AsHashtable -NoEnumerate -DateKind String
    if ($lock -isnot [Collections.IDictionary] -or $lock.schemaVersion -ne 1 -or
        $lock.package -cne 'prism-inference' -or $lock.version -cnotmatch '^\d+\.\d+\.\d+(?:-[a-z0-9.-]+)?$' -or
        $lock.repository -cne 'https://github.com/symbiogenesis/prism-inference' -or
        $lock.commit -cnotmatch '^[a-f0-9]{40}$' -or $lock.target -cne 'aarch64-pc-windows-msvc' -or
        $lock.release -cnotmatch '^[a-zA-Z0-9._-]+$' -or $lock.files -isnot [array]) {
        throw 'Invalid Prism Inference release identity.'
    }
    $filename = "prism-inference-$($lock.version)-$($lock.target).zip"
    if ($lock.asset.filename -cne $filename -or
        $lock.asset.url -cne "$($lock.repository)/releases/download/$($lock.release)/$filename" -or
        ($lock.asset.bytes -isnot [long] -and $lock.asset.bytes -isnot [int]) -or
        $lock.asset.bytes -le 0 -or $lock.asset.bytes -gt 1073741824 -or
        $lock.asset.sha256 -cnotmatch '^[a-f0-9]{64}$') {
        throw 'Invalid Prism Inference archive identity.'
    }
    $required = @('prism-inference.exe', 'bonsai-token-worker.exe', 'bonsai-swift-worker.exe', 'prism-inference-server.exe')
    $allowed = $required + @('bonsai-gguf.exe', 'LICENSE', 'NOTICE', 'PRISM-LICENSE.txt', 'CAMELID-LICENSE.txt')
    $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    [long]$total = 0
    foreach ($file in $lock.files) {
        if ($file -isnot [Collections.IDictionary] -or $file.name -cnotin $allowed -or
            -not $names.Add($file.name) -or
            ($file.bytes -isnot [long] -and $file.bytes -isnot [int]) -or
            $file.bytes -le 0 -or $file.bytes -gt 536870912 -or
            $file.sha256 -cnotmatch '^[a-f0-9]{64}$') {
            throw 'Invalid or duplicate Prism Inference file identity.'
        }
        $total += $file.bytes
    }
    if ($total -gt 1073741824 -or @($required | Where-Object { -not $names.Contains($_) }).Count) {
        throw 'The inference release must pin all four executables within the size bound.'
    }
    return $lock
}

function Test-BonsaiInferenceDirectory {
    <# .SYNOPSIS
    Verifies every installed file against its lock without executing a binary.
    #>
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([Parameter(Mandatory)][Collections.IDictionary]$Lock,
          [Parameter(Mandatory)][string]$Directory)
    $directoryPath = Resolve-ExternalPath -Path $Directory
    if (-not [IO.Directory]::Exists($directoryPath)) { throw 'Prism Inference is not installed.' }
    $expected = @($Lock.files.name)
    $entries = @(Get-ChildItem -LiteralPath $directoryPath -Force)
    if ($entries.Count -ne $expected.Count -or @($entries | Where-Object { $_.PSIsContainer -or $_.Name -cnotin $expected }).Count) {
        throw 'Installed inference files differ from the locked inventory.'
    }
    foreach ($file in $Lock.files) {
        $path = Resolve-ExternalPath -Path (Join-Path $directoryPath $file.name)
        Test-LockedArtifact -Path $path -Bytes $file.bytes -Sha256 $file.sha256
        if ($file.name.EndsWith('.exe', [StringComparison]::Ordinal)) { Assert-Arm64PE -Path $path }
    }
    $ggufPath = if ('bonsai-gguf.exe' -cin $expected) { Join-Path $directoryPath 'bonsai-gguf.exe' } else { $null }
    return [pscustomobject]@{
        Package = $Lock.package; Version = $Lock.version; Commit = $Lock.commit; Target = $Lock.target
        Directory = $directoryPath
        Executable = Join-Path $directoryPath 'prism-inference.exe'
        TokenWorker = Join-Path $directoryPath 'bonsai-token-worker.exe'
        SwiftWorker = Join-Path $directoryPath 'bonsai-swift-worker.exe'
        Server = Join-Path $directoryPath 'prism-inference-server.exe'
        Gguf = $ggufPath
        Verified = $true
    }
}

function Install-BonsaiInference {
    <# .SYNOPSIS
    Installs a pinned prebuilt release outside Git, without invoking Cargo.
    #>
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Installation is explicitly requested by the calling provisioning command.')]
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([Parameter(Mandatory)][string]$DataDirectory,
          [string]$LockPath = (Join-Path $PSScriptRoot '../config/prism-inference.lock.json'),
          [switch]$VerifyOnly)
    $lock = Read-BonsaiInferenceLock -Path $LockPath
    $root = Resolve-ExternalPath -Path $DataDirectory
    $destination = Resolve-ExternalPath -Path (Join-Path $root -ChildPath 'inference', $lock.commit, $lock.target)
    if ([IO.Directory]::Exists($destination)) { return Test-BonsaiInferenceDirectory -Lock $lock -Directory $destination }
    if ($VerifyOnly) { throw 'Prism Inference is not installed.' }
    $archivePath = Resolve-ExternalPath -Path (Join-Path $root -ChildPath 'downloads', $lock.asset.filename)
    Receive-LockedArtifact -Uri $lock.asset.url -Path $archivePath -Bytes $lock.asset.bytes -Sha256 $lock.asset.sha256 | Out-Null
    $parent = [IO.Path]::GetDirectoryName($destination)
    [void][IO.Directory]::CreateDirectory($parent)
    $staging = Resolve-ExternalPath -Path (Join-Path $parent ('.staging-' + [guid]::NewGuid().ToString('N')))
    [void][IO.Directory]::CreateDirectory($staging)
    try {
        # Retain the verified archive with write/delete sharing denied throughout extraction.
        $stream = [IO.FileStream]::new($archivePath, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
        try {
            if ($stream.Length -ne $lock.asset.bytes -or
                [Convert]::ToHexStringLower([Security.Cryptography.SHA256]::HashData($stream)) -cne $lock.asset.sha256) {
                throw 'Inference archive changed before extraction.'
            }
            $stream.Position = 0
            $archive = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Read, $true)
            try {
                if ($archive.Entries.Count -ne $lock.files.Count) { throw 'Inference archive inventory differs from its lock.' }
                $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
                foreach ($entry in $archive.Entries) {
                    $files = @($lock.files | Where-Object { $_.name -ceq $entry.FullName })
                    if ($files.Count -ne 1 -or -not $seen.Add($entry.FullName) -or
                        $entry.Length -ne $files[0].bytes -or (($entry.ExternalAttributes -shr 16) -band 0xF000) -eq 0xA000) {
                        throw 'Inference archive contains an unexpected, duplicate or linked entry.'
                    }
                    $inputStream = $entry.Open()
                    try {
                        $path = Join-Path $staging $entry.FullName
                        $output = [IO.FileStream]::new($path, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
                        try {
                            $buffer = [byte[]]::new(65536)
                            while (($count = $inputStream.Read($buffer, 0, $buffer.Length)) -gt 0) {
                                if ($output.Position + $count -gt $files[0].bytes) { throw 'Inference archive entry exceeded its locked size.' }
                                $output.Write($buffer, 0, $count)
                            }
                            if ($output.Position -ne $files[0].bytes) { throw 'Inference archive entry is truncated.' }
                            $output.Flush($true)
                        } finally { $output.Dispose() }
                    } finally { $inputStream.Dispose() }
                }
            } finally { $archive.Dispose() }
        } finally { $stream.Dispose() }
        Test-BonsaiInferenceDirectory -Lock $lock -Directory $staging | Out-Null
        [IO.Directory]::Move($staging, $destination)
        $staging = $null
    } finally {
        if ($staging) { Write-Warning "Incomplete Prism Inference installation retained at $staging" }
    }
    return Test-BonsaiInferenceDirectory -Lock $lock -Directory $destination
}

Export-ModuleMember -Function Read-BonsaiInferenceLock, Test-BonsaiInferenceDirectory, Install-BonsaiInference
