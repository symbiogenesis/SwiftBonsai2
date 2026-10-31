#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

function Read-BonsaiPtq1CaptureReceipt {
    <# .SYNOPSIS
    Validates the hosted Rust collector receipt against the reviewed checkout.
    #>
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([Parameter(Mandatory)][string]$BundleDirectory,
          [Parameter(Mandatory)][string]$RepositoryDirectory,
          [Parameter(Mandatory)][string]$Revision,
          [Parameter(Mandatory)][pscustomobject]$Pins)
    $receiptPath = Join-Path $BundleDirectory 'capture-tools.json'
    if ((Get-Item -LiteralPath $receiptPath).Length -gt 65536) { throw 'Capture tool receipt exceeds its pre-read byte bound.' }
    $receipt = Get-Content -LiteralPath $receiptPath -Raw | ConvertFrom-Json -DateKind String
    if ($receipt.SchemaVersion -ne 2 -or $receipt.RunnerRevision -cne $Revision -or
        $receipt.CollectorLanguage -cne 'rust' -or $receipt.CollectorSourcePath -cne 'tools/native-reference/swift_ptq1_reference.rs' -or
        $receipt.RustToolchain -cne 'nightly-2026-10-22-aarch64-pc-windows-msvc' -or
        $receipt.NativeSource.commit -cne $Pins.nativeSource.commit -or $receipt.NativeSource.url -cne $Pins.nativeSource.url -or
        $receipt.NativeSource.sha256 -cne $Pins.nativeSource.sha256 -or $receipt.NativeSource.bytes -ne $Pins.nativeSource.bytes -or
        $receipt.NativeSource.headerSha256 -cne $Pins.nativeSource.headerSha256 -or
        $receipt.RustSource.commit -cne $Pins.rustSource.commit -or $receipt.RustSource.url -cne $Pins.rustSource.url -or
        $receipt.RustSource.sha256 -cne $Pins.rustSource.sha256 -or $receipt.RustSource.bytes -ne $Pins.rustSource.bytes -or
        $receipt.Files.Count -ne 2 -or
        $receipt.CollectorSourceSha256 -cne (Get-FileHash -LiteralPath (Join-Path $RepositoryDirectory 'tools/native-reference/swift_ptq1_reference.rs') -Algorithm SHA256).Hash.ToLowerInvariant() -or
        $receipt.AbiControlSha256 -cne (Get-FileHash -LiteralPath (Join-Path $RepositoryDirectory 'tools/native-reference/fixtures/collector-abi-v1.txt') -Algorithm SHA256).Hash.ToLowerInvariant() -or
        $receipt.AbiProvenanceSha256 -cne (Get-FileHash -LiteralPath (Join-Path $RepositoryDirectory 'tools/native-reference/fixtures/collector-abi-v1.provenance.json') -Algorithm SHA256).Hash.ToLowerInvariant()) {
        throw 'Hosted capture tools differ from reviewed source/revision pins.'
    }
    return $receipt
}

function Read-Ptq1AbiObservationSet {
    <# .SYNOPSIS
    Reads a bounded, unique-key unsigned ABI observation set without coercions.
    #>
    [CmdletBinding()]
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseOutputTypeCorrectly', '', Justification = 'The unary comma prevents pipeline enumeration; the caller receives one typed dictionary, not the array wrapper inferred by the analyzer.')]
    [OutputType([Collections.Generic.Dictionary[string, uint64]])]
    param([Parameter(Mandatory)][string]$Path,
          [string]$ExpectedSha256)
    $stream = [IO.FileStream]::new($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    try {
        if ($stream.Length -gt 65536) { throw 'ABI observations exceed their fixed pre-read byte bound.' }
        $bytes = [byte[]]::new([int]$stream.Length)
        $stream.ReadExactly($bytes, 0, $bytes.Length)
    } finally { $stream.Dispose() }
    if ($ExpectedSha256 -and [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes)).ToLowerInvariant() -cne $ExpectedSha256) {
        throw 'ABI observation control bytes differ from the source-bound receipt.'
    }
    $text = [Text.UTF8Encoding]::new($false, $true).GetString($bytes)
    $records = [Collections.Generic.Dictionary[string, uint64]]::new([StringComparer]::Ordinal)
    $lines = $text -split '\r?\n'
    for ($index = 0; $index -lt $lines.Count; $index++) {
        $line = $lines[$index]
        if ($index -eq $lines.Count - 1 -and $line -ceq '') { continue }
        if ($line -cnotmatch '^(?<key>[a-zA-Z_][a-zA-Z_0-9.]*)=(?<value>0|[1-9][0-9]*)$') {
            throw 'Invalid ABI observation record.'
        }
        $key = $Matches.key
        [uint64]$value = 0
        if (-not [uint64]::TryParse($Matches.value, [Globalization.NumberStyles]::None, [Globalization.CultureInfo]::InvariantCulture, [ref]$value) -or
            -not $records.TryAdd($key, $value)) {
            throw 'Overflowing or duplicate ABI observation record.'
        }
    }
    if ($records.Count -eq 0) { throw 'Empty ABI observation set.' }
    return ,$records
}

function Assert-BonsaiPtq1AbiObservationSet {
    <# .SYNOPSIS
    Requires every live Rust/DLL ABI observation to match the frozen native control.
    #>
    [CmdletBinding()]
    [OutputType([void])]
    param([Parameter(Mandatory)][string]$ActualPath,
          [Parameter(Mandatory)][string]$ExpectedPath,
          [Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{64}$')][string]$ExpectedSha256)
    $actual = Read-Ptq1AbiObservationSet -Path $ActualPath
    $expected = Read-Ptq1AbiObservationSet -Path $ExpectedPath -ExpectedSha256 $ExpectedSha256
    if ($actual.Count -ne $expected.Count) { throw 'Native/Rust ABI observation inventory differs.' }
    foreach ($entry in $expected.GetEnumerator()) {
        [uint64]$value = 0
        if (-not $actual.TryGetValue($entry.Key, [ref]$value) -or $value -ne $entry.Value) {
            throw "Native/Rust ABI observation differs: $($entry.Key)."
        }
    }
}

Export-ModuleMember -Function Read-BonsaiPtq1CaptureReceipt, Assert-BonsaiPtq1AbiObservationSet
