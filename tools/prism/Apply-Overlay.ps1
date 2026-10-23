#requires -Version 7.6
<#
.SYNOPSIS
Applies the exact CPU LoRA B storage overlay to a verified pinned Prism tree.
.DESCRIPTION
Rejects source drift and repeat application. Changes only src/llama-adapter.cpp
and adds src/llama-lora-b-share.h. It does not build, download, or run inference.
#>
[CmdletBinding()]
[OutputType([Collections.IDictionary])]
param([Parameter(Mandatory)][string]$SourceDirectory)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if (-not [IO.Path]::IsPathFullyQualified($SourceDirectory) -or $SourceDirectory -match '%[^%]+%') { throw 'SourceDirectory must be expanded and absolute.' }
Import-Module (Join-Path $PSScriptRoot '../../scripts/Bonsai.Common.psm1') -Force
$source = Resolve-ExternalPath -Path $SourceDirectory
$adapterPath = Resolve-ExternalPath -Path (Join-Path $source 'src/llama-adapter.cpp')
$headerPath = Resolve-ExternalPath -Path (Join-Path $source 'src/llama-lora-b-share.h')
$lock = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'source.lock.json') -Raw | ConvertFrom-Json -DateKind String
if ((Get-FileHash -LiteralPath $adapterPath -Algorithm SHA256).Hash.ToLowerInvariant() -cne $lock.AdapterSourceSha256) { throw 'Pinned llama-adapter.cpp source hash mismatch.' }
if ([IO.File]::Exists($headerPath)) { throw 'Overlay header already exists; use a new extracted source tree.' }
$text = [IO.File]::ReadAllText($adapterPath)
function Edit-ExactText {
    [CmdletBinding()]
    [OutputType([string])]
    param([string]$Text, [string]$Before, [string]$After)
    # Git may check this script out with CRLF on Windows; the hash-pinned
    # upstream source and deterministic output use LF on every platform.
    $Before = $Before.Replace("`r`n", "`n")
    $After = $After.Replace("`r`n", "`n")
    $first = $Text.IndexOf($Before, [StringComparison]::Ordinal)
    if ($first -lt 0 -or $Text.IndexOf($Before, $first + $Before.Length, [StringComparison]::Ordinal) -ge 0) { throw 'Expected a unique exact overlay insertion point.' }
    return $Text.Replace($Before, $After, [StringComparison]::Ordinal)
}
$text = Edit-ExactText -Text $text -Before '#include "llama-adapter.h"' -After "#include `"llama-adapter.h`"`n#include `"llama-lora-b-share.h`""
$before = '    // add tensors'
$after = @'
    // One file handle supplies both byte-identity planning and final loading.
    llama_file gguf_file(path_lora, "rb");
    auto read_tensor = [&](ggml_tensor * orig, std::vector<uint8_t> & bytes) {
        const int index = gguf_find_tensor(ctx_gguf.get(), orig->name);
        if (index < 0) {
            throw std::runtime_error("LoRA tensor is missing from its GGUF descriptor");
        }
        const size_t data_offset = gguf_get_data_offset(ctx_gguf.get());
        const size_t tensor_offset = gguf_get_tensor_offset(ctx_gguf.get(), index);
        const size_t size = ggml_nbytes(orig);
        if (data_offset > gguf_file.size() || tensor_offset > gguf_file.size() - data_offset ||
            size > gguf_file.size() - data_offset - tensor_offset) {
            throw std::runtime_error(format("LoRA tensor '%s' data is not within the file bounds, file is corrupted or incomplete", orig->name));
        }
        bytes.resize(size);
        gguf_file.seek(data_offset + tensor_offset, SEEK_SET);
        gguf_file.read_raw(bytes.data(), size);
    };
    llama_lora_b_share b_share;
    std::vector<uint8_t> b_bytes;

    // add tensors
'@
$text = Edit-ExactText -Text $text -Before $before -After $after
$text = Edit-ExactText -Text $text -Before '        ggml_tensor * tensor_b = ggml_dup_tensor(dev_ctx, w.b);' -After @'
        b_bytes.clear();
        if (llama_lora_b_share::eligible(w.b, buft)) {
            read_tensor(w.b, b_bytes);
        }
        ggml_tensor * tensor_b = b_share.make(dev_ctx, w.b, buft, b_bytes);
'@
$text = Edit-ExactText -Text $text -Before '    // allocate tensors / buffers and zero
    {
        adapter.ctxs.reserve(ctx_map.size());' -After @'
    LLAMA_LOG_INFO("%s: shared %zu immutable CPU LoRA B tensors, avoiding %zu payload bytes\n",
                   __func__, b_share.shared_tensors(), b_share.shared_bytes());

    // allocate tensors / buffers and zero
    {
        adapter.ctxs.reserve(ctx_map.size());
'@
$before = @'
        llama_file gguf_file(path_lora, "rb");
        std::vector<uint8_t> read_buf;
        auto set_tensor = [&](ggml_tensor * orig, ggml_tensor * dev) {
            const size_t offs = gguf_get_data_offset(ctx_gguf.get()) + gguf_get_tensor_offset(ctx_gguf.get(), gguf_find_tensor(ctx_gguf.get(), orig->name));
            const size_t size = ggml_nbytes(orig);
            if (offs + size < offs || offs + size > gguf_file.size()) {
                throw std::runtime_error(format("LoRA tensor '%s' data is not within the file bounds, file is corrupted or incomplete", orig->name));
            }
            read_buf.resize(size);
            gguf_file.seek(offs, SEEK_SET);
            gguf_file.read_raw(read_buf.data(), size);
            ggml_backend_tensor_set(dev, read_buf.data(), 0, size);
        };
'@
$after = @'
        std::vector<uint8_t> read_buf;
        auto set_tensor = [&](ggml_tensor * orig, ggml_tensor * dev) {
            // A pooled B view is immutable and its canonical owner is loaded by
            // the same ab_map iteration that created it. Never overwrite aliases.
            if (dev->view_src != nullptr) {
                return;
            }
            read_tensor(orig, read_buf);
            ggml_backend_tensor_set(dev, read_buf.data(), 0, read_buf.size());
        };
'@
$text = Edit-ExactText -Text $text -Before $before -After $after
$bytes = [Text.UTF8Encoding]::new($false).GetBytes($text)
$outputHash = [Convert]::ToHexStringLower([Security.Cryptography.SHA256]::HashData($bytes))
if ($outputHash -cne $lock.OverlaidAdapterSha256) { throw "Overlay output hash mismatch ($outputHash); no source file was changed." }
[IO.File]::WriteAllBytes($adapterPath, $bytes)
[IO.File]::Copy((Join-Path $PSScriptRoot 'llama-lora-b-share.h'), $headerPath, $false)
return [ordered]@{
    UpstreamCommit = $lock.Commit
    AdapterSourceSha256 = $lock.AdapterSourceSha256
    OverlaidAdapterSha256 = $outputHash
    HeaderSha256 = (Get-FileHash -LiteralPath $headerPath -Algorithm SHA256).Hash.ToLowerInvariant()
    SourceDirectory = $source
    Qualification = 'CPU storage-only overlay; throughput and output-quality qualification is still required'
}
