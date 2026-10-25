#requires -Version 7.6
<#
.SYNOPSIS
Focused synthetic checks for streamed weight-lossless GGUF PTQ1 export.
#>
[CmdletBinding()]
[OutputType([void])]
param([ValidateSet('Repack','Rejection','All')][string]$Case = 'All')
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if ($Case -eq 'All' -and $env:GITHUB_ACTIONS -ne 'true') { throw 'The full suite runs only in GitHub Actions. Select one focused -Case locally.' }
Import-Module (Join-Path $PSScriptRoot 'Gguf.TestSupport.psm1') -Force
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-PtqExport-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($scratch)
$dialect = 'Prism-73bfe6cc485554822f80c473a3c0c4f25e80f41b'
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')


function New-Fixture {
    [CmdletBinding(SupportsShouldProcess)]
    [OutputType([hashtable])]
    param([string]$Path, [int]$Blocks = 1928, [uint32]$FileType = 141)
    if (-not $PSCmdlet.ShouldProcess($Path, 'Create synthetic GGUF export fixture')) { return }
    $stream = [IO.File]::Open($Path, [IO.FileMode]::CreateNew, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    $writer = [IO.BinaryWriter]::new($stream)
    try {
        $writer.Write([uint32]0x46554747); $writer.Write([uint32]3)
        $writer.Write([uint64]3); $writer.Write([uint64]4)
        Write-TestGgufText $writer 'general.architecture'; $writer.Write([uint32]8); Write-TestGgufText $writer 'synthetic'
        Write-TestGgufText $writer 'general.file_type'; $writer.Write([uint32]4); $writer.Write($FileType)
        Write-TestGgufText $writer 'tokenizer.chat_template'; $writer.Write([uint32]8); Write-TestGgufText $writer '{{ unchanged }}'
        Write-TestGgufText $writer 'prism.hadamard.transform'; $writer.Write([uint32]8); Write-TestGgufText $writer 'unchanged-transform'
        # Descriptor order deliberately differs from physical order. Source gaps
        # and final padding contain nonzero bytes, outside tensor payloads.
        $sourcePqOffset = 64L
        $secondOffset = [long]([Math]::Ceiling(($sourcePqOffset + 34 * $Blocks) / 32.0) * 32) + 32
        Write-TestGgufText $writer 'last.pq'; $writer.Write([uint32]1); $writer.Write([uint64]128); $writer.Write([uint32]142); $writer.Write([uint64]$secondOffset)
        Write-TestGgufText $writer 'first.bf16'; $writer.Write([uint32]1); $writer.Write([uint64]2); $writer.Write([uint32]30); $writer.Write([uint64]0)
        Write-TestGgufText $writer 'large.pq'; $writer.Write([uint32]1); $writer.Write([uint64](128 * $Blocks)); $writer.Write([uint32]142); $writer.Write([uint64]$sourcePqOffset)
        while ($stream.Position % 32) { $writer.Write([byte]0) }
        $data = $stream.Position
        $padding = [byte[]]::new($secondOffset + 64)
        [Array]::Fill[byte]($padding, 0xEF)
        $writer.Write($padding)
        $stream.Position = $data
        $writer.Write([byte[]]@(1,2,3,4))
        $block = [byte[]]::new(34)
        [Array]::Fill[byte]($block, 0xAA) # all +1
        $block[0] = 0; $block[1] = 0x3c
        $stream.Position = $data + $sourcePqOffset
        for ($index = 0; $index -lt $Blocks; $index++) { $writer.Write($block) }
        $stream.Position = $data + $secondOffset
        $block[1] = 0xbc # retain negative scale bits
        $writer.Write($block)
        return @{ Data = $data; PqOffset = $sourcePqOffset; LastOffset = $secondOffset; Blocks = $Blocks }
    } finally { $writer.Dispose() }
}
if ($Case -in @('Repack','All')) {
    $source = Join-Path $scratch 'source.gguf'
    $fixture = New-Fixture $source
    $target = Join-Path $scratch 'export.gguf'
    $result = (Export-TestGguf $source $target 30)
    $original = (Read-TestGguf $source $dialect $false)
    $converted = (Read-TestGguf $target $dialect $false)
    Assert ($result.ConvertedBlocks -eq $fixture.Blocks + 1 -and $result.ConvertedTensorCount -eq 2 -and $result.ExactPq2ReconstructionVerified) 'Not every PQ2 block was verified.'
    Assert ($converted.Scalars['general.file_type'] -eq 143 -and $converted.TensorTypeHistogram[1].TypeId -eq 143) 'PTQ1 file/tensor types differ.'
    Assert ($converted.TokenizerMetadataSha256 -ceq $original.TokenizerMetadataSha256 -and $converted.Scalars['prism.hadamard.transform'] -ceq 'unchanged-transform') 'Tokenizer, template or transform metadata changed.'
    Assert ($result.OutputSha256 -ceq (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash.ToLowerInvariant() -and $result.OutputBytes -eq (Get-Item -LiteralPath $target).Length) 'Export receipt differs from written bytes.'
    Assert ($result.WorkingBufferBytes -lt 256KB) 'Exporter working buffers exceed their bounded allowance.'
    $bytes = [IO.File]::ReadAllBytes($target)
    Assert ([Convert]::ToHexString($bytes, [int]$fixture.Data, 4) -ceq '01020304') 'Non-PQ tensor bytes changed.'
    # Build the independent all-+1 wire oracle once, including compacted gaps
    # and final padding. One native comparison replaces a PowerShell pipeline
    # over every five-trit byte and thousands of repeated assertion invocations.
    $lastPackedOffset = [int]([Math]::Ceiling((32 + 28 * $fixture.Blocks) / 32.0) * 32)
    $expectedPayload = [byte[]]::new([int]([Math]::Ceiling(($lastPackedOffset + 28) / 32.0) * 32))
    [Array]::Copy([byte[]]@(1,2,3,4), $expectedPayload, 4)
    $expectedBlock = [byte[]]::new(28)
    [Array]::Fill[byte]($expectedBlock, 255)
    $expectedBlock[24] = $expectedBlock[25] = 253
    $expectedBlock[26] = 0; $expectedBlock[27] = 0x3c
    for ($block = 0; $block -lt $fixture.Blocks; $block++) {
        [Array]::Copy($expectedBlock, 0, $expectedPayload, 32 + 28 * $block, 28)
    }
    $expectedBlock[27] = 0xbc
    [Array]::Copy($expectedBlock, 0, $expectedPayload, $lastPackedOffset, 28)
    Assert ([Convert]::ToHexString($bytes, [int]$fixture.Data, $bytes.Length - [int]$fixture.Data) -ceq
        [Convert]::ToHexString($expectedPayload)) 'Compacted PTQ1 payload differs from independent wire bytes, raw scale bits or zero padding.'
    Assert ($result.OutputBytes -lt $result.SourceBytes) 'Synthetic export did not compact tensor payloads/gaps.'
}
if ($Case -in @('Rejection','All')) {
    $source = Join-Path $scratch 'invalid.gguf'
    $fixture = New-Fixture $source -Blocks 1
    $bytes = [IO.File]::ReadAllBytes($source)
    $bytes[$fixture.Data + $fixture.LastOffset + 2] = 0xff # +2 in final tensor, after earlier writes
    [IO.File]::WriteAllBytes($source, $bytes)
    $partial = Join-Path $scratch 'failed.partial'
    Assert-Failure { (Export-TestGguf $source $partial 30) } 'Late fourth symbol was silently recoded.'
    Assert (-not (Test-Path -LiteralPath $partial)) 'Failed export published a final output.'
    Assert (@(Get-ChildItem -LiteralPath $scratch -Filter 'failed.partial.partial-*').Count -eq 1) 'Failure evidence was unexpectedly removed.'
    [IO.File]::WriteAllBytes($partial, [byte[]]@(1,2,3,4))
    $before = [IO.File]::ReadAllBytes($partial)
    Assert-Failure { (Export-TestGguf $source $partial 30) } 'Existing output was overwritten.'
    Assert ([Convert]::ToHexString($before) -ceq [Convert]::ToHexString([IO.File]::ReadAllBytes($partial))) 'CreateNew rejection modified existing output.'
    $wrong = Join-Path $scratch 'wrong-file-type.gguf'
    New-Fixture $wrong -Blocks 1 -FileType 143 | Out-Null
    Assert-Failure { Export-TestGguf $wrong (Join-Path $scratch 'wrong-output.gguf') 30 } 'Non-PQ2 file metadata was relabeled.'
}
Write-Output "PTQ1 export $Case checks passed. Evidence: $scratch"
