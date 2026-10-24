# Pinned native codec probe

`bonsai-codec-probe` checks production `bonsai-ternary` PQ2/PTQ1 conversion
against both native dequantizers from the released Windows ARM64 Prism runtime
`73bfe6cc485554822f80c473a3c0c4f25e80f41b`. It does not load model weights.

Run the wrapper, which first verifies the complete installed runtime package,
including DLL hashes, PE architecture and reported runtime version:

```powershell
./tests/Test-PrismCodec.ps1 -DataDirectory C:\BonsaiData
```

The wrapper resolves a source-built Rust probe by default. Use `-ProbeToolPath`
or `BONSAI_CODEC_PROBE_PATH` for a prebuilt executable, and `-ToolPath` for a
prebuilt `bonsai-tools` supervisor. `-EvidenceDirectory` optionally selects a
new external directory for retained stdout, stderr, result JSON and process
evidence. Its default is a unique directory beneath the system temp directory.

Only the supervisor should launch the native probe. It creates one owned
process in an external working directory, inherits the system environment,
and imposes a kill-on-close Windows Job, a 1 GiB commit limit and a 30-second
deadline. The internal executable protocol is:

```text
bonsai-codec-probe --runtime-directory ABSOLUTE_PINNED_DIRECTORY --result ABSOLUTE_JSON_PATH
```

The 6,670 synthetic blocks cover 853,760 FP32 values: dense symbols at twelve
positive/negative FP16 scales, every individual symbol position, every five-
and four-trit word in each native lane, and the original fixed seeded random
corpus. Every native result must match independently reviewed frozen
FP16-to-FP32 bit pairs and ternary arithmetic, including signed zero; the
oracle never calls production `half_to_float`. Conversion must preserve exact
source symbol and scale bytes. Separate `bonsai-ternary` regressions derive
expected decoded positions by integer base-three division for all 5,994 lane
words, and reject every noncanonical byte. They do not use round trips as the
expected-symbol oracle.

Unsafe code is confined to `src/native.rs`: absolute-path DLL loading with
dependency search restricted to that DLL directory and System32, export ABI
conversion, static type-traits reads and two bounded native decoder calls.
The reviewed ABI is 56-byte `ggml_type_traits`, `to_float` at byte 40, PQ2 type
142 (34 bytes) and PTQ1 type 143 (28 bytes), each with 128 values. The DLL stays
loaded until all calls return. The wrapper must verify the exact released
package before this boundary runs; the newer static overlay is not a substitute.

Passing proves synthetic format consumption and exact symbol/scale packing.
Activation quantization, model logits and performance remain unqualified.
