#requires -Version 7.6
<# .SYNOPSIS
Checks pinned external source extraction, cache reuse and unsafe archive rejection.
#>
[CmdletBinding()]
[OutputType([void])]
param([ValidateSet('All', 'Valid', 'Traversal')][string]$Case = 'All')
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if ($Case -eq 'All' -and $env:GITHUB_ACTIONS -ne 'true') { throw 'The complete suite runs only in CI. Select one focused case locally.' }
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
$module = Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.InferenceSource.psm1') -Force -PassThru
$lane = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-InferenceSourceTest-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($lane)
$commit = '1' * 40
$rootName = 'prism-inference-' + $commit + '/'
$binaryFiles = @('prism-inference.exe', 'bonsai-token-worker.exe', 'bonsai-swift-worker.exe', 'prism-inference-server.exe')
$selections = if ($Case -eq 'All') { @('Valid', 'Traversal') } else { @($Case) }
try {
    foreach ($selection in $selections) {
        $fixtureArchive = Join-Path $lane ($selection + '.zip')
        $stream = [IO.FileStream]::new($fixtureArchive, [IO.FileMode]::CreateNew, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
        try {
            $zip = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Create, $true)
            try {
                $entries = @(($rootName + 'Cargo.toml'), ($rootName + 'tests/upstream_prism/harness.rs'))
                if ($selection -eq 'Traversal') { $entries += $rootName + '../escape.rs' }
                foreach ($name in $entries) {
                    $writer = [IO.StreamWriter]::new($zip.CreateEntry($name).Open())
                    try { $writer.Write('source fixture') } finally { $writer.Dispose() }
                }
            } finally { $zip.Dispose() }
        } finally { $stream.Dispose() }
        $source = @{
            Repository = 'https://github.com/symbiogenesis/prism-inference'; Commit = $commit
            Url = "https://github.com/symbiogenesis/prism-inference/archive/$commit.zip"
            Bytes = ([IO.FileInfo]$fixtureArchive).Length
            Sha256 = (Get-FileHash -LiteralPath $fixtureArchive -Algorithm SHA256).Hash.ToLowerInvariant()
        }
        $lock = @{
            schemaVersion = 1; package = 'prism-inference'; version = '0.1.0'
            repository = $source.Repository; commit = $commit; target = 'aarch64-pc-windows-msvc'; release = 'v0.1.0'
            asset = @{
                filename = 'prism-inference-0.1.0-aarch64-pc-windows-msvc.zip'; bytes = 1; sha256 = ('0' * 64)
                url = 'https://github.com/symbiogenesis/prism-inference/releases/download/v0.1.0/prism-inference-0.1.0-aarch64-pc-windows-msvc.zip'
            }
            files = @($binaryFiles | ForEach-Object { @{ name = $_; bytes = 1; sha256 = ('0' * 64) } })
            Source = $source
        }
        $lockPath = Join-Path $lane ($selection + '-lock.json')
        [IO.File]::WriteAllText($lockPath, (ConvertTo-Json -InputObject $lock -Depth 8))
        # Replace transport only: the actual resolver still verifies archive bytes
        # against the synthetic lock before validating and extracting every entry.
        & $module {
            param([string]$Archive, [string]$Url)
            $script:FixtureArchive = $Archive
            $script:FixtureUrl = $Url
            $script:DownloadCount = 0
            $script:DownloadDirectory = $null
            function script:Receive-LockedArtifact {
                [CmdletBinding()]
                [OutputType([string])]
                param([Uri]$Uri, [string]$Path, [long]$Bytes, [string]$Sha256)
                if ($Uri.AbsoluteUri -cne $script:FixtureUrl) { throw 'Unexpected source download URL.' }
                $script:DownloadCount++
                $script:DownloadDirectory = [IO.Path]::GetDirectoryName($Path)
                [IO.File]::Copy($script:FixtureArchive, $Path, $false)
                Test-LockedArtifact -Path $Path -Bytes $Bytes -Sha256 $Sha256
                return $Path
            }
        } $fixtureArchive $source.Url
        if ($selection -eq 'Valid') {
            $resolved = Resolve-BonsaiInferenceSource -LockPath $lockPath
            Assert ([IO.File]::ReadAllText((Join-Path $resolved 'Cargo.toml')) -ceq 'source fixture') 'Source manifest bytes changed during extraction.'
            Assert ([IO.File]::ReadAllText((Join-Path $resolved 'tests/upstream_prism/harness.rs')) -ceq 'source fixture') 'Conformance harness bytes changed during extraction.'
            Assert ((Resolve-BonsaiInferenceSource -LockPath $lockPath) -ceq $resolved) 'The pinned source extraction was not reused.'
            $downloadCount = & $module { $script:DownloadCount }
            Assert ($downloadCount -eq 1) 'Reusing pinned source repeated its download.'
        } else {
            Assert-Failure { Resolve-BonsaiInferenceSource -LockPath $lockPath } 'Source archive traversal was accepted.'
            $downloadDirectory = & $module { $script:DownloadDirectory }
            Assert (-not [IO.File]::Exists((Join-Path $downloadDirectory 'escape.rs'))) 'Source archive traversal wrote outside its extraction root.'
        }
    }
} finally {
    # Discard the test-only transport replacement and source cache before any
    # later operation in this PowerShell session imports the production resolver.
    Remove-Module -ModuleInfo $module -Force
}
