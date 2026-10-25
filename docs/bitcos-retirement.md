# BITCOS retirement

BITCOS is retired; its implementation, tests and dedicated CI were removed.
Current-weight PTQ1 is the supported lossless packing option. Shared PQ2/PTQ1
utilities remain in [`bonsai-ternary`](https://github.com/symbiogenesis/prism-inference/blob/main/tools/ternary/README.md) for the exporter
and native Rust runtime.

The actual Swift [census](../config/swift.pq2-census.json) contains **32.76154%
zero symbols**, below BITCOS's **37.5%** ideal same-scale PTQ1 break-even before
offsets, padding or indexes. The proposed layout was estimated at
**6,158,339,606 bytes**, excluding new container/index overhead, versus the
verified [PTQ1 export](../config/swift.ptq1-export.json) at **5,946,648,928 bytes**:
BITCOS would be **211,690,678 bytes larger**. These are serialized sizes, not RAM
or throughput measurements.

The former optimized C++ prototype was **3.24× / 3.36× slower than PQ2** in hot
and larger-footprint single-thread synthetic cases on Cobalt 100 ARM64, MSVC
19.51.36260.0 with DOTPROD. [Run 39986261941](https://github.com/symbiogenesis/SwiftBonsai2/actions/runs/39986261941)
tested `49693a5`; its adapted PTQ/Q8_K comparator differed from Prism's released
PTQ/Q8_0 route. These historical kernel results do not qualify Rust, full-model,
multithreaded or Snapdragon behavior.

PTQ1 is smaller and has an existing native backend; BITCOS failed its prototype
speed condition. This decision does not establish a universal result for other
BITCOS implementations. PTQ's different activation quantization still requires
separate logits, quality and performance qualification. See the
[current assessment](swift-memory.md).

Prior sources and raw evidence remain in
[Git at `b59b3236`](https://github.com/symbiogenesis/SwiftBonsai2/tree/b59b3236d76e1f0c4a38943898286c64f43dff9a),
including the [layout census](https://github.com/symbiogenesis/SwiftBonsai2/blob/b59b3236d76e1f0c4a38943898286c64f43dff9a/config/swift.bitcos-census.json)
and [timing records](https://github.com/symbiogenesis/SwiftBonsai2/blob/b59b3236d76e1f0c4a38943898286c64f43dff9a/config/bitcos.synthetic-cobalt100.json).
