#requires -Version 7.6
<#
.SYNOPSIS
Focused synthetic exact-symbol/scale PQ2-to-PTQ1 codec regressions.
.DESCRIPTION
Tests pinned native PTQ layout independently, including all 243 five-trit and
81 four-trit words in every lane. No model, requantization, or inference is run.
#>
[CmdletBinding()]
[OutputType([void])]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$repo = Split-Path $PSScriptRoot -Parent
if (-not ('SwiftBonsai.Pq2Ptq1Codec' -as [type])) { Add-Type -Path (Join-Path $repo 'scripts/Pq2Ptq1Codec.cs') -CompilerOptions @('/nullable:enable', '/checked+', '/unsafe-', '/warn:9999', '/warnaserror+', '/langversion:14.0') }
# PowerShell cannot pass ref-like spans directly. Bind a typed delegate to the
# already loaded codec so this tiny test adapter exercises the actual batch API.
if (-not ('SwiftBonsai.PtqBatchProbe' -as [type])) {
    Add-Type -CompilerOptions @('/nullable:enable', '/checked+', '/unsafe-', '/warn:9999', '/warnaserror+', '/langversion:14.0') -TypeDefinition @'
#nullable enable
using System;
using System.IO;
namespace SwiftBonsai {
/// <summary>Test-only adapter for span APIs that PowerShell cannot call directly.</summary>
public static class PtqBatchProbe {
    private delegate void Transform(ReadOnlySpan<byte> source, Span<byte> destination);
    /// <summary>Invokes a real codec batch method through a strongly typed span delegate.</summary>
    /// <param name="codec">The already loaded codec type.</param>
    /// <param name="source">Input block bytes.</param>
    /// <param name="destination">Caller-owned output block bytes.</param>
    /// <param name="decode">Whether to decode rather than encode.</param>
    public static void Run(Type codec, byte[] source, byte[] destination, bool decode) {
        ArgumentNullException.ThrowIfNull(codec);
        ArgumentNullException.ThrowIfNull(source);
        ArgumentNullException.ThrowIfNull(destination);
        var method=codec.GetMethod(decode ? "DecodeBlocks" : "EncodeBlocks") ?? throw new InvalidDataException("Missing real codec batch method.");
        method.CreateDelegate<Transform>()(source,destination);
    }
    /// <summary>Exercises actual overlapping span slices with otherwise valid block lengths.</summary>
    /// <param name="codec">The already loaded codec type.</param>
    /// <param name="decode">Whether to decode rather than encode.</param>
    public static void RunOverlap(Type codec, bool decode) {
        ArgumentNullException.ThrowIfNull(codec);
        var buffer=new byte[34];
        var method=codec.GetMethod(decode ? "DecodeBlocks" : "EncodeBlocks") ?? throw new InvalidDataException("Missing real codec batch method.");
        method.CreateDelegate<Transform>()(buffer.AsSpan(0,decode ? 28 : 34),buffer.AsSpan(0,decode ? 34 : 28));
    }
}
}
'@
}
function Assert {
    [CmdletBinding()]
    [OutputType([void])]
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw $Message }
}
function Assert-Block {
    [CmdletBinding()]
    [OutputType([void])]
    param([byte[]]$Actual, [byte[]]$Expected, [string]$Message)
    Assert ([Convert]::ToHexString($Actual) -ceq [Convert]::ToHexString($Expected)) $Message
}
function Assert-Failure {
    [CmdletBinding()]
    [OutputType([void])]
    param([scriptblock]$Action, [string]$Message)
    $failed = $false
    try { & $Action | Out-Null } catch { $failed = $true }
    Assert $failed $Message
}
$zeros = [byte[]]::new(34); [Array]::Fill[byte]($zeros, 0x55); $zeros[0] = 0; $zeros[1] = 0x3c
$negative = [byte[]]::new(34); $negative[1] = 0x3c
# Known native-format vectors establish scale position and ceil rather than
# direct base-3 storage: zero symbols -> qs=128, qh=127; +1 -> 255/253.
foreach ($code in 0..2) {
    $source = [byte[]]::new(34)
    [Array]::Fill[byte]($source, [byte](85 * $code)); $source[0] = 0; $source[1] = 0x3c
    $expected = [byte[]]::new(28); $expected[27] = 0x3c
    for ($i = 0; $i -lt 24; $i++) { $expected[$i] = [byte]@(0,128,255)[$code] }
    $expected[24] = $expected[25] = [byte]@(0,127,253)[$code]
    Assert-Block -Actual ([SwiftBonsai.Pq2Ptq1Codec]::EncodeBlock($source)) -Expected $expected -Message "Canonical dense code-$code vector differs."
    Assert-Block -Actual ([SwiftBonsai.Pq2Ptq1Codec]::DecodeBlock($expected)) -Expected $source -Message "Dense code-$code vector did not restore exact bytes."
}
foreach ($position in 0..127) {
    foreach ($code in 0..2) {
        $source = [byte[]]$zeros.Clone()
        $index = 2 + [int][Math]::Floor($position / 4); $shift = 2 * ($position % 4)
        $source[$index] = [byte](($source[$index] -band (255 -bxor (3 -shl $shift))) -bor ($code -shl $shift))
        $original = [byte[]]$source.Clone()
        $encoded = [SwiftBonsai.Pq2Ptq1Codec]::EncodeBlock($source)
        Assert ($encoded.Length -eq 28) 'PTQ output exceeded its exact block size.'
        $encodedOriginal = [byte[]]$encoded.Clone()
        Assert-Block -Actual ([SwiftBonsai.Pq2Ptq1Codec]::DecodeBlock($encoded)) -Expected $source -Message "Position $position code $code did not round-trip."
        Assert-Block -Actual $source -Expected $original -Message 'Encoder mutated its input.'
        Assert-Block -Actual $encoded -Expected $encodedOriginal -Message 'Decoder mutated its input.'
    }
    $bad = [byte[]]$zeros.Clone()
    $bad[2 + [int][Math]::Floor($position / 4)] = $bad[2 + [int][Math]::Floor($position / 4)] -bor [byte](3 -shl (2 * ($position % 4)))
    Assert-Failure { [SwiftBonsai.Pq2Ptq1Codec]::EncodeBlock($bad) } "Fourth symbol was accepted at position $position."
}
# Decode every possible canonical/noncanonical byte at every native lane.
# Expected positions and digits come from independent base-3 division, not
# from the implementation's multiply/wrap native-decoder formula.
for ($lane = 0; $lane -lt 26; $lane++) {
    $count = if ($lane -lt 24) { 5 } else { 4 }
    $words = if ($count -eq 5) { 243 } else { 81 }
    $stride = if ($lane -lt 16) { 16 } elseif ($lane -lt 24) { 8 } else { 2 }
    $first = if ($lane -lt 16) { $lane } elseif ($lane -lt 24) { 80 + $lane - 16 } else { 120 + $lane - 24 }
    $canonical = [Collections.Generic.HashSet[byte]]::new()
    for ($word = 0; $word -lt $words; $word++) {
        $base3 = if ($count -eq 4) { $word * 3 } else { $word }
        $stored = [byte][Math]::Ceiling($base3 * 256 / 243)
        Assert ($canonical.Add($stored)) 'Canonical scaled trit words collided.'
        $encoded = [byte[]]::new(28); $encoded[27] = 0x3c; $encoded[$lane] = $stored
        $expected = [byte[]]$negative.Clone()
        for ($trit = 0; $trit -lt $count; $trit++) {
            $digit = [int][Math]::Floor($word / [Math]::Pow(3, $count - 1 - $trit)) % 3
            $position = $first + $trit * $stride
            $index = 2 + [int][Math]::Floor($position / 4)
            $expected[$index] = $expected[$index] -bor [byte]($digit -shl (2 * ($position % 4)))
        }
        Assert-Block -Actual ([SwiftBonsai.Pq2Ptq1Codec]::DecodeBlock($encoded)) -Expected $expected -Message "Native stage/lane $lane word $word decoded differently."
        Assert-Block -Actual ([SwiftBonsai.Pq2Ptq1Codec]::EncodeBlock($expected)) -Expected $encoded -Message "Native stage/lane $lane word $word encoded differently."
    }
    foreach ($value in 0..255) {
        if (-not $canonical.Contains([byte]$value)) {
            $bad = [byte[]]::new(28); $bad[27] = 0x3c; $bad[$lane] = [byte]$value
            Assert-Failure { [SwiftBonsai.Pq2Ptq1Codec]::DecodeBlock($bad) } "Noncanonical lane $lane byte $value was accepted."
        }
    }
}
foreach ($scale in @(0x0001,0x03ff,0x0400,0x3555,0x3c00,0x7bff,0x8001,0x83ff,0x8400,0xbc00,0xfbff)) {
    $source = [byte[]]$zeros.Clone(); $source[2] = 0x24
    [Array]::Copy([BitConverter]::GetBytes([uint16]$scale), 0, $source, 0, 2)
    $encoded = [SwiftBonsai.Pq2Ptq1Codec]::EncodeBlock($source)
    Assert ($encoded[26] -eq $source[0] -and $encoded[27] -eq $source[1]) 'Raw finite FP16 scale changed.'
    Assert-Block -Actual ([SwiftBonsai.Pq2Ptq1Codec]::DecodeBlock($encoded)) -Expected $source -Message "Raw scale $scale did not round-trip."
}
foreach ($scale in @(0,0x8000,0x7c00,0xfc00,0x7c01,0xfc01,0x7e00,0xfe00,0x7fff,0xffff)) {
    $source = [byte[]]$zeros.Clone(); $encoded = [byte[]]::new(28)
    $bits = [BitConverter]::GetBytes([uint16]$scale)
    [Array]::Copy($bits, 0, $source, 0, 2); [Array]::Copy($bits, 0, $encoded, 26, 2)
    Assert-Failure { [SwiftBonsai.Pq2Ptq1Codec]::EncodeBlock($source) } "Ineligible scale $scale was encoded."
    Assert-Failure { [SwiftBonsai.Pq2Ptq1Codec]::DecodeBlock($encoded) } "Ineligible scale $scale was decoded."
}
foreach ($length in @(0,1,27,28,33,35,68,4096)) {
    Assert-Failure { [SwiftBonsai.Pq2Ptq1Codec]::EncodeBlock([byte[]]::new($length)) } "Wrong PQ2 length $length was accepted."
}
foreach ($length in @(0,1,26,27,29,34,56,4096)) {
    Assert-Failure { [SwiftBonsai.Pq2Ptq1Codec]::DecodeBlock([byte[]]::new($length)) } "Wrong PTQ length $length was accepted."
}
Assert-Failure { [SwiftBonsai.Pq2Ptq1Codec]::EncodeBlock($null) } 'Null PQ2 block was accepted.'
Assert-Failure { [SwiftBonsai.Pq2Ptq1Codec]::DecodeBlock($null) } 'Null PTQ block was accepted.'
# Multiple blocks share caller buffers. Nonzero destination bytes ensure decode
# overwrites every code instead of accidentally relying on freshly zeroed memory.
$batch = [byte[]]::new(34 * 3)
[Array]::Copy($zeros, 0, $batch, 0, 34); [Array]::Copy($negative, 0, $batch, 34, 34)
$signed = [byte[]]$zeros.Clone(); $signed[0] = 1; $signed[1] = 0x80; $signed[33] = 0x24
[Array]::Copy($signed, 0, $batch, 68, 34)
$packedBatch = [byte[]]::new(28 * 3); [Array]::Fill[byte]($packedBatch, 255)
[SwiftBonsai.PtqBatchProbe]::Run([SwiftBonsai.Pq2Ptq1Codec], $batch, $packedBatch, $false)
$restoredBatch = [byte[]]::new(34 * 3); [Array]::Fill[byte]($restoredBatch, 255)
[SwiftBonsai.PtqBatchProbe]::Run([SwiftBonsai.Pq2Ptq1Codec], $packedBatch, $restoredBatch, $true)
Assert-Block -Actual $restoredBatch -Expected $batch -Message 'Caller-owned multiple-block buffers did not round-trip.'
for ($block = 0; $block -lt 3; $block++) {
    $expected = [SwiftBonsai.Pq2Ptq1Codec]::EncodeBlock($batch[($block * 34)..($block * 34 + 33)])
    Assert-Block -Actual $packedBatch[($block * 28)..($block * 28 + 27)] -Expected $expected -Message 'Batch format differs from the single-block wrapper.'
}
[SwiftBonsai.PtqBatchProbe]::Run([SwiftBonsai.Pq2Ptq1Codec], [byte[]]::new(0), [byte[]]::new(0), $false)
[SwiftBonsai.PtqBatchProbe]::Run([SwiftBonsai.Pq2Ptq1Codec], [byte[]]::new(0), [byte[]]::new(0), $true)
Assert-Failure { [SwiftBonsai.PtqBatchProbe]::Run([SwiftBonsai.Pq2Ptq1Codec], [byte[]]::new(35), [byte[]]::new(28), $false) } 'Incomplete PQ2 batch was accepted.'
Assert-Failure { [SwiftBonsai.PtqBatchProbe]::Run([SwiftBonsai.Pq2Ptq1Codec], $batch, [byte[]]::new(85), $false) } 'Extra PTQ destination byte was accepted.'
Assert-Failure { [SwiftBonsai.PtqBatchProbe]::Run([SwiftBonsai.Pq2Ptq1Codec], [byte[]]::new(29), [byte[]]::new(34), $true) } 'Incomplete PTQ batch was accepted.'
Assert-Failure { [SwiftBonsai.PtqBatchProbe]::Run([SwiftBonsai.Pq2Ptq1Codec], $packedBatch, [byte[]]::new(101), $true) } 'Short PQ2 destination was accepted.'
Assert-Failure { [SwiftBonsai.PtqBatchProbe]::RunOverlap([SwiftBonsai.Pq2Ptq1Codec], $false) } 'Overlapping encode buffers were accepted.'
Assert-Failure { [SwiftBonsai.PtqBatchProbe]::RunOverlap([SwiftBonsai.Pq2Ptq1Codec], $true) } 'Overlapping decode buffers were accepted.'
Write-Information -InformationAction Continue 'PQ2/PTQ1 focused regressions passed: exact symbols/scales and pinned canonical byte layout, no runtime parity claim.'
