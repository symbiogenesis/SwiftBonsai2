#requires -Version 7.6
[CmdletBinding()]
param([ValidateSet('All','Parser','Artifact')][string]$Case = 'All')
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ($Case -eq 'All' -and $env:GITHUB_ACTIONS -ne 'true') { throw 'The full suite runs only in GitHub Actions. Select one focused -Case locally.' }
$repo = Split-Path $PSScriptRoot -Parent
Import-Module (Join-Path $repo 'scripts/Bonsai.Common.psm1') -Force
if (-not ('SwiftBonsai.GgufInspector' -as [type])) { Add-Type -Path (Join-Path $repo 'scripts/GgufMetadata.cs') }
$scratch = Resolve-ExternalPath (Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-GgufTests-' + [Guid]::NewGuid().ToString('N')))
[IO.Directory]::CreateDirectory($scratch) | Out-Null
function Assert([bool]$Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
function Expect-Failure([scriptblock]$Action, [string]$Message) {
    $failed = $false
    try { & $Action | Out-Null } catch { $failed = $true }
    Assert $failed $Message
}
function Write-Text([IO.BinaryWriter]$Writer, [string]$Text) {
    $bytes = [Text.Encoding]::UTF8.GetBytes($Text)
    $Writer.Write([uint64]$bytes.Length); $Writer.Write($bytes)
}
function Write-Key([IO.BinaryWriter]$Writer, [string]$Key, [uint32]$Type) { Write-Text $Writer $Key; $Writer.Write($Type) }
function New-Fixture([string]$Path) {
    # Vetted against pinned gguf.h and reader before this implementation step.
    # Independent serialized records make digest assertions sensitive to prefixes,
    # raw record order, Unicode encoding, and accidental vocabulary materialization.
    $memory = [IO.MemoryStream]::new()
    $writer = [IO.BinaryWriter]::new($memory, [Text.Encoding]::UTF8, $true)
    try {
        $writer.Write([uint32]0x46554747); $writer.Write([uint32]3); $writer.Write([uint64]3); $writer.Write([uint64]11)
        Write-Key $writer 'general.architecture' 8; Write-Text $writer 'qwen35'
        Write-Key $writer 'general.alignment' 4; $alignmentValue = $memory.Position; $writer.Write([uint32]32)
        Write-Key $writer 'qwen35.block_count' 4; $writer.Write([uint32]65)
        Write-Key $writer 'qwen35.nextn_predict_layers' 4; $writer.Write([uint32]1)
        Write-Key $writer 'qwen35.context_length' 4; $writer.Write([uint32]262144)
        Write-Key $writer 'qwen35.attention.head_count' 4; $writer.Write([uint32]24)
        Write-Key $writer 'qwen35.attention.recurrent_layers' 9; $writer.Write([uint32]7); $writer.Write([uint64]4)
        $boolValue = $memory.Position; $writer.Write([byte]1); $writer.Write([byte]1); $writer.Write([byte]1); $writer.Write([byte]0)
        $tokenizerStart = $memory.Position
        Write-Key $writer 'tokenizer.ggml.model' 8; Write-Text $writer 'gpt2'
        Write-Key $writer 'tokenizer.ggml.tokens' 9; $writer.Write([uint32]8); $vocabCount = $memory.Position; $writer.Write([uint64]5000)
        for ($i = 0; $i -lt 5000; $i++) { Write-Text $writer ('token-' + $i) }
        Write-Key $writer 'tokenizer.ggml.eos_token_id' 4; $writer.Write([uint32]42)
        Write-Key $writer 'tokenizer.chat_template' 8
        $template = "{{ messages }}`nUnicode: λ 🍀`n"
        Write-Text $writer $template
        $tokenizerEnd = $memory.Position
        Write-Text $writer 'blk.0.attn_q.weight'; $dimensionsValue = $memory.Position; $writer.Write([uint32]1); $dimensionSize = $memory.Position; $writer.Write([uint64]256)
        $tensorType = $memory.Position; $writer.Write([uint32]11); $tensorOffset = $memory.Position; $writer.Write([uint64]0)
        Write-Text $writer 'blk.64.ffn_norm.weight'; $writer.Write([uint32]1); $writer.Write([uint64]2); $writer.Write([uint32]0); $writer.Write([uint64]128)
        Write-Text $writer 'blk.64.nextn.eh_proj.weight'; $writer.Write([uint32]1); $writer.Write([uint64]2); $writer.Write([uint32]0); $writer.Write([uint64]160)
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
    $report = [SwiftBonsai.GgufInspector]::Read($path)
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
        Expect-Failure { [SwiftBonsai.GgufInspector]::Read($badPath) } "Corrupt GGUF was accepted: $mutation"
    }
}
if ($Case -in @('All','Artifact')) {
    $hash = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
    $output = Join-Path $scratch 'inspection.json'
    $report = & (Join-Path $repo 'scripts/Read-Gguf.ps1') -ModelPath $path -ExpectedBytes $fixture.Bytes.Length -ExpectedSha256 $hash -OutputPath $output
    $saved = Get-Content -LiteralPath $output -Raw | ConvertFrom-Json -DateKind String
    Assert ($saved.ArtifactVerification -eq 'verified-size-and-sha256' -and $saved.TensorCount -eq 3) 'Verified JSON report did not round-trip.'
    Expect-Failure { & (Join-Path $repo 'scripts/Read-Gguf.ps1') -ModelPath $path -ExpectedBytes $fixture.Bytes.Length -ExpectedSha256 ('0' * 64) } 'Digest mismatch was accepted.'
    Expect-Failure { & (Join-Path $repo 'scripts/Read-Gguf.ps1') -ModelPath $path -ExpectedBytes $fixture.Bytes.Length -ExpectedSha256 $hash -OutputPath $output } 'Existing report was overwritten.'
    Expect-Failure { & (Join-Path $repo 'scripts/Read-Gguf.ps1') -ModelPath $path -ExpectedBytes $fixture.Bytes.Length } 'Incomplete artifact identity was accepted.'
}
Write-Host "GGUF focused case '$Case' passed. Synthetic evidence retained at $scratch"
