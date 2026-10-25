#requires -Version 7.6
[CmdletBinding()]
[OutputType([void])]
param([ValidateSet('All','Parser','Strings','Arrays','Artifact')][string]$Case = 'All')
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if ($Case -eq 'All' -and $env:GITHUB_ACTIONS -ne 'true') { throw 'The full suite runs only in GitHub Actions. Select one focused -Case locally.' }
$repo = Split-Path $PSScriptRoot -Parent
Import-Module (Join-Path $repo 'scripts/Bonsai.Common.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'Gguf.TestSupport.psm1') -Force
$scratch = Resolve-ExternalPath (Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-GgufTests-' + [Guid]::NewGuid().ToString('N')))
[void][IO.Directory]::CreateDirectory($scratch)
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')


function Write-Key {
    [CmdletBinding()]
    [OutputType([void])]
    param([IO.BinaryWriter]$Writer, [string]$Key, [uint32]$Type)
    Write-TestGgufText -Writer $Writer -Text $Key
    $Writer.Write($Type)
}
function New-Fixture {
    [CmdletBinding(SupportsShouldProcess)]
    [OutputType([hashtable])]
    param([string]$Path)
    if (-not $PSCmdlet.ShouldProcess($Path, 'Create synthetic GGUF fixture')) { return }
    # Vetted against pinned gguf.h and reader before this implementation step.
    # Independent serialized records make digest assertions sensitive to prefixes,
    # raw record order, Unicode encoding, and accidental vocabulary materialization.
    $memory = [IO.MemoryStream]::new()
    $writer = [IO.BinaryWriter]::new($memory, [Text.Encoding]::UTF8, $true)
    try {
        $writer.Write([uint32]0x46554747); $writer.Write([uint32]3); $writer.Write([uint64]3); $writer.Write([uint64]11)
        Write-Key -Writer $writer -Key 'general.architecture' -Type 8; Write-TestGgufText $writer 'qwen35'
        Write-Key -Writer $writer -Key 'general.alignment' -Type 4; $alignmentValue = $memory.Position; $writer.Write([uint32]32)
        Write-Key -Writer $writer -Key 'qwen35.block_count' -Type 4; $writer.Write([uint32]65)
        Write-Key -Writer $writer -Key 'qwen35.nextn_predict_layers' -Type 4; $writer.Write([uint32]1)
        Write-Key -Writer $writer -Key 'qwen35.context_length' -Type 4; $writer.Write([uint32]262144)
        Write-Key -Writer $writer -Key 'qwen35.attention.head_count' -Type 4; $writer.Write([uint32]24)
        Write-Key -Writer $writer -Key 'qwen35.attention.recurrent_layers' -Type 9; $writer.Write([uint32]7); $writer.Write([uint64]4)
        $boolValue = $memory.Position; $writer.Write([byte]1); $writer.Write([byte]1); $writer.Write([byte]1); $writer.Write([byte]0)
        $tokenizerStart = $memory.Position
        Write-Key -Writer $writer -Key 'tokenizer.ggml.model' -Type 8; Write-TestGgufText $writer 'gpt2'
        Write-Key -Writer $writer -Key 'tokenizer.ggml.tokens' -Type 9; $writer.Write([uint32]8); $vocabCount = $memory.Position; $writer.Write([uint64]5000)
        for ($i = 0; $i -lt 5000; $i++) { Write-TestGgufText $writer ('token-' + $i) }
        Write-Key -Writer $writer -Key 'tokenizer.ggml.eos_token_id' -Type 4; $writer.Write([uint32]42)
        Write-Key -Writer $writer -Key 'tokenizer.chat_template' -Type 8
        $template = "{{ messages }}`nUnicode: λ 🍀`n"
        Write-TestGgufText $writer $template
        $tokenizerEnd = $memory.Position
        Write-TestGgufText $writer 'blk.0.attn_q.weight'; $dimensionsValue = $memory.Position; $writer.Write([uint32]1); $dimensionSize = $memory.Position; $writer.Write([uint64]256)
        $tensorType = $memory.Position; $writer.Write([uint32]11); $tensorOffset = $memory.Position; $writer.Write([uint64]0)
        Write-TestGgufText $writer 'blk.64.ffn_norm.weight'; $writer.Write([uint32]1); $writer.Write([uint64]2); $writer.Write([uint32]0); $writer.Write([uint64]128)
        Write-TestGgufText $writer 'blk.64.nextn.eh_proj.weight'; $writer.Write([uint32]1); $writer.Write([uint64]2); $writer.Write([uint32]0); $writer.Write([uint64]160)
        while ($memory.Position % 32) { $writer.Write([byte]0) }
        $dataOffset = $memory.Position
        $writer.Write([byte[]]::new(168)); $writer.Flush()
        $bytes = $memory.ToArray()
        [IO.File]::WriteAllBytes($Path, $bytes)
        $rawTokenizer = [byte[]]::new($tokenizerEnd - $tokenizerStart)
        [Array]::Copy($bytes, $tokenizerStart, $rawTokenizer, 0, $rawTokenizer.Length)
        return @{
            Bytes = $bytes; DataOffset = $dataOffset; AlignmentValue = $alignmentValue; BoolValue = $boolValue
            VocabCount = $vocabCount; DimensionsValue = $dimensionsValue; DimensionSize = $dimensionSize
            TensorType = $tensorType; TensorOffset = $tensorOffset
            TokenizerHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($rawTokenizer)).ToLowerInvariant()
            TemplateHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($template))).ToLowerInvariant()
        }
    } finally { $writer.Dispose(); $memory.Dispose() }
}
$path = Join-Path $scratch 'synthetic.gguf'
$fixture = New-Fixture $path
if ($Case -in @('All','Parser')) {
    $report = (Read-TestGguf $path)
    Assert ($report.Architecture -eq 'qwen35' -and $report.TensorCount -eq 3) 'Header/architecture inspection failed.'
    Assert ($report.Scalars['qwen35.context_length'] -eq 262144) 'Context scalar was lost.'
    Assert ($report.Arrays['qwen35.attention.recurrent_layers'].Count -eq 4) 'Small layout array was lost.'
    Assert ($report.Arrays['tokenizer.ggml.tokens'].Count -eq 5000) 'Vocabulary descriptor was lost.'
    Assert (-not ($report.Arrays['tokenizer.ggml.tokens'] -is [Collections.IList])) 'Vocabulary contents were materialized.'
    Assert ($report.TokenizerMetadataSha256 -ceq $fixture.TokenizerHash) 'Raw tokenizer digest differs from serialized record bytes.'
    Assert ($report.EmbeddedTemplates['tokenizer.chat_template'].Sha256 -ceq $fixture.TemplateHash) 'Template UTF-8 digest is wrong.'
    Assert ($report.SpecialTokenIds['tokenizer.ggml.eos_token_id'] -eq 42) 'Special token ID was lost.'
    Assert ($report.TensorDataOffset -eq $fixture.DataOffset -and -not $report.TensorPayloadRead) 'Unexpected payload inspection.'
    Assert ($report.TensorTypeHistogram.Count -eq 2) 'Mixed tensor types were not distinguished.'
    Assert ($report.TensorTypeHistogram[1].TypeId -eq 11 -and $report.TensorTypeHistogram[1].Name -ceq 'Q3_K') 'Tensor type numeric ID/name is wrong.'
    Assert ($report.MtpTensorCount -eq 2) 'MTP block tensors were not classified using metadata.'
    # Each corruption changes a descriptor/header invariant rather than weight data.
    foreach ($mutation in @('magic','version','alignment','boolean','array-count','dimension-count','dimension-zero','dimension-overflow','tensor-type','tensor-offset','truncated-header','truncated-payload')) {
        $bytes = [byte[]]$fixture.Bytes.Clone()
        $position = 0L; $replacement = [byte[]]@()
        switch ($mutation) {
            'magic' { $replacement = [BitConverter]::GetBytes([uint32]0) }
            'version' { $position = 4; $replacement = [BitConverter]::GetBytes([uint32]2) }
            'alignment' { $position = $fixture.AlignmentValue; $replacement = [BitConverter]::GetBytes([uint32]3) }
            'boolean' { $position = $fixture.BoolValue; $replacement = [byte[]]@(2) }
            'array-count' { $position = $fixture.VocabCount; $replacement = [BitConverter]::GetBytes([uint64]::MaxValue) }
            'dimension-count' { $position = $fixture.DimensionsValue; $replacement = [BitConverter]::GetBytes([uint32]5) }
            'dimension-zero' { $position = $fixture.DimensionSize; $replacement = [BitConverter]::GetBytes([uint64]0) }
            'dimension-overflow' { $position = $fixture.DimensionSize; $replacement = [BitConverter]::GetBytes([uint64]::MaxValue) }
            'tensor-type' { $position = $fixture.TensorType; $replacement = [BitConverter]::GetBytes([uint32]999) }
            'tensor-offset' { $position = $fixture.TensorOffset; $replacement = [BitConverter]::GetBytes([uint64]32) }
            'truncated-header' { $bytes = $bytes[0..20] }
            'truncated-payload' { $bytes = $bytes[0..($bytes.Length - 2)] }
        }
        if ($replacement.Length) { [Array]::Copy($replacement, 0, $bytes, $position, $replacement.Length) }
        $badPath = Join-Path $scratch ($mutation + '.gguf')
        [IO.File]::WriteAllBytes($badPath, $bytes)
        Assert-Failure { (Read-TestGguf $badPath) } "Corrupt GGUF was accepted: $mutation"
    }
}
if ($Case -in @('All','Strings')) {
    function New-StringFixture {
        [CmdletBinding(SupportsShouldProcess)]
        [OutputType([hashtable])]
        param([string]$Path, [string]$Value, [string]$Key = 'tokenizer.test_string')
        if (-not $PSCmdlet.ShouldProcess($Path, 'Create synthetic GGUF string fixture')) { return }
        $memory = [IO.MemoryStream]::new()
        $writer = [IO.BinaryWriter]::new($memory, [Text.Encoding]::UTF8, $true)
        try {
            $writer.Write([uint32]0x46554747); $writer.Write([uint32]3); $writer.Write([uint64]0); $writer.Write([uint64]2)
            Write-Key -Writer $writer -Key 'general.architecture' -Type 8; Write-TestGgufText $writer 'synthetic'
            $tokenizerStart = $memory.Position
            Write-Key -Writer $writer -Key $Key -Type 8
            $valueStart = $memory.Position + 8
            Write-TestGgufText $writer $Value
            $writer.Flush()
            $bytes = $memory.ToArray()
            [IO.File]::WriteAllBytes($Path, $bytes)
            $rawTokenizer = [byte[]]::new($bytes.Length - $tokenizerStart)
            [Array]::Copy($bytes, $tokenizerStart, $rawTokenizer, 0, $rawTokenizer.Length)
            return @{
                Bytes = $bytes; KeyStart = $tokenizerStart + 8; ValueStart = $valueStart
                TokenizerHash = [Convert]::ToHexStringLower([Security.Cryptography.SHA256]::HashData($rawTokenizer))
            }
        } finally { $writer.Dispose(); $memory.Dispose() }
    }
    foreach ($size in @(0,1,1024,65535,65536,65537)) {
        # End each nontrivial string with a four-byte Unicode character. This
        # reaches the exact byte limit without assuming UTF-16 length is bytes.
        $value = if ($size -lt 4) { 'a' * $size } else { ('a' * ($size - 4)) + '🍀' }
        $stringPath = Join-Path $scratch "string-$size.gguf"
        $stringFixture = New-StringFixture -Path $stringPath -Value $value
        $stringReport = (Read-TestGguf $stringPath)
        if ($size -le 65536) {
            Assert ($stringReport.Scalars['tokenizer.test_string'] -ceq $value) "Retained UTF-8 string changed at $size bytes."
        } else {
            Assert (-not $stringReport.Scalars.ContainsKey('tokenizer.test_string')) 'Over-limit retained string was materialized.'
        }
        Assert ($stringReport.TokenizerMetadataSha256 -ceq $stringFixture.TokenizerHash) "String buffer reuse changed tokenizer digest at $size bytes."
    }
    $key = 'tokenizer.' + ('a' * 1014)
    $keyPath = Join-Path $scratch 'long-key.gguf'
    $stringFixture = New-StringFixture -Path $keyPath -Value 'λ' -Key $key
    Assert ((Read-TestGguf $keyPath).Scalars[$key] -ceq 'λ') 'The 1024-byte metadata key was not decoded exactly.'
    foreach ($part in @('Key','Value')) {
        $bytes = [byte[]]$stringFixture.Bytes.Clone()
        $bytes[$stringFixture["${part}Start"]] = 255
        $badPath = Join-Path $scratch "invalid-utf8-$part.gguf"
        [IO.File]::WriteAllBytes($badPath, $bytes)
        Assert-Failure { (Read-TestGguf $badPath) } "Invalid UTF-8 in $part was accepted."
    }
    $badPath = Join-Path $scratch 'truncated-string.gguf'
    [IO.File]::WriteAllBytes($badPath, $stringFixture.Bytes[0..($stringFixture.Bytes.Length - 2)])
    Assert-Failure { (Read-TestGguf $badPath) } 'A truncated UTF-8 string was accepted.'
    $badPath = Join-Path $scratch 'over-limit-key.gguf'
    New-StringFixture -Path $badPath -Value 'test' -Key ($key + 'a') | Out-Null
    Assert-Failure { (Read-TestGguf $badPath) } 'An over-limit metadata key was accepted.'
}
if ($Case -in @('All','Arrays')) {
    function New-ArrayFixture {
        [CmdletBinding(SupportsShouldProcess)]
        [OutputType([hashtable])]
        param([string]$Path, [uint32]$ElementType, [uint64]$Count, [scriptblock]$WriteValues)
        if (-not $PSCmdlet.ShouldProcess($Path, 'Create synthetic GGUF array fixture')) { return }
        $memory = [IO.MemoryStream]::new()
        $writer = [IO.BinaryWriter]::new($memory, [Text.Encoding]::UTF8, $true)
        try {
            $writer.Write([uint32]0x46554747); $writer.Write([uint32]3); $writer.Write([uint64]0); $writer.Write([uint64]2)
            Write-Key -Writer $writer -Key 'general.architecture' -Type 8; Write-TestGgufText $writer 'synthetic'
            $tokenizerStart = $memory.Position
            Write-Key -Writer $writer -Key 'tokenizer.test_array' -Type 9; $writer.Write($ElementType); $writer.Write($Count)
            $valuesStart = $memory.Position
            & $WriteValues $writer
            $writer.Flush()
            $bytes = $memory.ToArray()
            [IO.File]::WriteAllBytes($Path, $bytes)
            $rawTokenizer = [byte[]]::new($bytes.Length - $tokenizerStart)
            [Array]::Copy($bytes, $tokenizerStart, $rawTokenizer, 0, $rawTokenizer.Length)
            return @{
                Bytes = $bytes; ValuesStart = $valuesStart
                TokenizerHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($rawTokenizer)).ToLowerInvariant()
            }
        } finally { $writer.Dispose(); $memory.Dispose() }
    }
    $arrayPath = Join-Path $scratch 'boolean-array.gguf'
    $arrayFixture = New-ArrayFixture -Path $arrayPath -ElementType 7 -Count 131077 -WriteValues {
        param($writer)
        $flags = [byte[]]::new(131077)
        $flags[0] = $flags[65536] = $flags[131076] = 1
        $writer.Write($flags)
    }
    $arrayReport = (Read-TestGguf $arrayPath)
    Assert ($arrayReport.Arrays['tokenizer.test_array'].Count -eq 131077 -and
        $arrayReport.Arrays['tokenizer.test_array'].ElementTypeId -eq 7) 'Large boolean array descriptor changed.'
    Assert ($arrayReport.TokenizerMetadataSha256 -ceq $arrayFixture.TokenizerHash) 'Boolean validation changed the raw tokenizer digest.'
    foreach ($offset in @(0,65536,131076)) {
        $bytes = [byte[]]$arrayFixture.Bytes.Clone()
        $bytes[$arrayFixture.ValuesStart + $offset] = 2
        $badPath = Join-Path $scratch "boolean-array-invalid-$offset.gguf"
        [IO.File]::WriteAllBytes($badPath, $bytes)
        Assert-Failure { (Read-TestGguf $badPath) } "Invalid boolean array value was accepted at $offset."
    }
    $badPath = Join-Path $scratch 'boolean-array-truncated.gguf'
    [IO.File]::WriteAllBytes($badPath, $arrayFixture.Bytes[0..($arrayFixture.Bytes.Length - 2)])
    Assert-Failure { (Read-TestGguf $badPath) } 'Truncated boolean array was accepted.'
    $nestedPath = Join-Path $scratch 'nested-array.gguf'
    $nestedFixture = New-ArrayFixture -Path $nestedPath -ElementType 9 -Count 3 -WriteValues {
        param($writer)
        $writer.Write([uint32]4); $writer.Write([uint64]8192); $writer.Write([byte[]]::new(8192 * 4))
        $writer.Write([uint32]7); $writer.Write([uint64]5000); $writer.Write([byte[]]::new(5000))
        $writer.Write([uint32]9); $writer.Write([uint64]1)
        $writer.Write([uint32]7); $writer.Write([uint64]1); $writer.Write([byte]1)
    }
    $nestedReport = (Read-TestGguf $nestedPath)
    Assert ($nestedReport.Arrays['tokenizer.test_array'].Count -eq 3 -and
        $nestedReport.Arrays['tokenizer.test_array'].ElementTypeId -eq 9) 'Nested array descriptor changed.'
    Assert ($nestedReport.TokenizerMetadataSha256 -ceq $nestedFixture.TokenizerHash) 'Nested array inspection changed the raw tokenizer digest.'
    $nestedFixture.Bytes[$nestedFixture.Bytes.Length - 1] = 2
    $badPath = Join-Path $scratch 'nested-array-invalid-boolean.gguf'
    [IO.File]::WriteAllBytes($badPath, $nestedFixture.Bytes)
    Assert-Failure { (Read-TestGguf $badPath) } 'A discarded nested array bypassed boolean validation.'
    $deepPath = Join-Path $scratch 'nested-array-depth.gguf'
    New-ArrayFixture -Path $deepPath -ElementType 9 -Count 1 -WriteValues {
        param($writer)
        for ($i = 0; $i -lt 3; $i++) { $writer.Write([uint32]9); $writer.Write([uint64]1) }
        $writer.Write([uint32]7); $writer.Write([uint64]1); $writer.Write([byte]1)
    } | Out-Null
    Assert-Failure { (Read-TestGguf $deepPath) } 'Boolean array validation bypassed the existing nesting limit.'
}
if ($Case -in @('All','Artifact')) {
    $hash = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
    $output = Join-Path $scratch 'inspection.json'
    $report = & (Join-Path $repo 'scripts/Read-Gguf.ps1') -GgufToolPath $env:BONSAI_GGUF_TOOL -ModelPath $path -ExpectedBytes $fixture.Bytes.Length -ExpectedSha256 $hash -OutputPath $output
    $saved = Get-Content -LiteralPath $output -Raw | ConvertFrom-Json -DateKind String
    Assert ($saved.ArtifactVerification -eq 'verified-size-and-sha256' -and $saved.TensorCount -eq 3) 'Verified JSON report did not round-trip.'
    Assert-Failure { & (Join-Path $repo 'scripts/Read-Gguf.ps1') -GgufToolPath $env:BONSAI_GGUF_TOOL -ModelPath $path -ExpectedBytes $fixture.Bytes.Length -ExpectedSha256 ('0' * 64) } 'Digest mismatch was accepted.'
    Assert-Failure { & (Join-Path $repo 'scripts/Read-Gguf.ps1') -GgufToolPath $env:BONSAI_GGUF_TOOL -ModelPath $path -ExpectedBytes $fixture.Bytes.Length -ExpectedSha256 $hash -OutputPath $output } 'Existing report was overwritten.'
    Assert-Failure { & (Join-Path $repo 'scripts/Read-Gguf.ps1') -GgufToolPath $env:BONSAI_GGUF_TOOL -ModelPath $path -ExpectedBytes $fixture.Bytes.Length } 'Incomplete artifact identity was accepted.'
}
Write-Information -InformationAction Continue "GGUF focused case '$Case' passed. Synthetic evidence retained at $scratch"
