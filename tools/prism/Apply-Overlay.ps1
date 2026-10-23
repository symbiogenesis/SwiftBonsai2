#requires -Version 7.6
<#
.SYNOPSIS
Applies the exact CPU LoRA B storage overlay to a verified pinned Prism tree.
.DESCRIPTION
Rejects source drift and repeat application. Changes only src/llama-adapter.cpp
and src/CMakeLists.txt, and adds the Rust library's C ABI declarations.
It does not build, download, or run inference.
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
$headerPath = Resolve-ExternalPath -Path (Join-Path $source 'src/bonsai_prism.h')
$cmakePath = Resolve-ExternalPath -Path (Join-Path $source 'src/CMakeLists.txt')
$lock = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'source.lock.json') -Raw | ConvertFrom-Json -DateKind String
if ((Get-FileHash -LiteralPath $adapterPath -Algorithm SHA256).Hash.ToLowerInvariant() -cne $lock.AdapterSourceSha256) { throw 'Pinned llama-adapter.cpp source hash mismatch.' }
if ((Get-FileHash -LiteralPath $cmakePath -Algorithm SHA256).Hash.ToLowerInvariant() -cne $lock.CMakeSourceSha256) { throw 'Pinned src/CMakeLists.txt source hash mismatch.' }
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
$text = Edit-ExactText -Text $text -Before '#include "llama-adapter.h"' -After "#include `"llama-adapter.h`"`n#include `"bonsai_prism.h`"`n#include <algorithm>`n#include <memory>"
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
    // The pinned engine owns ggml pointers and materializes the Rust decisions.
    // Context/buffer tokens are identities only; Rust never dereferences them.
    std::unique_ptr<bonsai_prism_pool, decltype(&bonsai_prism_pool_free)> b_share(
        bonsai_prism_pool_new(), &bonsai_prism_pool_free);
    if (!b_share) {
        throw std::runtime_error("Rust LoRA sharing pool allocation failed");
    }
    auto describe_b = [](ggml_context * ctx, const ggml_tensor * tensor, ggml_backend_buffer_type_t buft) {
        bonsai_prism_layout layout = {};
        layout.context = reinterpret_cast<uintptr_t>(ctx);
        layout.buffer_type = reinterpret_cast<uintptr_t>(buft);
        layout.cpu = buft == ggml_backend_cpu_buffer_type();
        layout.f32 = tensor->type == GGML_TYPE_F32;
        layout.contiguous = ggml_is_contiguous(tensor);
        std::copy(tensor->ne, tensor->ne + GGML_MAX_DIMS, layout.dimensions);
        std::copy(tensor->nb, tensor->nb + GGML_MAX_DIMS, layout.strides);
        layout.byte_len = ggml_nbytes(tensor);
        return layout;
    };
    std::vector<uint8_t> b_bytes;

    // add tensors
'@
$text = Edit-ExactText -Text $text -Before $before -After $after
$text = Edit-ExactText -Text $text -Before '        ggml_tensor * tensor_b = ggml_dup_tensor(dev_ctx, w.b);' -After @'
        b_bytes.clear();
        const auto b_layout = describe_b(dev_ctx, w.b, buft);
        if (bonsai_prism_pool_eligible(&b_layout)) {
            read_tensor(w.b, b_bytes);
        }
        const auto owner = bonsai_prism_pool_find(b_share.get(), &b_layout, b_bytes.data(), b_bytes.size());
        ggml_tensor * tensor_b = owner != 0
            ? ggml_view_tensor(dev_ctx, reinterpret_cast<ggml_tensor *>(owner))
            : ggml_dup_tensor(dev_ctx, w.b);
        if (owner == 0) {
            const auto owner_layout = describe_b(dev_ctx, tensor_b, buft);
            bonsai_prism_pool_remember(b_share.get(), &owner_layout, b_bytes.data(), b_bytes.size(),
                                       reinterpret_cast<uintptr_t>(tensor_b));
        }
'@
$text = Edit-ExactText -Text $text -Before '    // allocate tensors / buffers and zero
    {
        adapter.ctxs.reserve(ctx_map.size());' -After @'
    LLAMA_LOG_INFO("%s: shared %zu immutable CPU LoRA B tensors, avoiding %zu payload bytes\n",
                   __func__, bonsai_prism_pool_shared_tensors(b_share.get()), bonsai_prism_pool_shared_bytes(b_share.get()));

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
$cmake = [IO.File]::ReadAllText($cmakePath)
$cmake = Edit-ExactText -Text $cmake -Before 'target_link_libraries(llama PUBLIC ggml)' -After @'
target_link_libraries(llama PUBLIC ggml)
if(NOT BONSAI_PRISM_LIBRARY OR NOT EXISTS "${BONSAI_PRISM_LIBRARY}")
    message(FATAL_ERROR "BONSAI_PRISM_LIBRARY must name the built Rust static library")
endif()
add_library(bonsai-prism STATIC IMPORTED)
set_target_properties(bonsai-prism PROPERTIES IMPORTED_LOCATION "${BONSAI_PRISM_LIBRARY}")
target_link_libraries(llama PRIVATE bonsai-prism)
if(WIN32)
    target_link_libraries(llama PRIVATE ws2_32 userenv ntdll dbghelp)
else()
    target_link_libraries(llama PRIVATE ${CMAKE_DL_LIBS} pthread m)
    if(CMAKE_SYSTEM_NAME STREQUAL "Linux")
        target_link_libraries(llama PRIVATE rt util)
    endif()
endif()
'@
$cmakeBytes = [Text.UTF8Encoding]::new($false).GetBytes($cmake)
$cmakeHash = [Convert]::ToHexStringLower([Security.Cryptography.SHA256]::HashData($cmakeBytes))
if ($cmakeHash -cne $lock.OverlaidCMakeSha256) { throw "Overlay CMake output hash mismatch ($cmakeHash); no source file was changed." }
# Validate both deterministic outputs before publishing any source changes.
[IO.File]::Copy((Join-Path $PSScriptRoot 'include/bonsai_prism.h'), $headerPath, $false)
[IO.File]::WriteAllBytes($adapterPath, $bytes)
[IO.File]::WriteAllBytes($cmakePath, $cmakeBytes)
return [ordered]@{
    UpstreamCommit = $lock.Commit
    AdapterSourceSha256 = $lock.AdapterSourceSha256
    OverlaidAdapterSha256 = $outputHash
    CMakeSourceSha256 = $lock.CMakeSourceSha256
    OverlaidCMakeSha256 = $cmakeHash
    HeaderSha256 = (Get-FileHash -LiteralPath $headerPath -Algorithm SHA256).Hash.ToLowerInvariant()
    SourceDirectory = $source
    Qualification = 'CPU storage-only overlay; throughput and output-quality qualification is still required'
}
