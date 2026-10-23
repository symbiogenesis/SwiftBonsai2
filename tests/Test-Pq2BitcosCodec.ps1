#requires -Version 7.6
<#
.SYNOPSIS
Focused exact-byte regressions for the experimental single-block PQ2/BITCOS codec.
.DESCRIPTION
Only synthetic blocks are used. No model conversion, runtime, benchmark, or
performance/RAM claim is involved. This proves a bounded reference layout only.
#>
[CmdletBinding()]
[OutputType([void])]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$repo = Split-Path $PSScriptRoot -Parent
if (-not ('SwiftBonsai.Pq2BitcosCodec' -as [type])) { Add-Type -Path (Join-Path $repo 'scripts/Pq2BitcosCodec.cs') -CompilerOptions @('/nullable:enable', '/checked+', '/unsafe-', '/warn:9999', '/warnaserror+', '/langversion:14.0') }
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
# Independent canonical all-zero vector: raw FP16(1), 128 original code-1s,
# no present positions and no sign bytes.
$zeros = [byte[]]::new(34)
[Array]::Fill[byte]($zeros, 0x55)
$zeros[0] = 0; $zeros[1] = 0x3c
$zeroEncoded = [byte[]]::new(18); $zeroEncoded[1] = 0x3c
Assert-Block -Actual ([SwiftBonsai.Pq2BitcosCodec]::EncodeBlock($zeros)) -Expected $zeroEncoded -Message 'All-zero block has a noncanonical encoding.'
Assert-Block -Actual ([SwiftBonsai.Pq2BitcosCodec]::DecodeBlock($zeroEncoded)) -Expected $zeros -Message 'All-zero block did not restore exact original codes.'
# Every ternary code in every position proves the two independent bit orders.
foreach ($position in 0..127) {
    foreach ($code in 0..2) {
        $source = [byte[]]$zeros.Clone()
        $index = 2 + [int][Math]::Floor($position / 4)
        $shift = 2 * ($position % 4)
        $source[$index] = [byte](($source[$index] -band (255 -bxor (3 -shl $shift))) -bor ($code -shl $shift))
        $original = [byte[]]$source.Clone()
        $encoded = [SwiftBonsai.Pq2BitcosCodec]::EncodeBlock($source)
        $expected = [byte[]]::new($(if ($code -eq 1) { 18 } else { 19 }))
        $expected[1] = 0x3c
        if ($code -ne 1) {
            $expected[2 + [int][Math]::Floor($position / 8)] = [byte](1 -shl ($position % 8))
            $expected[18] = [byte]$(if ($code -eq 2) { 1 } else { 0 })
        }
        Assert-Block -Actual $encoded -Expected $expected -Message "Bit position/sign meaning changed at position $position, code $code."
        Assert-Block -Actual ([SwiftBonsai.Pq2BitcosCodec]::DecodeBlock($encoded)) -Expected $source -Message "Exact round-trip failed at position $position, code $code."
        Assert-Block -Actual $source -Expected $original -Message 'Encoder mutated its caller-owned source.'
        Assert ($encoded.Length -ge 18 -and $encoded.Length -le 34) 'Reference block exceeded its fixed bounds.'
    }
}
# Every nonzero count crosses all sign-byte boundaries. Interleaved logical
# positions and alternating signs exercise sign packing independently of PQ2 order.
foreach ($nonzero in 0..128) {
    $source = [byte[]]$zeros.Clone()
    $expected = [byte[]]::new(18 + [int][Math]::Ceiling($nonzero / 8))
    $expected[1] = 0x3c
    for ($sign = 0; $sign -lt $nonzero; $sign++) {
        $position = ($sign * 37) % 128 # permutation: 37 is coprime to 128
        $code = if ($position % 3) { 2 } else { 0 }
        $index = 2 + [int][Math]::Floor($position / 4)
        $shift = 2 * ($position % 4)
        $source[$index] = [byte](($source[$index] -band (255 -bxor (3 -shl $shift))) -bor ($code -shl $shift))
        $expected[2 + [int][Math]::Floor($position / 8)] = $expected[2 + [int][Math]::Floor($position / 8)] -bor [byte](1 -shl ($position % 8))
    }
    $signIndex = 0
    for ($position = 0; $position -lt 128; $position++) {
        if ($expected[2 + [int][Math]::Floor($position / 8)] -band (1 -shl ($position % 8))) {
            if ($position % 3) {
                $expected[18 + [int][Math]::Floor($signIndex / 8)] = $expected[18 + [int][Math]::Floor($signIndex / 8)] -bor [byte](1 -shl ($signIndex % 8))
            }
            $signIndex++
        }
    }
    $encoded = [SwiftBonsai.Pq2BitcosCodec]::EncodeBlock($source)
    Assert-Block -Actual $encoded -Expected $expected -Message "Canonical bitmap/sign encoding differs at $nonzero nonzeros."
    $encodedOriginal = [byte[]]$encoded.Clone()
    Assert-Block -Actual ([SwiftBonsai.Pq2BitcosCodec]::DecodeBlock($encoded)) -Expected $source -Message "Exact round-trip failed at $nonzero nonzeros."
    Assert-Block -Actual $encoded -Expected $encodedOriginal -Message 'Decoder mutated its caller-owned source.'
    Assert-Block -Actual ([SwiftBonsai.Pq2BitcosCodec]::EncodeBlock([SwiftBonsai.Pq2BitcosCodec]::DecodeBlock($encoded))) -Expected $encoded -Message 'Canonical reference encoding was not stable.'
    if ($nonzero % 8) {
        foreach ($unused in ($nonzero % 8)..7) {
            $bad = [byte[]]$encoded.Clone()
            $bad[-1] = [byte]($bad[-1] -bor (1 -shl $unused))
            Assert-Failure { [SwiftBonsai.Pq2BitcosCodec]::DecodeBlock($bad) } "Unused sign bit $unused was accepted at $nonzero nonzeros."
        }
    }
}
# Dense extremes are fully canonical, with no more than 34 output bytes.
foreach ($code in @(0,2)) {
    $source = [byte[]]::new(34)
    [Array]::Fill[byte]($source, [byte]$(if ($code -eq 0) { 0 } else { 0xaa }))
    $source[1] = 0x3c; $source[0] = 0
    $expected = [byte[]]::new(34)
    $expected[1] = 0x3c
    for ($i = 2; $i -lt 18; $i++) { $expected[$i] = 255 }
    if ($code -eq 2) { for ($i = 18; $i -lt 34; $i++) { $expected[$i] = 255 } }
    Assert-Block -Actual ([SwiftBonsai.Pq2BitcosCodec]::EncodeBlock($source)) -Expected $expected -Message "Dense code-$code block differs."
    Assert-Block -Actual ([SwiftBonsai.Pq2BitcosCodec]::DecodeBlock($expected)) -Expected $source -Message "Dense code-$code block did not round-trip."
}
# Raw positive/negative normal/subnormal/extreme finite scale bits survive exact
# byte round-trips; scale decoding or requantization would weaken this property.
foreach ($scale in @(0x0001,0x03ff,0x0400,0x3555,0x3c00,0x7bff,0x8001,0x83ff,0x8400,0xbc00,0xfbff)) {
    $source = [byte[]]$zeros.Clone()
    $source[2] = 0x24 # -1,0,+1,-1
    [Array]::Copy([BitConverter]::GetBytes([uint16]$scale), 0, $source, 0, 2)
    $encoded = [SwiftBonsai.Pq2BitcosCodec]::EncodeBlock($source)
    Assert ($encoded[0] -eq $source[0] -and $encoded[1] -eq $source[1]) 'Encoder changed raw finite FP16 bits.'
    Assert-Block -Actual ([SwiftBonsai.Pq2BitcosCodec]::DecodeBlock($encoded)) -Expected $source -Message "Scale $scale did not round-trip exactly."
}
# Every possible packed byte is classified independently: 3^4=81 ternary
# patterns accepted; all 175 patterns containing a fourth symbol rejected.
$accepted = 0; $rejected = 0
foreach ($packed in 0..255) {
    $source = [byte[]]$zeros.Clone(); $source[2] = [byte]$packed
    $eligible = $true
    foreach ($shift in @(0,2,4,6)) { if ((($packed -shr $shift) -band 3) -eq 3) { $eligible = $false } }
    if ($eligible) {
        Assert-Block -Actual ([SwiftBonsai.Pq2BitcosCodec]::DecodeBlock([SwiftBonsai.Pq2BitcosCodec]::EncodeBlock($source))) -Expected $source -Message "Packed ternary byte $packed failed round-trip."
        $accepted++
    } else {
        Assert-Failure { [SwiftBonsai.Pq2BitcosCodec]::EncodeBlock($source) } "Fourth-symbol byte $packed was accepted."
        $rejected++
    }
}
Assert ($accepted -eq 81 -and $rejected -eq 175) 'Packed-byte classification coverage changed.'
foreach ($position in 0..127) {
    $source = [byte[]]$zeros.Clone()
    $index = 2 + [int][Math]::Floor($position / 4)
    $source[$index] = [byte]($source[$index] -bor (3 -shl (2 * ($position % 4))))
    Assert-Failure { [SwiftBonsai.Pq2BitcosCodec]::EncodeBlock($source) } "Fourth symbol was accepted at position $position."
}
# Zero scales are excluded conservatively even when all symbols are zero.
# Nonfinite (both signs; infinities and signaling/quiet NaNs) are also excluded.
foreach ($scale in @(0,0x8000,0x7c00,0xfc00,0x7c01,0xfc01,0x7e00,0xfe00,0x7fff,0xffff)) {
    $source = [byte[]]$zeros.Clone()
    $encoded = [byte[]]$zeroEncoded.Clone()
    $bits = [BitConverter]::GetBytes([uint16]$scale)
    [Array]::Copy($bits, 0, $source, 0, 2); [Array]::Copy($bits, 0, $encoded, 0, 2)
    Assert-Failure { [SwiftBonsai.Pq2BitcosCodec]::EncodeBlock($source) } "Ineligible scale $scale was encoded."
    Assert-Failure { [SwiftBonsai.Pq2BitcosCodec]::DecodeBlock($encoded) } "Ineligible scale $scale was decoded."
}
foreach ($length in @(0,1,17,18,19,33,35,68,4096)) {
    Assert-Failure { [SwiftBonsai.Pq2BitcosCodec]::EncodeBlock([byte[]]::new($length)) } "Wrong PQ2 input length $length was accepted."
}
foreach ($length in @(0,1,17,35,68,4096)) {
    Assert-Failure { [SwiftBonsai.Pq2BitcosCodec]::DecodeBlock([byte[]]::new($length)) } "Out-of-range reference length $length was accepted."
}
Assert-Failure { [SwiftBonsai.Pq2BitcosCodec]::EncodeBlock($null) } 'Null PQ2 input was accepted.'
Assert-Failure { [SwiftBonsai.Pq2BitcosCodec]::DecodeBlock($null) } 'Null reference input was accepted.'
# Valid prefix and scale with too few/many signs must fail exact-length checks.
$needsSigns = [byte[]]$zeroEncoded.Clone(); $needsSigns[2] = 1
Assert-Failure { [SwiftBonsai.Pq2BitcosCodec]::DecodeBlock($needsSigns) } 'Missing sign byte was accepted.'
$extraSign = [byte[]]::new(19); $extraSign[1] = 0x3c
Assert-Failure { [SwiftBonsai.Pq2BitcosCodec]::DecodeBlock($extraSign) } 'Extra sign byte for a zero bitmap was accepted.'
$nineWithoutSecond = [byte[]]::new(19); $nineWithoutSecond[1] = 0x3c; $nineWithoutSecond[2] = 255; $nineWithoutSecond[3] = 1
Assert-Failure { [SwiftBonsai.Pq2BitcosCodec]::DecodeBlock($nineWithoutSecond) } 'Truncated second sign byte was accepted.'
$extra = [byte[]]::new(20); $extra[1] = 0x3c; $extra[2] = 1
Assert-Failure { [SwiftBonsai.Pq2BitcosCodec]::DecodeBlock($extra) } 'Trailing reference block bytes were accepted.'
Write-Information -InformationAction Continue 'PQ2/BITCOS experimental reference-block regressions passed (synthetic bytes only).'
