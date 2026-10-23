# BITCOS retirement

On October 23, 2026 the owner approved retiring the BITCOS experiment from
Swift Bonsai 2. Its implementation, tests and dedicated CI work were removed.
The shared PQ2/PTQ1 packing and numerical utilities remain as
[`bonsai-ternary`](../tools/ternary/README.md), supporting the GGUF exporter and
native Rust runtime.

The exact current Swift PQ2 census found **32.76154% zero symbols**, below the
**37.5%** ideal same-scale PTQ1 size break-even, before any extra offsets,
padding or indexes. The verified current-weight PTQ1 artifact is
**5,946,648,928 bytes**. The proposed BITCOS layout, with sign bytes rounded per
128-weight block and original scales/non-PQ bytes retained, was estimated at
**6,158,339,606 bytes**, excluding new indexes/container overhead:
**211,690,678 bytes larger** than PTQ1. These are serialized sizes, not measured
RAM or throughput. The [PQ2 census](../config/swift.pq2-census.json) and
[PTQ1 export receipt](../config/swift.ptq1-export.json) remain current evidence.

The optimized former C++ prototype was still **3.24× / 3.36× slower than PQ2**
in the hot/larger-footprint single-thread synthetic cases on Cobalt 100 ARM64,
MSVC 19.51.36260.0 with DOTPROD. The
[vector-rank comparison, run 39986261941](https://github.com/symbiogenesis/SwiftBonsai2/actions/runs/39986261941)
tested revision `49693a5`. The
[initial comparison, run 39985113979](https://github.com/symbiogenesis/SwiftBonsai2/actions/runs/39985113979)
tested `b41b466` and was slower still. These are historical C++ kernel timings;
they do not qualify Rust, full-model behavior, multithreading or Snapdragon
performance. The adapted PTQ/Q8_K comparator was distinct from Prism's released
PTQ/Q8_0 backend.

This is a project maintenance decision, not a universal disproof of BITCOS or a
claim that every future implementation must lose. Current same-weight PTQ1 is
smaller, an existing native PTQ backend is available, and the measured prototype
failed its speed condition. PTQ activation quantization differs from PQ2, so
weight parity and bounded native decode still do not establish equal logits,
creative quality or speed. See the [memory assessment](swift-memory.md).

The complete prior implementation and raw evidence remain in Git at
[`b59b3236d76e1f0c4a38943898286c64f43dff9a`](https://github.com/symbiogenesis/SwiftBonsai2/tree/b59b3236d76e1f0c4a38943898286c64f43dff9a),
including the
[original census with layout estimates](https://github.com/symbiogenesis/SwiftBonsai2/blob/b59b3236d76e1f0c4a38943898286c64f43dff9a/config/swift.bitcos-census.json),
[three-sample Cobalt 100 timing records](https://github.com/symbiogenesis/SwiftBonsai2/blob/b59b3236d76e1f0c4a38943898286c64f43dff9a/config/bitcos.synthetic-cobalt100.json)
and [standalone sources and reference fixture](https://github.com/symbiogenesis/SwiftBonsai2/tree/b59b3236d76e1f0c4a38943898286c64f43dff9a/tools/bitcos).
