# Runtime conformance ownership

Runtime source/backend catalogs, codec parity, native overlays and their tests
are owned by [Prism Inference](https://github.com/symbiogenesis/prism-inference).
See the [complete conformance guide](https://github.com/symbiogenesis/prism-inference/blob/main/docs/prism-upstream-tests.md),
[main-only runtime workflow](https://github.com/symbiogenesis/prism-inference/blob/main/.github/workflows/runtime-conformance.yml)
and [focused diagnostics](https://github.com/symbiogenesis/prism-inference/blob/main/.github/workflows/prism-focused.yml).

Runner's [Foundation](../.github/workflows/foundation.yml) validates its app, broker,
process ownership, provisioning and released-runtime integration. It does not
build the upstream catalog or run runtime algorithm/codec suites. Full suites
and all benchmarks remain CI-only in their owning repository.
