# Production common-library process adapter

Build this separate CMake tree with `PRISM_SOURCE_DIR` naming the verified pinned
upstream root, and target `prism-upstream-common-bridge`. It statically links the
production `llama`, `llama-common` and `mtmd` libraries. Tests, tools, examples, server,
apps, TLS, optional external libraries and dependency downloads are disabled.
No upstream test source is compiled or included.

Run `prism-upstream-common-bridge --input ABSOLUTE_PATH --output ABSOLUTE_PATH`.
Input and output are bounded UTF-8 JSON files with this schema:

```json
{"SchemaVersion":1,"Operations":[
  {"Kind":"unicode-regex-split","Text":"text","Expressions":["expression"],"ByteEncoding":false},
  {"Kind":"mtp-first-decode-fits","Batch":32,"Catchup":31,"Sequences":1}
]}
```

Every operation is validated before any production API executes. Operation fields
are exact; unknown fields, duplicate object keys, unknown kinds and incorrect
types are rejected. `Batch`, `Catchup` and `Sequences` are signed 32-bit integers.
JSON nesting is limited to eight levels. Request files are at most 1 MiB, response
files at most 16 MiB, and operations number 1 through 128. Text is at most 65,536 UTF-8 bytes; there are 1 through 16
expressions, each at most 8,192 UTF-8 bytes. Paths must be absolute and distinct.

The adapter forwards inputs directly to `unicode_regex_split` and
`common_speculative_mtp_first_decode_fits`. Results preserve input order:

```json
{"SchemaVersion":1,"Results":[{"Pieces":["piece"]},{"Fits":true}]}
```

`Pieces` contains the production split result without further encoding changes;
the production function receives `ByteEncoding` exactly as supplied. `Fits` is
the production boolean result. The Rust suite owns all expected results and
assertions. A zero process exit means the operations completed and the response
was written. Errors return a nonzero exit and a message on stderr. Discard any
response from a failing process. The caller owns the timeout and file lifecycle.

## Generic Jinja operations

Jinja operations call the production lexer, parser, runtime and value APIs. A
template is at most 65,536 UTF-8 bytes. Variable-bearing operations accept exactly
one of `Variables` (an object) or `VariablesJson` (a string containing an object).
`VariablesJson` preserves object insertion order and numeric types such as `1.0`
across callers whose JSON DOM sorts keys or normalizes numbers. Variables are
limited to 256 KiB and 32 nesting levels, with duplicate object keys rejected.
Strings are marked as input when passed to the production runtime.

| Kind | Additional request fields | Successful result fields |
| --- | --- | --- |
| `jinja-render` | `Template`, variables, optional `ByteStrings` | `Rendered`: string |
| `jinja-hash` | `Chunks`: array of byte arrays | `Digest`: production 64-bit unsigned digest |
| `jinja-hash-trace` | `Actions`: array of `{"Update":[bytes]}` or `{"Digest":true}` | `Digests`: production 64-bit unsigned digests in action order |
| `jinja-stats` | `Template`, variables, `Paths`: array of paths | `Used`: booleans in path order |
| `jinja-caps` | `Template` | `SupportsStringContent`, `SupportsTypedContent`: booleans |
| `jinja-string-parts` | `Template`, variables | `Parts`: array of objects with `Value` string and `IsInput` boolean |
| `jinja-builtins` | none | `Builtins`: array of objects with `Type`, `Name` strings |

Hash chunks number at most 1,024 and contain at most 256 KiB of bytes combined.
Every byte must be an integer from 0 through 255. Builtin inventory is read from
the production global, int, float, string, array and object builtin maps, in that
group order and each map's iteration order.
Hash traces contain at most 2,048 actions and 256 KiB of update bytes combined;
they retain the same production hasher instance across all updates and digests,
including digest's mutation of buffered state.

A path is an array of 1 through 32 string keys or nonnegative integer indices,
beginning with a global variable name. String keys are at most 4,096 UTF-8 bytes
and indices at most 65,535. Stats accepts at most 1,024 paths. Optional render-only
`ByteStrings` contains at most 16 objects with exact fields `Path` and `Bytes`.
Each replaces an existing string at that path with at most 65,536 raw bytes after
the JSON variables have been parsed, permitting invalid UTF-8 inputs to production
code without placing invalid text on the JSON wire.

