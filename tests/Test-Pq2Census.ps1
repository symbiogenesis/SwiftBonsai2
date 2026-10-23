#requires -Version 7.6
<#
.SYNOPSIS
Focused synthetic regressions for pinned Prism PQ2 payload census and ternary eligibility.
#>
[CmdletBinding()]
[OutputType([void])]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$repo = Split-Path $PSScriptRoot -Parent
Import-Module (Join-Path $repo 'scripts/Bonsai.Common.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'Gguf.TestSupport.psm1') -Force
$dialect = 'Prism-73bfe6cc485554822f80c473a3c0c4f25e80f41b'
$scratch = Resolve-ExternalPath (Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Pq2Tests-' + [Guid]::NewGuid().ToString('N')))
[void][IO.Directory]::CreateDirectory($scratch)
function Assert {
    [CmdletBinding()]
    [OutputType([void])]
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw $Message }
}
function Assert-Failure {
    [CmdletBinding()]
    [OutputType([void])]
    param([scriptblock]$Action, [string]$Message)
    $failed = $false
    try { & $Action | Out-Null } catch { $failed = $true }
    Assert $failed $Message
}
function Write-Text {
    [CmdletBinding()]
    [OutputType([void])]
    param([IO.BinaryWriter]$Writer, [string]$Text)
    $bytes = [Text.Encoding]::UTF8.GetBytes($Text)
    $Writer.Write([uint64]$bytes.Length); $Writer.Write($bytes)
}
function New-CensusFixture {
    [CmdletBinding(SupportsShouldProcess)]
    [OutputType([hashtable])]
    param([string]$Path, [hashtable[]]$Tensors)
    if (-not $PSCmdlet.ShouldProcess($Path, 'Create synthetic GGUF census fixture')) { return }
    $memory = [IO.MemoryStream]::new()
    $writer = [IO.BinaryWriter]::new($memory, [Text.Encoding]::UTF8, $true)
    $positions = [Collections.Generic.List[hashtable]]::new()
    try {
        $writer.Write([uint32]0x46554747); $writer.Write([uint32]3)
        $writer.Write([uint64]$Tensors.Count); $writer.Write([uint64]1)
        Write-Text $writer 'general.architecture'; $writer.Write([uint32]8); Write-Text $writer 'synthetic'
        $offset = 0L
        foreach ($tensor in $Tensors) {
            Write-Text $writer $tensor.Name; $writer.Write([uint32]1)
            $dimensionPosition = $memory.Position; $writer.Write([uint64]$tensor.Elements)
            $typePosition = $memory.Position; $writer.Write([uint32]$tensor.Type)
            $offsetPosition = $memory.Position; $writer.Write([uint64]$offset)
            $positions.Add(@{ Dimension = $dimensionPosition; Type = $typePosition; Offset = $offsetPosition; Data = $offset })
            $offset = [long]([Math]::Ceiling(($offset + $tensor.Payload.Length) / 32.0) * 32)
        }
        while ($memory.Position % 32) { $writer.Write([byte]0) }
        $dataOffset = $memory.Position
        # Nonzero padding detects any accidental scanning of tensor gaps.
        $padding = [byte[]]::new($offset)
        [Array]::Fill[byte]($padding, 255)
        $writer.Write($padding)
        for ($index = 0; $index -lt $Tensors.Count; $index++) {
            $memory.Position = $dataOffset + $positions[$index].Data
            $writer.Write([byte[]]$Tensors[$index].Payload)
        }
        $writer.Flush()
        $bytes = $memory.ToArray()
        [IO.File]::WriteAllBytes($Path, $bytes)
        return @{ Bytes = $bytes; DataOffset = $dataOffset; Positions = $positions }
    } finally { $writer.Dispose(); $memory.Dispose() }
}
# Independent fixtures use the pinned serialized codec: FP16 then low-first
# 2-bit codes. Code 1 is zero; 0/2 are -1/+1; code 3 is the fourth (+2) symbol.
$small = [byte[]]::new(68)
[Array]::Fill[byte]($small, 0x55)
$small[0] = $small[34] = 0; $small[1] = 0x3c; $small[35] = 0xbc
$small[2] = 0x58 # codes 0,2,1,1: -1,+1,0,0
$small[67] = 0x95 # codes 1,1,1,2: 0,0,0,+1
$largeBlocks = 1928 # exceeds the inspector's 1927-block batch by one block
$large = [byte[]]::new(34 * $largeBlocks)
[Array]::Fill[byte]($large, 0x64) # codes 0,1,2,1: -1,0,+1,0
for ($i = 0; $i -lt $largeBlocks; $i++) { $large[34 * $i] = 0; $large[34 * $i + 1] = 0x3c }
$path = Join-Path $scratch 'mixed-ternary.gguf'
$fixture = New-CensusFixture $path @(
    @{ Name = 'fixture.small'; Type = 142; Elements = 256; Payload = $small }
    @{ Name = 'fixture.large'; Type = 142; Elements = 128 * $largeBlocks; Payload = $large }
    @{ Name = 'fixture.float'; Type = 0; Elements = 1; Payload = [byte[]]@(255,255,255,255) }
    @{ Name = 'fixture.ptq'; Type = 143; Elements = 128; Payload = [byte[]]::new(28) }
)
Assert-Failure { (Read-TestGguf $path) } 'Stock dialect accepted private Prism IDs.'
Assert-Failure { (Read-TestGguf $path 'Prism-unpinned' $true) } 'Unpinned Prism dialect was accepted.'
$metadata = (Read-TestGguf $path $dialect $false)
Assert (-not $metadata.TensorPayloadRead -and -not $metadata.ContainsKey('Pq2Census')) 'Metadata-only inspection read payloads.'
Assert ($metadata.TensorTypeHistogram[1].Name -ceq 'PQ2_0' -and $metadata.TensorTypeHistogram[2].Name -ceq 'PTQ1_0') 'Pinned private type table differs.'
$report = (Read-TestGguf $path $dialect $true)
$census = $report.Pq2Census
$first = $census.Tensors[0]
$second = $census.Tensors[1]
Assert ($report.TensorPayloadRead -and $census.TensorCount -eq 2) 'Actual payload census was not reported.'
Assert ($first.CodeMinusOne -eq 1 -and $first.CodeZero -eq 253 -and $first.CodePlusOne -eq 2 -and $first.CodePlusTwo -eq 0) 'Low-first two-bit code census differs.'
Assert ($first.LosslessThreeSymbolEligible -and $first.ZeroScaleBlocks -eq 0 -and $first.NonfiniteScaleBlocks -eq 0) 'Finite negative scale was incorrectly rejected.'
Assert ($second.CodeMinusOne -eq 32 * $largeBlocks -and $second.CodeZero -eq 64 * $largeBlocks -and $second.CodePlusOne -eq 32 * $largeBlocks) 'Census lost symbols across the read boundary.'
Assert ($census.SerializedBytesRead -eq $small.Length + $large.Length -and $census.UnchangedOtherTensorBytes -eq 32) 'Census scanned tensor padding or unrelated payloads.'
Assert ($census.PayloadBufferBytes -eq 65536 -and $census.MaximumBlocksPerRead -eq 1927) 'Bounded payload buffer contract changed.'
Assert ($census.Blocks -eq 2 + $largeBlocks -and $census.Scales -eq $census.Blocks) 'Block/scale counts differ.'
Assert ($census.CodeMinusOne + $census.CodeZero + $census.CodePlusOne + $census.CodePlusTwo -eq 128 * $census.Blocks) 'Census symbols do not sum to validated tensor elements.'
# Exercise every possible packed byte against an independent scalar-code count.
$allCodes = [byte[]]::new(34 * 8)
$expected = [long[]]::new(4)
for ($block = 0; $block -lt 8; $block++) {
    $allCodes[$block * 34 + 1] = 0x3c
    for ($byte = 0; $byte -lt 32; $byte++) {
        $value = $block * 32 + $byte
        $allCodes[$block * 34 + 2 + $byte] = [byte]$value
        for ($shift = 0; $shift -lt 8; $shift += 2) { $expected[($value -shr $shift) -band 3]++ }
    }
}
$allPath = Join-Path $scratch 'all-packed-bytes.gguf'
New-CensusFixture $allPath @(@{ Name = 'all.codes'; Type = 142; Elements = 1024; Payload = $allCodes }) | Out-Null
$all = (Read-TestGguf $allPath $dialect $true).Pq2Census
Assert ($all.CodeMinusOne -eq $expected[0] -and $all.CodeZero -eq $expected[1] -and $all.CodePlusOne -eq $expected[2] -and $all.CodePlusTwo -eq $expected[3]) 'Lookup census differs for a packed byte pattern.'
Assert (-not $all.LosslessThreeSymbolEligible) 'Fourth symbols received three-symbol eligibility.'
# Invalid scales remain raw-code zeros, never silently recoded decoded zeros.
foreach ($scale in @(0,0x8000,0x7c00,0xfc00,0x7e00)) {
    $bytes = [byte[]]$fixture.Bytes.Clone()
    [Array]::Copy([BitConverter]::GetBytes([uint16]$scale), 0, $bytes, $fixture.DataOffset, 2)
    $scalePath = Join-Path $scratch "scale-$scale.gguf"
    [IO.File]::WriteAllBytes($scalePath, $bytes)
    $invalid = (Read-TestGguf $scalePath $dialect $true).Pq2Census.Tensors[0]
    Assert ($invalid.StrictTernaryCodes -and -not $invalid.LosslessThreeSymbolEligible -and $invalid.CodeZero -eq 253) 'Invalid-scale handling changed raw symbols or eligibility.'
    $isZero = $scale -in @(0,0x8000)
    Assert (($invalid.ZeroScaleBlocks -eq [int]$isZero) -and ($invalid.NonfiniteScaleBlocks -eq [int](-not $isZero))) 'Zero/nonfinite scale classification differs.'
}
foreach ($mutation in @('truncation','overlap','row-size','dimension-overflow')) {
    $bytes = [byte[]]$fixture.Bytes.Clone()
    switch ($mutation) {
        'truncation' { $bytes = $bytes[0..($fixture.DataOffset + 33)] }
        'overlap' { [Array]::Copy([BitConverter]::GetBytes([uint64]32), 0, $bytes, $fixture.Positions[1].Offset, 8) }
        'row-size' { [Array]::Copy([BitConverter]::GetBytes([uint64]129), 0, $bytes, $fixture.Positions[0].Dimension, 8) }
        'dimension-overflow' { [Array]::Copy([BitConverter]::GetBytes([uint64]::MaxValue), 0, $bytes, $fixture.Positions[0].Dimension, 8) }
    }
    $badPath = Join-Path $scratch "$mutation.gguf"
    [IO.File]::WriteAllBytes($badPath, $bytes)
    Assert-Failure { (Read-TestGguf $badPath $dialect $true) } "Malformed census payload accepted: $mutation"
}
# Stock type 42 remains Q2_0 (64 symbols/18 bytes), never an alias for PQ2_0.
$stockPath = Join-Path $scratch 'stock-q2.gguf'
New-CensusFixture $stockPath @(@{ Name = 'stock.q2'; Type = 42; Elements = 64; Payload = [byte[]]::new(18) }) | Out-Null
$stock = (Read-TestGguf $stockPath)
Assert ($stock.TensorTypeHistogram[0].Name -ceq 'Q2_0' -and $stock.TensorTypeHistogram[0].Bytes -eq 18) 'Private dialect changed stock type 42.'
Assert-Failure { (Read-TestGguf $stockPath $dialect $true) } 'Empty PQ2 census was silently accepted.'
Assert-Failure { (Read-TestGguf $stockPath 'Stock' $true) } 'Stock census was accepted.'
# The entrypoint verifies immutable artifact identity before payload counting.
$hash = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
$output = Join-Path $scratch 'census.json'
$verified = & (Join-Path $repo 'scripts/Read-Gguf.ps1') -GgufToolPath $env:BONSAI_GGUF_TOOL -ModelPath $path -ExpectedBytes $fixture.Bytes.Length -ExpectedSha256 $hash -Dialect $dialect -CensusPq2 -OutputPath $output
$saved = Get-Content -LiteralPath $output -Raw | ConvertFrom-Json -DateKind String
Assert ($verified.ArtifactVerification -eq 'verified-size-and-sha256' -and $saved.TensorPayloadRead -and $saved.Pq2Census.CodeZero -eq $census.CodeZero) 'Verified census did not round-trip.'
Assert-Failure { & (Join-Path $repo 'scripts/Read-Gguf.ps1') -GgufToolPath $env:BONSAI_GGUF_TOOL -ModelPath $path -ExpectedBytes $fixture.Bytes.Length -ExpectedSha256 ('0' * 64) -Dialect $dialect -CensusPq2 } 'Census accepted a failed artifact hash.'
Assert-Failure { & (Join-Path $repo 'scripts/Read-Gguf.ps1') -GgufToolPath $env:BONSAI_GGUF_TOOL -ModelPath $path -ExpectedBytes $fixture.Bytes.Length -ExpectedSha256 $hash -CensusPq2 } 'Entrypoint permitted an implicit Prism dialect.'
Assert-Failure { & (Join-Path $repo 'scripts/Read-Gguf.ps1') -GgufToolPath $env:BONSAI_GGUF_TOOL -ModelPath $path -ExpectedBytes $fixture.Bytes.Length -ExpectedSha256 $hash -Dialect $dialect -CensusPq2 -OutputPath $output } 'Census overwrote an existing evidence report.'
Write-Information -InformationAction Continue "PQ2 census focused regressions passed. Synthetic evidence retained at $scratch"
