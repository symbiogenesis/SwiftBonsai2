# Prism implementation reuse review

Reviewed October 24, 2026. Scope: missing safe Rust Prism runtime behavior.
Upstream documentation and comments were treated as reference data, with no
upstream agent instructions or setup scripts adopted. Source implementations,
licenses and the pinned Prism contracts determine what can be reused.

## Sources and decisions

| Project and pinned revision | Relevant code | Decision |
| --- | --- | --- |
| [Camelid, `1829a7e`](https://github.com/timtoole02/Camelid/tree/1829a7e73775eb829efe8fabf46a6680897a975b), MIT | [`src/tensor/mod.rs`](https://github.com/timtoole02/Camelid/blob/1829a7e73775eb829efe8fabf46a6680897a975b/src/tensor/mod.rs), Q1/Q2 decoders; [`src/fabric/http.rs`](https://github.com/timtoole02/Camelid/blob/1829a7e73775eb829efe8fabf46a6680897a975b/src/fabric/http.rs#L352), body framing | Adapt decoder structure and complete-body checks into existing Prism kernels and model resolution. Retain Prism type 42's 64-element layout; Camelid also recognizes another deployed dialect. |
| [Candle, `12bb5d1`](https://github.com/huggingface/candle/tree/12bb5d1f6350f4ea9827d9086f213feec1f0af6a), MIT/Apache-2.0 | [`segment_anything/image_encoder.rs`](https://github.com/huggingface/candle/blob/12bb5d1f6350f4ea9827d9086f213feec1f0af6a/candle-transformers/src/models/segment_anything/image_encoder.rs#L44), [`backprop.rs`](https://github.com/huggingface/candle/blob/12bb5d1f6350f4ea9827d9086f213feec1f0af6a/candle-core/src/backprop.rs#L343), quantized types | Useful mathematical references. No code imported: relative-position addition order differs; pooling backward excludes overlapping windows and uses different tie behavior. Its flash-attention path has no CPU implementation. Quantized types do not supply the remaining Prism IQ codecs. |
| [mistral.rs, `9459891`](https://github.com/EricLBuehler/mistral.rs/tree/9459891e7697238225885518119682ac8209a612), MIT | [`Cargo.toml`](https://github.com/EricLBuehler/mistral.rs/blob/9459891e7697238225885518119682ac8209a612/Cargo.toml#L42), Candle kernel dependencies | No code imported. The reviewed kernels depend on Candle and add no independent exact CPU implementation for these missing Prism operations. |
| [Prism/ggml, `b87ca72`](https://github.com/PrismML-Eng/llama.cpp/tree/b87ca7225456b06b0975b8239c66b5aecf07e37c), MIT | [`ggml.c`](https://github.com/PrismML-Eng/llama.cpp/blob/b87ca7225456b06b0975b8239c66b5aecf07e37c/ggml/src/ggml.c), [`ggml-cpu/ops.cpp`](https://github.com/PrismML-Eng/llama.cpp/blob/b87ca7225456b06b0975b8239c66b5aecf07e37c/ggml/src/ggml-cpu/ops.cpp) | Adapt original scalar CPU contracts for windows, relative positions, scalar addition, im2col backward and pooling backward. |

Camelid attribution is retained in [NOTICE](../NOTICE) and its complete
[MIT license](../src/prism-runtime/CAMELID-LICENSE.txt). Existing ggml attribution
covers the direct Prism adaptations. No framework dependency was added.

## Integrated behavior and focused evidence

- `Q1_0` and `Q2_0`: checked storage, reference encoding, arbitrary packed
  decoding, native `Q8_0` activation dots, prepared matvec and CPU graph routing.
  Eighteen native scenarios compare encoded bytes, decoded float bytes and dots.
  Native hashes cover every payload-byte value at every payload position:
  8,192 blocks and 786,432 decoded values. The retained fixture in
  `src/prism-runtime/tests/fixtures/upstream-signquants.json` records provenance.
- CPU graphs: F32/F16/BF16 `ADD1`, `WIN_PART`, `WIN_UNPART`, `GET_REL_POS`,
  `ADD_REL_POS`, `IM2COL_BACK` and `POOL_2D_BACK`. Checks cover padded rows/windows,
  both patch axes, channels/batches, strided gradients, overlapping pooling,
  first-maximum ties, nonfinite comparisons, native F32 accumulation order,
  raw F16/BF16 copying, extra slabs and in-place aliases.
- Model retrieval: Content-Length agreement, complete chunk termination,
  trailers, unambiguous framing and decoded-body bounds. Curl output is bounded
  during capture; failed/oversized children are terminated and reaped. Invalid
  transfers cannot publish a model blob or snapshot. The original model-resolution
  source passed 94 listing variants, 13 handler assemblies, 757 predicates,
  358 guards and 228 loopback GETs.

These are focused correctness results. Full upstream/backend matrices and all
performance workloads remain hosted qualification work; the accepted baseline
is unchanged. Production model admission remains limited to existing Qwen35
formats. Packed `ADD1`, IQ1/2/3 codecs, `FLASH_ATTN_BACK`, custom callbacks, full
backward graphs, CLI/download completion and architecture/session APIs remain
open. Window and relative-position extraction currently require contiguous
source storage; no broader strided-source claim is made.
HTTP follow-up also retains the existing wire-size cap for chunk framing
overhead and incomplete validation of optional chunk-extension syntax.
