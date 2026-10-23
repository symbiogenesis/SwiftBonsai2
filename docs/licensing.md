# License decision and dependency review

Original Swift Bonsai 2 code, documentation, and configuration are licensed
under [Apache-2.0](../LICENSE), except where third-party terms apply. Copyright
2026 Edward Miller and contributors. The [NOTICE](../NOTICE) preserves the
Prism/ggml attribution and MIT permission notice for inherited portions.

## Why Apache-2.0

The dependency review found no requirement to license the application under
GPL, LGPL, MPL, or any one permissive license. MIT, BSD-2-Clause,
BSD-3-Clause, and Apache-2.0 are viable choices for our original contributions
while complying with the separate terms of included material.

Apache-2.0 permits commercial use, modification, redistribution, and proprietary
derivatives. Its explicit contributor patent grant and patent-litigation
termination provision make it a useful choice for a project developing native
inference and quantization code. The grant covers qualifying contributor patent
claims; it is not clearance for unrelated third-party patents. Its contribution
terms also establish an explicit default for intentionally submitted changes.
See [sections 2, 3, and 5 of the license](https://www.apache.org/licenses/LICENSE-2.0).

MIT and BSD are shorter and impose fewer redistribution steps, but their
standard texts lack an express patent grant. BSD-3-Clause adds a non-endorsement
condition. Apache-2.0's tradeoff is more notice handling, including marking
modified files; it is also incompatible with GPL-2.0-only combinations. This
project currently has no such dependency. See the [MIT text](https://opensource.org/license/mit),
[BSD-3-Clause text](https://opensource.org/license/bsd-3-clause), and
[Apache compatibility FAQ](https://www.apache.org/foundation/license-faq.html#GPL).

## Reviewed dependencies

Reviewed on October 23, 2026: the Cargo workspace manifests and shared lockfile, their
resolved dependency metadata and cached package license files, the existing
Prism notices, and the pinned runtime/model declarations. The Cargo graph
contains 62 unique external package/version pairs, including optional build and
platform-specific dependencies. Package licenses remain applicable regardless
of the license selected for our original code.

| Material | Declared terms | Effect on this project |
| --- | --- | --- |
| `http`, `httparse`, `serde`, `serde_json`, `sha2`, `windows-sys` | MIT OR Apache-2.0 | Permissive dependency terms; preserve the applicable license and attribution when distributing them. |
| `tokio` 1.54.1 | MIT | Retain its copyright and permission notice. |
| `subtle` 2.6.1 | BSD-3-Clause | Retain its copyright, conditions, and disclaimer in source and binary distributions; respect non-endorsement. |
| `zip` 9.0.5 | MIT | Its enabled deflate dependency graph also has permissive options. |
| `zlib-rs` | Zlib | Retain the notice in source distributions and identify altered source versions. |
| `unicode-ident` | (MIT OR Apache-2.0) AND Unicode-3.0 | The Unicode-3.0 notice is required in addition to the chosen code license. |
| `r-efi` | MIT OR Apache-2.0 OR LGPL-2.1-or-later | A permissive alternative is available; LGPL is not mandatory. |
| `cbindgen` 0.29.4 | MPL-2.0 | Optional build dependency of `tools/prism`, enabled by `engine-loader` to generate ABI declarations; not linked into the application. Using this tool does not require our source or generated declarations to adopt MPL. Redistributing cbindgen itself or modified cbindgen source requires its own MPL compliance. |
| Prism/ggml codec, loader and native reference portions | MIT | Preserve the upstream MIT notice; Apache-2.0 does not replace those terms. |

The remaining resolved packages have permissive license alternatives. No
external Cargo package was missing a declared license. `cbindgen` is the sole
dependency without a permissive option; its scope is the build tool. See its
[license](https://github.com/mozilla/cbindgen/blob/v0.29.4/LICENSE) and Mozilla's
[MPL FAQ](https://www.mozilla.org/en-US/MPL/2.0/FAQ/) for distribution obligations.

The native Rust reference reuses the existing local GGUF/ternary/Prism crates
without adding any external package/version pair to that reviewed set. Its graph
and scalar numerical code follow pinned Prism sources; the upstream MIT notice
is retained in `src/prism-runtime/PRISM-LICENSE.txt` and the root `NOTICE`.

The external engines are pinned separately:

- Reference llama.cpp: commit `36e36e63a54fc8f85b95e9c499a022b597e478dd`,
  [MIT license](https://github.com/ggml-org/llama.cpp/blob/36e36e63a54fc8f85b95e9c499a022b597e478dd/LICENSE).
- Swift Prism runtime: commit `73bfe6cc485554822f80c473a3c0c4f25e80f41b`,
  [MIT license](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/LICENSE).
- Prism overlay source: commit `b87ca7225456b06b0975b8239c66b5aecf07e37c`,
  [MIT license](https://github.com/PrismML-Eng/llama.cpp/blob/b87ca7225456b06b0975b8239c66b5aecf07e37c/LICENSE).

An engine distribution can include components under additional licenses.
Retain that exact distribution's bundled licenses and notices when shipping
it; the engine's top-level MIT license is not a replacement for those notices.

## Models and adapters

Weights and adapters are downloaded to external storage and retain their own
terms. Choosing the application's license does not relicense these artifacts.

| Artifact | Pinned revision | Published license |
| --- | --- | --- |
| Swift Bonsai 2 GGUF | `a3bdac086bbb6b04d87d0108d18943b5f63868f7` | [Apache-2.0](https://huggingface.co/ukisai/Swift-Bonsai-2-GGUF/blob/a3bdac086bbb6b04d87d0108d18943b5f63868f7/LICENSE) |
| Qwen3.8-27B GGUF reference | `4ca720788d1e01f1bff70c033e0d0028fd02e502` | [Apache-2.0 declaration](https://huggingface.co/unsloth/Qwen3.8-27B-GGUF/blob/4ca720788d1e01f1bff70c033e0d0028fd02e502/README.md) |
| Optional OrcaBonsai LoRA | `703028bb7fba03496e796b144839b161cf0f5409` | [Apache-2.0 declaration](https://github.com/Continuum-AI-Corp/OrcaBonsai-27B-Uncensored/blob/703028bb7fba03496e796b144839b161cf0f5409/README.md#license) |

Swift's [upstream notice](https://huggingface.co/ukisai/Swift-Bonsai-2-GGUF/blob/a3bdac086bbb6b04d87d0108d18943b5f63868f7/NOTICE.txt)
attributes Bonsai to Prism ML and the Qwen base to Alibaba Cloud. Preserve that
notice and the model's license if redistributing the weights, including a
derived PTQ1 repack. Identify changes to derived artifacts. Alternative models,
accelerator SDKs, and agent tools discussed as future options in the plan need
their own license review if adopted.

## Redistribution

Distribute our `LICENSE` and `NOTICE` with project releases. Retain each included
dependency's applicable copyright, license, and notice material, including
Unicode data notices. Mark modified Apache-licensed files. If shipping model
weights or adapters, include their separate upstream licenses and notices too.
If shipping cbindgen, satisfy its MPL source-availability requirements.

This review records the current source dependency choices; it is not a complete
notice bundle for a future binary installer. Assemble that bundle from the
exact packages, runtime archive, and model artifacts actually included, and
repeat the review when dependency pins, enabled features, or bundled assets
change.
