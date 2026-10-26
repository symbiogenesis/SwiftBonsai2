# Licensing and dependency terms

Original Swift Bonsai 2 - Runner code, documentation and configuration use
[Apache-2.0](../LICENSE), copyright 2026 Edward Miller and contributors.
Third-party material retains its own terms. [NOTICE](../NOTICE) preserves
Prism/ggml attribution and the MIT permission notice for inherited portions.

## Project license

Apache-2.0 permits commercial use, modification and redistribution, with an
express contributor patent grant and patent-litigation termination. That grant
covers qualifying contributor claims, not unrelated third-party patents.
See [license sections 2, 3 and 5](https://www.apache.org/licenses/LICENSE-2.0).
No current dependency requires the original application to adopt GPL, LGPL
or MPL; permissive dependencies remain subject to their own notices.

## Reviewed dependencies

The review covers workspace manifests, the shared lockfile, resolved package
metadata/cached license files, Prism notices and pinned artifact declarations.
Dependency terms apply independently of the application's license.

| Material | Declared terms | Distribution requirements |
| --- | --- | --- |
| `http`, `httparse`, `serde`, `serde_json`, `sha2`, `windows-sys`, `tempfile` | MIT OR Apache-2.0 | Retain the applicable license and attribution. |
| `tokio` 1.54.1 | MIT | Retain copyright and permission notice. |
| `fearless_simd` 1.1.0 | Apache-2.0 OR MIT | Optional native ARM64 Windows kernels; retain applicable license/attribution. |
| `subtle` 2.6.1 | BSD-3-Clause | Retain copyright, conditions and disclaimer; respect non-endorsement. |
| `zip` 9.1.0 | MIT | Retain the notice; enabled deflate dependencies also have permissive options. |
| `zlib-rs` | Zlib | Retain source notices and identify altered versions. |
| `unicode-ident` | (MIT OR Apache-2.0) AND Unicode-3.0 | Include Unicode-3.0 notices as well as the selected code license. |
| `r-efi` | MIT OR Apache-2.0 OR LGPL-2.1-or-later | A permissive alternative is available; LGPL is not required. |
| `cbindgen` 0.29.4 | MPL-2.0 | Optional `tools/prism` build tool, not linked into the application. Redistributing the tool or modified tool source requires its own MPL compliance. |
| `eframe`, `egui`, `epaint`, `egui-wgpu`, `egui-winit`, `egui_system_font_provider` 0.37.0 | MIT OR Apache-2.0 | Retain the selected code license and attribution; embedded font terms are separate. |
| `wgpu` and its core/HAL/type crates 30.0.1 | MIT OR Apache-2.0 | Retain the selected license and attribution. |
| `winit` 0.30.13, `accesskit_winit` 0.34.2 | Apache-2.0 | Retain the license and applicable notices; identify modified files. |
| `accesskit` 0.25.2, `accesskit_consumer` 0.39.2, `accesskit_windows` 0.35.2 | MIT OR Apache-2.0 | Retain the selected license and attribution. |
| `fontique` 0.11.1, `skrifa` 0.44.0, `read-fonts` 0.41.0, `font-types` 0.12.7 | MIT OR Apache-2.0 | Retain code licenses/notices independently of the fonts they load. |
| `harfrust` 0.12.0 | MIT | Retain the HarfBuzz developers' and Yevhenii Reizner's copyright and permission notice. |
| `arboard` 3.7.0 / `clipboard-win` 5.4.3 | MIT OR Apache-2.0 / BSL-1.0 | Retain applicable clipboard code licenses/notices. |
| `epaint_default_fonts` 0.37.0 | (MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0 | Preserve the individual font notices described below; the crate's code license does not replace them. |
| Prism/ggml codec, loader and native reference portions | MIT | Retain upstream MIT notices; Apache-2.0 does not replace them. |

The other reviewed code packages have permissive alternatives; bundled font
assets have separate terms. None lacked a declared license in the review.
cbindgen has no permissive alternative declared. Its use does not require our
source or generated declarations to adopt MPL. See its
[license](https://github.com/mozilla/cbindgen/blob/v0.29.4/LICENSE) and the
[MPL FAQ](https://www.mozilla.org/en-US/MPL/2.0/FAQ/).

### Native GUI fonts

The GUI enables `accesskit`, `clipboard`, `default_fonts`, `system_fonts` and
`wgpu`; inspection, persistence and browser-link features are disabled. The
downloaded 0.37.0 font crate embeds Hack Regular, Ubuntu Light and the small
`egui-icons.ttf` subset with this selection. Its optional monochrome Noto Emoji
and full emoji-icon fonts are not enabled. See the pinned
[font manifest](https://github.com/emilk/egui/blob/0.37.0/crates/epaint_default_fonts/Cargo.toml)
and [embedded assets](https://github.com/emilk/egui/blob/0.37.0/crates/epaint_default_fonts/src/lib.rs).

Binary distributions must retain accessible font copyright/license material
even though the fonts are compiled into the executable. Copy the applicable
files from the exact `epaint_default_fonts` package's `fonts/` directory:

- `Hack-Regular.txt`: Source Foundry MIT notice, DejaVu public-domain statement
  and Bitstream Vera license, including its font naming and standalone-sale
  restrictions.
- `UFL.txt`: Ubuntu Font Licence 1.0 for Ubuntu Light; also retain the font's
  metadata copyright, `Copyright 2011 Canonical Ltd.`, and follow its naming
  rules if modifying the font.
- `emoji-icon-font-mit-license.txt` and `egui-icons.txt`: John Slegers' MIT
  notice and the provenance of the always-embedded icon subset.
- `OFL.txt` and corresponding font copyrights if shipping the optional Noto
  Emoji asset, a source distribution containing it, or another OFL font. Keep
  OFL font software under OFL and respect reserved names and standalone-sale
  restrictions.

`system_fonts` uses `fontique` to locate installed fonts at runtime; the provider
memory-maps those local files rather than bundling them. This does not grant
permission to ship a user's or the operating system's fonts. Review each font's
own terms before adding it to a release. These font terms do not relicense the
original application code.

The [native Rust reference](https://github.com/symbiogenesis/prism-inference/blob/main/README.md) uses independently published
GGUF/ternary/Prism crates. Its graph/scalar numerical code follows pinned Prism
sources; notices are retained in
[PRISM-LICENSE.txt](https://github.com/symbiogenesis/prism-inference/blob/main/PRISM-LICENSE.txt) and root NOTICE.
Optional `fearless_simd` 1.1.0 is pinned to source revision
`b8a50a2c9ce03ee5384038ae8e6f45749662e2f6`, crate SHA-256
`3296eb97d869113eaf148a6bdf01884e90b3828ad7ae2340b0f93362fc158611`
matching Cargo.lock. Its default `std` feature has no external dependencies;
`libm`/the macro crate are unused, and it has no build script. The review covers
slice/array bounds, widening, bitcasts, baseline NEON and its internal unsafe
boundary; runtime source forbids unsafe code. See the
[immutable source](https://github.com/linebender/fearless_simd/tree/b8a50a2c9ce03ee5384038ae8e6f45749662e2f6/fearless_simd)
and [SIMD plan](simd-plan.md).

## External engines

| Artifact | Pinned source | Terms |
| --- | --- | --- |
| Reference llama.cpp | `36e36e63a54fc8f85b95e9c499a022b597e478dd` | [MIT](https://github.com/ggml-org/llama.cpp/blob/36e36e63a54fc8f85b95e9c499a022b597e478dd/LICENSE) |
| Swift assessment Prism | `73bfe6cc485554822f80c473a3c0c4f25e80f41b` | [MIT](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/LICENSE) |
| Prism overlay source | `b87ca7225456b06b0975b8239c66b5aecf07e37c` | [MIT](https://github.com/PrismML-Eng/llama.cpp/blob/b87ca7225456b06b0975b8239c66b5aecf07e37c/LICENSE) |

Retain each exact distribution's bundled licenses/notices. A top-level MIT
license does not replace terms for additional included components.

## Models and adapters

External weights/adapters retain their terms; the application license does not
relicense them.

| Artifact | Pinned revision | Published terms |
| --- | --- | --- |
| Swift Bonsai 2 GGUF | `a3bdac086bbb6b04d87d0108d18943b5f63868f7` | [Apache-2.0](https://huggingface.co/ukisai/Swift-Bonsai-2-GGUF/blob/a3bdac086bbb6b04d87d0108d18943b5f63868f7/LICENSE) |
| Qwen3.8-27B GGUF reference | `4ca720788d1e01f1bff70c033e0d0028fd02e502` | [Apache-2.0 declaration](https://huggingface.co/unsloth/Qwen3.8-27B-GGUF/blob/4ca720788d1e01f1bff70c033e0d0028fd02e502/README.md) |
| Approved OrcaBonsai LoRA | `703028bb7fba03496e796b144839b161cf0f5409` | [Apache-2.0 declaration](https://github.com/Continuum-AI-Corp/OrcaBonsai-27B-Uncensored/blob/703028bb7fba03496e796b144839b161cf0f5409/README.md#license) |

Swift's [NOTICE](https://huggingface.co/ukisai/Swift-Bonsai-2-GGUF/blob/a3bdac086bbb6b04d87d0108d18943b5f63868f7/NOTICE.txt)
attributes Bonsai to Prism ML and the Qwen base to Alibaba Cloud. Preserve it
and the model license when redistributing weights, including derived PTQ1;
identify changes to derived artifacts. Future models, accelerator SDKs and agent
tools require their own review if adopted.

## Redistribution

Include project LICENSE/NOTICE and every bundled dependency's applicable
copyright/license/notice material, including Unicode notices. Mark modified
Apache-licensed files. Include separate upstream notices/licenses when shipping
models/adapters. If shipping cbindgen, satisfy its MPL source-availability terms.

This review is not a complete installer notice bundle. Assemble that bundle
from the exact packages, runtime archive and artifacts shipped; repeat the
review when pins, enabled features or bundled assets change.