Production Jinja exceptions are observations returned per operation as
`{"ExceptionKind":"not-implemented"|"standard"|"unknown","Message":"..."}`;
they do not abort the batch. The caller decides which outcomes satisfy each case.
`not-implemented` identifies the production exception type only during runtime
execution and string gathering; the same type from setup, lexing or parsing is
reported as `standard`, preserving the caller's ability to follow the original
phase-specific exception contract.
Invalid protocol inputs still fail the process before execution. Rendered text
that cannot be encoded as JSON yields a standard encoding exception observation.

## Generic MTMD operations

`mtmd-image-should-tile` takes exact fields `Kind`, `PatchSize`, `Merge`,
`MinImageTokens`, `MaxImageTokens`, `Width`, `Height`. It forwards the populated
production hyperparameters and image size to
`mtmd_image_preprocessor_lfm2::should_tile`, returning `{"ShouldTile":boolean}`.
Patch size ranges from 1 through 64, merge from 1 through 8, token bounds from 1
through 4,096 in ascending order, and dimensions from 1 through 4,096.

`mtmd-group-bitmaps` takes exact fields `Kind`, `Merge`, `Parts`. Merge ranges
from 1 through 64 and parts contains at most 64 descriptors:

- `{"Kind":"text","Text":"..."}`: at most 65,536 UTF-8 bytes.
- `{"Kind":"bitmap","Width":100,"Height":100,"Mergeable":true}`: dimensions 1 through 4,096.
- `{"Kind":"audio","Samples":100,"Mergeable":true}`: sample count 1 through 1,048,576.

The adapter constructs production bitmap handles with null payload pointers,
sets the supplied mergeability flags, and calls `mtmd_group_mergeable_bitmaps`.
It returns `{"GroupSizes":[...],"RemainingBitmapParts":number}` from the actual
groups and mutated part list. Descriptor generation and interpretation of any
compact test specifications belong to the Rust suite.

## Generic grammar-parser operation

`grammar-parser-parse` takes exact fields `Kind` and `Grammar`. Grammar is a
nonempty UTF-8 string of at most 65,536 bytes without NUL characters, since the
production parser consumes a C string. A fresh `llama_grammar_parser` receives
each request. The result is `{"Parsed":boolean,"SymbolIds":[{"Name":"...",
"Id":number}],"Rules":[[{"Type":number,"Value":number}]]}`. Symbol IDs retain
the production map's name order, and rules retain the production vector order.
The result is limited to 4,096 symbols, 4,096 rules and 65,536 total rule elements.
The Rust caller owns every grammar input, expected symbol and expected rule.

## Generic reasoning-budget and UTF-8 operations

`utf8-is-complete` takes exact fields `Kind`, `Bytes` (0 through 65,536 integer
bytes) and returns `{"Complete":boolean}` directly from
`common_utf8_is_complete`. Raw bytes permit incomplete and invalid UTF-8 inputs.

`reasoning-budget-trace` takes exact fields `Kind`, `Actions`, with 1 through
4,096 actions. Every action has a `Kind` and a `Sampler` handle (0 through 31),
or `null` for native APIs that accept a null sampler. Handles are owned only for
that operation and freed on all exit paths. Handle lifetimes are checked before
any native action executes. Additional fields and production calls are:

| Action Kind | Additional fields | Trace event |
| --- | --- | --- |
| `init` | `Vocabulary`: null; `StartSequences`, `EndSequences`: token sequence arrays; `ForcedTokens`: token array; `Budget`: i32; `InitialState`: enum 0 through 4 | `{}` |
| `apply` | `Candidates`: objects with `Id` i32, `LogitBits` u32, `ProbabilityBits` u32; `Selected`: valid index or -1; `Sorted`: boolean | `Candidates`, `Selected`, `Sorted` after `llama_sampler_apply` |
| `accept` | `Token`: i32 | `{}` |
| `clone` | `Target`: unused handle | `{}` |
| `state` | none | `{"State":enum}` |
| `force` | none | `{"Forced":boolean}` |
| `end-match` | none | `{"EndMatch":null}` or `{"EndMatch":[tokens]}` |
| `reset` | none | `{}` |
| `free` | none | `{}` |

The response is `{"Trace":[events in action order]}`. Start and end sequence
collections contain at most 64 sequences, each of at most 1,024 tokens, and the
forced sequence at most 1,024 tokens. Token IDs and budgets are signed 32-bit
integers. Each apply has at most 4,096 candidates, with at most 65,536 candidates
combined across the trace. Floats use their raw IEEE binary32 bits, preserving
infinities and NaNs without JSON number conversions. The complete backing
candidate vector is returned. Every state transition, clone, reset, forcing
operation and end match comes from the production sampler API; the adapter has
no test sequence generation, finite-count calculation or forcing assertions.
