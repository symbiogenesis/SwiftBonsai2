#requires -Version 7.6
<# .SYNOPSIS
Checks capture preparation and the Rust collector's source/ABI admission boundary.
#>
[CmdletBinding()]
[OutputType([string])]
param([Parameter(Mandatory)][ValidateSet('HostedGuard', 'CollectorControls')][string]$Case)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
if ($Case -ceq 'CollectorControls') {
    Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.Ptq1Capture.psm1')
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    $working = [IO.Path]::GetFullPath((Join-Path $tempRoot ('Bonsai-Ptq1CollectorControls-' + [guid]::NewGuid().ToString('N'))))
    $fixtureDirectory = Join-Path $working 'tools/native-reference/fixtures'
    [void][IO.Directory]::CreateDirectory($fixtureDirectory)
    try {
        $source = Join-Path $working 'tools/native-reference/swift_ptq1_reference.rs'
        $expected = Join-Path $fixtureDirectory 'collector-abi-v1.txt'
        $provenance = Join-Path $fixtureDirectory 'collector-abi-v1.provenance.json'
        $actual = Join-Path $working 'actual.txt'
        [IO.File]::WriteAllText($source, 'reviewed Rust collector source')
        [IO.File]::WriteAllText($expected, "model.size=80`nbatch.size=56`nsigned=18446744073709551615`n")
        [IO.File]::WriteAllText($provenance, '{"schema_version":1}')
        [IO.File]::WriteAllText($actual, "signed=18446744073709551615`nbatch.size=56`nmodel.size=80`n")
        $pins = Get-Content -LiteralPath (Join-Path $PSScriptRoot '../tools/native-reference/pins.json') -Raw | ConvertFrom-Json -DateKind String
        $receipt = [ordered]@{
            SchemaVersion = 2; RunnerRevision = ('a' * 40); CollectorLanguage = 'rust'
            CollectorSourcePath = 'tools/native-reference/swift_ptq1_reference.rs'
            RustToolchain = 'nightly-2026-10-22-aarch64-pc-windows-msvc'
            NativeSource = $pins.nativeSource; RustSource = $pins.rustSource
            CollectorSourceSha256 = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()
            AbiControlSha256 = (Get-FileHash -LiteralPath $expected -Algorithm SHA256).Hash.ToLowerInvariant()
            AbiProvenanceSha256 = (Get-FileHash -LiteralPath $provenance -Algorithm SHA256).Hash.ToLowerInvariant()
            Files = @(@{ Name = 'swift-ptq1-reference.exe' }, @{ Name = 'swift_model_check.exe' })
        }
        $receiptPath = Join-Path $working 'capture-tools.json'
        $reviewedJson = $receipt | ConvertTo-Json -Depth 12
        [IO.File]::WriteAllText($receiptPath, $reviewedJson)
        $accepted = Read-BonsaiPtq1CaptureReceipt -BundleDirectory $working -RepositoryDirectory $working -Revision ('a' * 40) -Pins $pins
        Assert ($accepted.CollectorLanguage -ceq 'rust') 'The reviewed Rust collector receipt was rejected.'
        Assert-BonsaiPtq1AbiObservations -ActualPath $actual -ExpectedPath $expected -ExpectedSha256 $receipt.AbiControlSha256
        foreach ($mutation in @(
            @{ SchemaVersion = 1 }, @{ CollectorLanguage = 'cpp' }, @{ RunnerRevision = ('b' * 40) },
            @{ CollectorSourceSha256 = ('0' * 64) }, @{ AbiControlSha256 = ('0' * 64) }, @{ AbiProvenanceSha256 = ('0' * 64) }
        )) {
            $changed = $reviewedJson | ConvertFrom-Json -DateKind String
            foreach ($key in $mutation.Keys) { $changed.$key = $mutation[$key] }
            [IO.File]::WriteAllText($receiptPath, ($changed | ConvertTo-Json -Depth 12))
            $failure = Assert-Failure -PassThru {
                Read-BonsaiPtq1CaptureReceipt -BundleDirectory $working -RepositoryDirectory $working -Revision ('a' * 40) -Pins $pins
            }
            Assert ($failure -like '*reviewed source/revision pins*') 'An unreviewed collector receipt was accepted.'
        }
        foreach ($text in @(
            "model.size=80`nbatch.size=56`n", # omission
            "model.size=80`nbatch.size=56`nsigned=18446744073709551615`nextra=1`n", # extra
            "model.size=81`nbatch.size=56`nsigned=18446744073709551615`n", # changed value
            "model.size=80`nbatch.size=56`nsigned=18446744073709551615`nmodel.size=80`n", # duplicate
            "model.size=80`nbatch.size=56`nsigned=18446744073709551616`n", # overflow
            "model.size=80`nbatch.size=56`nsigned=18446744073709551615`ntail", # malformed tail
            ('x' * 65537)
        )) {
            [IO.File]::WriteAllText($actual, $text)
            Assert-Failure { Assert-BonsaiPtq1AbiObservations -ActualPath $actual -ExpectedPath $expected -ExpectedSha256 $receipt.AbiControlSha256 }
        }
        [IO.File]::WriteAllText($actual, "model.size=81`nbatch.size=56`nsigned=18446744073709551615`n")
        [IO.File]::Copy($actual, $expected, $true)
        $failure = Assert-Failure -PassThru { Assert-BonsaiPtq1AbiObservations -ActualPath $actual -ExpectedPath $expected -ExpectedSha256 $receipt.AbiControlSha256 }
        Assert ($failure -like '*control bytes differ*') 'Changed control bytes were accepted after receipt validation.'
    } finally {
        $prefix = $tempRoot.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
        if (-not $working.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase) -or $working -ceq $tempRoot) {
            throw 'Refusing cleanup outside the generated temporary test directory.'
        }
        Remove-Item -LiteralPath $working -Recurse -Force
    }
    return "Focused $Case capture preparation checks passed."
}
$script = Join-Path $PSScriptRoot '../scripts/Build-NativePtq1CaptureTools.ps1'
$output = Join-Path ([IO.Path]::GetTempPath()) ('Bonsai-HostedCaptureGuard-' + [guid]::NewGuid().ToString('N'))
$savedActions = $env:GITHUB_ACTIONS
$savedEnvironment = $env:RUNNER_ENVIRONMENT
try {
    foreach ($environment in @('local', 'self-hosted')) {
        $env:GITHUB_ACTIONS = if ($environment -ceq 'local') { 'false' } else { 'true' }
        $env:RUNNER_ENVIRONMENT = if ($environment -ceq 'local') { 'github-hosted' } else { 'self-hosted' }
        $failure = Assert-Failure -PassThru { & $script -OutputDirectory $output }
        Assert ($failure -like '*hosted native Windows ARM64 CI*') 'Capture build did not reject the non-hosted environment.'
        Assert (-not (Test-Path -LiteralPath $output)) 'Rejected capture preparation created an output directory.'
    }
} finally {
    [Environment]::SetEnvironmentVariable('GITHUB_ACTIONS', $savedActions)
    [Environment]::SetEnvironmentVariable('RUNNER_ENVIRONMENT', $savedEnvironment)
}
"Focused $Case capture preparation checks passed."
