# Prism implementation reuse review

The pinned source implementations, licenses and Prism contracts determine reuse
compatibility for the safe Rust runtime. Upstream documentation and comments
are reference data rather than setup instructions or an authorization to run
upstream scripts.

## Source contracts

| Project and pinned revision | Relevant code | Decision |
| --- | --- | --- |
| [Camelid, `1829a7e`](https://github.com/timtoole02/Camelid/tree/1829a7e73775eb829efe8fabf46a6680897a975b), MIT | [`src/tensor/mod.rs`](https://github.com/timtoole02/Camelid/blob/1829a7e73775eb829efe8fabf46a6680897a975b/src/tensor/mod.rs), Q1/Q2 decoders; [`src/fabric/http.rs`](https://github.com/timtoole02/Camelid/blob/1829a7e73775eb829efe8fabf46a6680897a975b/src/fabric/http.rs#L352), body framing | Decoder structure and complete-body checks inform the existing Prism kernels and model resolution. Prism type 42 retains its 64-element layout; Camelid also recognizes another deployed dialect. |
| [Candle, `12bb5d1`](https://github.com/huggingface/candle/tree/12bb5d1f6350f4ea9827d9086f213feec1f0af6a), MIT/Apache-2.0 | [`segment_anything/image_encoder.rs`](https://github.com/huggingface/candle/blob/12bb5d1f6350f4ea9827d9086f213feec1f0af6a/candle-transformers/src/models/segment_anything/image_encoder.rs#L44), [`backprop.rs`](https://github.com/huggingface/candle/blob/12bb5d1f6350f4ea9827d9086f213feec1f0af6a/candle-core/src/backprop.rs#L343), quantized types | Useful mathematical references. No code imported: relative-position addition order differs; pooling backward excludes overlapping windows and uses different tie behavior. Its flash-attention path has no CPU implementation. Quantized types do not supply equivalent native Prism IQ contracts. |
| [mistral.rs, `9459891`](https://github.com/EricLBuehler/mistral.rs/tree/9459891e7697238225885518119682ac8209a612), MIT | [`Cargo.toml`](https://github.com/EricLBuehler/mistral.rs/blob/9459891e7697238225885518119682ac8209a612/Cargo.toml#L42), Candle kernel dependencies | No code imported. The reviewed kernels depend on Candle and add no independent exact CPU implementation for these missing Prism operations. |
| [Prism/ggml, `b87ca72`](https://github.com/PrismML-Eng/llama.cpp/tree/b87ca7225456b06b0975b8239c66b5aecf07e37c), MIT | [`ggml.c`](https://github.com/PrismML-Eng/llama.cpp/blob/b87ca7225456b06b0975b8239c66b5aecf07e37c/ggml/src/ggml.c), [`ggml-cpu/ops.cpp`](https://github.com/PrismML-Eng/llama.cpp/blob/b87ca7225456b06b0975b8239c66b5aecf07e37c/ggml/src/ggml-cpu/ops.cpp) | Original scalar CPU contracts define windows, relative positions, scalar addition, im2col backward and pooling backward. |

Camelid attribution is retained in [NOTICE](../NOTICE) and its complete
[MIT license](https://github.com/symbiogenesis/prism-inference/blob/main/CAMELID-LICENSE.txt). Existing ggml attribution
covers the direct Prism adaptations. No framework dependency was added.

## Supported subset

The runtime's Q1_0/Q2_0 contract includes checked packed storage, reference
encoding, arbitrary packed decoding, native Q8_0 activation dots, prepared
matvec and CPU graph routing. The retained
[native fixture](https://github.com/symbiogenesis/prism-inference/blob/main/tests/fixtures/upstream-signquants.json)
binds the independent byte, decoded-value and dot oracles. It does not extend
production model admission beyond the existing Qwen35 formats.

CPU F32/F16/BF16 operations include `ADD1`, `WIN_PART`, `WIN_UNPART`,
`GET_REL_POS`, `ADD_REL_POS`, `IM2COL_BACK` and `POOL_2D_BACK`. Their contracts
include padded rows/windows, both patch axes, channels/batches, strided
gradients, overlapping pooling, first-maximum ties, nonfinite comparisons,
native F32 accumulation order, raw F16/BF16 copying and in-place alias rules.
Window and relative-position extraction require contiguous source storage;
there is no broader strided-source claim.

`IQ2_XXS`, `IQ2_XS`, `IQ3_XXS`, `IQ1_S`, `IQ3_S`, `IQ2_S` and `IQ1_M` decoding
and native Q8_K dots derive directly from the pinned Prism C/C++ contracts.
Decoded float results and packed integer dots have independent native oracles;
float GEMV is not a substitute for the packed-dot contract. Native-unavailable
CPU encoder entries stay unavailable. Reference/importance encoding is not
established by decoder or dot support.

Packed `ADD1` composes checked decoders and CPU encoders for 20 formats, with
F32 scalars and padded rows. `Q8_1` has no native decoder and `Q8_K` is absent
from native operation dispatch, so both reject. The fourth packed dimension is
restricted to one: pinned `ops.cpp` uses `i3*nb0` for its destination row offset.
No parity claim covers that defective domain. Two- and three-dimensional view
constructors derive unused-axis strides from the supplied row/plane strides.

`FLASH_ATTN_BACK` is unsupported: its public constructor unconditionally aborts
at pinned `ggml.c:5545`. The dormant CPU implementation also uses query-head
counts for key/value gradient batch strides at `ops.cpp:9516/9520`. Dormant
implementation text cannot qualify that public operation. Custom callbacks,
complete backward graphs and broader architecture/session APIs are outside
this supported subset.

Model retrieval enforces Content-Length agreement, complete chunk termination,
trailers, unambiguous framing and decoded-body bounds. Curl capture is bounded;
failed or oversized children terminate and are reaped. Invalid transfers cannot
publish a model blob or snapshot. Chunk framing retains a wire-size cap, while
optional chunk-extension syntax is not completely validated. Broader CLI and
download parity is not implied by those framing checks.

These are bounded source contracts, not full upstream/backend-matrix or
performance qualification. The accepted baseline and supported model admission
are independent. All outstanding runtime delivery and deferred work is tracked
in [PLAN.md](../PLAN.md#runtime-and-correctness).

## Rejected reuse

The following source choices do not supply an equivalent native CPU contract:

| Source | Reason |
| --- | --- |
| [InfiniTensor/gguf `98f1803`](https://github.com/InfiniTensor/gguf/tree/98f180387df864b9492eb4d262ed8d88c19d57de), MIT | IQ2 implementations are placeholders. |
| [Joshua `a0ec8a2`](https://github.com/rexlunae/joshua/tree/a0ec8a21c0816512f808e96824e45aa16b64a38b), MIT | IQ2 decoding uses a float dot that does not preserve native packed integer arithmetic. |
| [oxillama `98f17b8`](https://github.com/cool-japan/oxillama/tree/98f17b848daefc0dcbda4d3c64681df30a4a991b), Apache-2.0; [Turbospark `cd45a15`](https://github.com/whit3rabbit/turbospark/tree/cd45a1580143fd8a939abe10b4d60bb1cb7d9a92), MIT | IQ3 decoders with float GEMV do not provide the required native packed-dot contract. |
