`philox.h` is the unmodified test oracle from `ServeurpersoCom/qwentts.cpp`,
revision `5916bf7c79dbb4e66fc059cdc507a79a23a28bb4`, `src/philox.h`.
Its SHA-256 with LF line endings is
`41a955e4267eba8d1c1a4b55a0b8964c4a7bd28f380faae5f981152403c81de7`.
The MIT copyright and permission notice is retained in
[`LICENSE.qwen-core`](../../LICENSE.qwen-core).

The private production build replaces this entire algorithm with a declaration
of the Rust scalar export. The original stays here for exact source anchoring
and independent, model-free bit comparisons; it is never compiled into the
speech service. `philox_oracle.cpp` exercises only single draws, as both native
sampler callers do, avoiding the upstream array helper's signed addition domain.
