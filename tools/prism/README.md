# Pinned Prism CPU LoRA storage overlay

This source overlay targets only Prism commit
`73bfe6cc485554822f80c473a3c0c4f25e80f41b`. It shares identical, immutable
rank-one F32 LoRA B vectors in the ordinary CPU buffer. It leaves all A tensors,
arithmetic, scales, model weights, and the approved adapter file intact.

The loader compares the complete payload and layout before creating a
same-context `ggml_view_tensor`. Each view keeps its own name. The real ggml
allocator allocates the canonical owner once and initializes views normally;
the loader writes only owners. GPU, repacked, other buffer types, non-F32,
noncontiguous, rank greater than one, and B tensors larger than 64 KiB retain
the original allocation behavior. The temporary comparison pool holds at most
1 MiB and 64 canonical entries. Existing matches remain usable after that cap.
Comparison uses exact bytes, including signed zero and NaN payloads.

The approved adapter has 129 identical 20,480-byte B vectors, so the expected
payload saving is 128 × 20,480 = **2,621,440 bytes (2.5 MiB)**. This does not
explain or eliminate the much larger graph-workspace premium observed when
loading a LoRA. No tokens-per-second improvement is claimed. The candidate
requires the standard profile's independent throughput and output-quality
qualification before promotion.

Download the ZIP at `source.lock.json`'s exact URL into external storage and
verify its SHA-256. Then, using PowerShell 7.6 and CMake/Ninja with a Clang
toolchain available:

```powershell
$result = ./scripts/Build-PrismEngine.ps1 -SourceArchive $absoluteZip -BuildMode All
ctest --test-dir (Join-Path $result.EvidenceDirectory 'build-probe') --output-on-failure
```

The script verifies the complete ZIP, extracts a new external temp source lane,
verifies the original adapter source hash, applies exact substitutions, and
checks the resulting source hash before writing. All source/build/log outputs
remain external. `-PrepareOnly` returns the source directory for another build
driver. `-BuildMode Probe` builds only the focused native test. Engine mode
builds completion and server with two workers and records binary hashes; it
does not install, execute, or select those binaries. Windows requires an ARM64
Clang-compatible development environment; a Visual Studio generator selects
`ClangCL`. The existing released runtime remains pinned separately.

The focused native regression uses the actual pinned ggml CPU allocator and
backend. It verifies 129 independent tensor names, shared data pointers and
strides, exact 2.5 MiB buffer saving, and byte-identical shared/copy MUL_MAT
results. It also exercises mismatched payloads/layouts, CPU scope, signed zero,
distinct NaN payloads, context boundaries, and bounded fallback. These are
synthetic correctness checks; they are not model-quality or speed benchmarks.
`tests/Test-PrismOverlay.ps1` separately checks exact source application and
source-drift/repeat rejection. Full native and model performance checks run in
CI under the repository's validation policy.

No graph-reserve change or floating-point fusion is included. Those require
separate evidence. Sources used to establish view allocation and CPU buffer
behavior are the pinned upstream
[adapter loader](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-adapter.cpp),
[tensor views](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml.c), and
[backend allocation](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-alloc.c).
The upstream MIT notice is retained in `PRISM-LICENSE.txt` for the loader
snippets used by the overlay. The complete upstream tree remains in the
external verified source archive.
