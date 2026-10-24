# Upstream process adapter

`prism-upstream-bridge --input ABSOLUTE_PATH --output ABSOLUTE_PATH` calls the
production ggml library from the verified pinned Prism source. It does not compile
or include upstream test sources. The safe Rust caller supplies all inputs,
independent references, tolerances and pass/fail decisions. Exit zero means the
operations completed and the response was written; errors use a nonzero exit and
stderr. A response produced by a failing process must be discarded.

All integers and IEEE-754 binary32 floats are little-endian. Packed tensor bytes
use the pinned upstream ggml layout on a little-endian host. No implicit padding
is present in the protocol.

The request begins with the eight bytes `BSPREQ1\n`, then a `u32` request count.
Each request consists of six `u32` values: operation, K, M, N, packed byte count,
activation float count; followed by packed bytes and activation floats.

| Operation | Shape and payload | Returned floats |
| --- | --- | --- |
| 1: PQ2 matmul | K is a positive multiple of 128; M rows of K/128 34-byte PQ2 blocks; N columns of K f32 activations | M*N, column-major (`column*M + row`) |
| 2: PTQ1 decode | K=128, M is the block count, N=0; M 28-byte PTQ1 blocks; no activations | 128*M, block order |

The response begins with the eight bytes `BSPRES1\n`, then the matching `u32`
request count. Each response consists of a `u32` float count and that many f32
values, in request order. There is no other response data.

The adapter accepts 1 through 64 requests and at most 128 MiB of request data,
16 MiB packed bytes and 4,194,304 activation floats per request, 4,194,304 returned
floats per request and 16,777,216 returned floats overall. PQ2 dimensions are
bounded by K<=8192, M<=4096, N<=256 and K*M<=8,388,608. PTQ1 batches have at most
20,000 blocks. Unsupported operations, mismatched lengths, trailing data and
relative paths are rejected before execution. Each operation receives a separate
128 MiB ggml context and matmul uses four threads. Process timeout and scratch-file
lifecycle belong to the caller.
