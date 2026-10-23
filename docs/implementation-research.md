# Initial implementation research and checks

Work began October 22 and continued October 23, 2026. Each step was checked against primary sources before implementation. Later engine changes require their own literature search and sanity check.

| Step | Literature checked first | Sanity check and implementation consequence |
| --- | --- | --- |
| M0a model selection | EQ-Bench published data/methodology/samples; model cards; immutable APIs; pinned Qwen graph/CPU operations | Elo and raw-rubric ordering disagree; exact-pack/laptop evidence is missing. Qwen chosen with limited confidence, Gemma12 fallback. Native graph support and 12.89 GiB known subtotal provide a credible CPU route. See [decision](model-selection.md). |
| M0 host inventory | Microsoft [OSArchitecture](https://learn.microsoft.com/en-us/dotnet/api/system.runtime.interopservices.runtimeinformation.osarchitecture), [ProcessArchitecture](https://learn.microsoft.com/en-us/dotnet/api/system.runtime.interopservices.runtimeinformation.processarchitecture), [OS](https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/win32-operatingsystem), [computer](https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/win32-computersystem), [graphics](https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/win32-videocontroller) | Convert OS-usable KiB to bytes separately from installed bytes. Distinguish emulation from native execution. AdapterRAM is not the GPU shared-memory budget. Missing CIM facts remain unavailable. |
| M0 download/verification | Microsoft [streaming completion](https://learn.microsoft.com/en-us/dotnet/api/system.net.http.httpcompletionoption), [SendAsync](https://learn.microsoft.com/en-us/dotnet/api/system.net.http.httpclient.sendasync), [redirect handling](https://learn.microsoft.com/en-us/dotnet/api/system.net.http.httpclienthandler.allowautoredirect), [Get-FileHash](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.utility/get-filehash), [Hub downloads](https://huggingface.co/docs/hub/en/models-downloading) | Header timeout does not cover streamed body; pass a cancellation token to reads. Range can return 200; restart instead of append. Bound bytes, then verify hash before final-file publication. |
| M0 runtime install | Microsoft [ZipArchive](https://learn.microsoft.com/en-us/dotnet/api/system.io.compression.ziparchive), [PE format](https://learn.microsoft.com/en-us/windows/win32/debug/pe-format), publisher release API | Verify archive before extraction; reject traversal/duplicates/oversized expansion. Compare every installed file and native ARM64 PE headers. Check reported commit to catch an inconsistent source pin. |
| CPU source-build path | [Pinned build guide](https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/docs/build.md), [CPU CMake logic](https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/ggml/src/ggml-cpu/CMakeLists.txt), [VS18 CMake generator](https://cmake.org/cmake/help/latest/generator/Visual%20Studio%2018%202026.html), [Git for Windows long paths](https://gitforwindows.org/git-cannot-create-a-file-or-directory-with-a-long-path.html) | MSVC ARM64 rejects GGML_NATIVE=OFF, so require native ARM64 and let its ISA probe run. Disable automatic dependency/UI fetches and full embedded UI. An initial checkout hit long paths; command-local core.longpaths=true fixes this without global policy changes. Require clean source before compiling. |
| Foundation CI | GitHub [hosted runners](https://docs.github.com/en/actions/reference/runners/github-hosted-runners), [runner hardening](https://docs.github.com/en/actions/reference/security/secure-use#hardening-for-self-hosted-runners) | Hosted Windows ARM64 checks tooling/package, not laptop comfort or Adreno. Main-only, read-only token, pinned action, no weights/benchmarks. No personal-laptop runner setup. |
| Full GGUF inspection | Pinned [GGUF header](https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/ggml/include/gguf.h), [reader](https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/gguf-py/gguf/gguf_reader.py), [constants](https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/gguf-py/gguf/constants.py) | A quantization label is not a tensor histogram. Verify the complete artifact digest, bound header/count/string work, skip tensor payloads, validate descriptor extents, and separately hash tokenizer records and embedded template bytes. |
| Bounded CPU correctness check | Pinned [completion](https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/tools/completion/completion.cpp), [modern CLI launcher](https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/tools/cli/cli-server.h), [CPU traits](https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/ggml/src/ggml-cpu/ggml-cpu.c), [ARM kernels](https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/ggml/src/ggml-cpu/arch/arm/quants.c), Microsoft [memory query](https://learn.microsoft.com/en-us/windows/win32/api/sysinfoapi/nf-sysinfoapi-globalmemorystatusex) | Modern CLI starts a server; completion stays in-process but does not reliably apply thinking-control kwargs. Preserve the embedded default, cap output at 16 tokens, reject inherited model/backend overrides, and use admission plus a current-memory safety watchdog. Actual 14 tensor types all have CPU inference paths. No load/decode claim before successful execution. |
| Selected external storage | Microsoft [DirectorySecurity](https://learn.microsoft.com/en-us/dotnet/api/system.security.accesscontrol.directorysecurity), [SetAccessControl](https://learn.microsoft.com/en-us/dotnet/api/system.io.filesystemaclextensions.setaccesscontrol) | Apply protected owner/System/Administrators access only to the newly created owner-selected artifact root. This protects private storage against other standard users; it is not isolation from the owner, administrators, or model parsers. Preserve global profile/environment policy. |
| Device-path spelling repair (October 23) | Microsoft [Windows path normalization](https://learn.microsoft.com/en-us/dotnet/standard/io/file-path-formats) | A read-only reproduction accepted `//?/C:/Bonsai` and `//./C:/Bonsai` as device paths after normalization. Check slash-normalized namespace prefixes before filesystem probes in provisioning and Doctor. Focused Paths regressions passed; no namespace or filesystem object was created. |
| Runtime version-probe teardown (October 23) | Microsoft [WaitForExit overloads](https://learn.microsoft.com/en-us/dotnet/api/system.diagnostics.process.waitforexit) and [Kill](https://learn.microsoft.com/en-us/dotnet/api/system.diagnostics.process.kill) | Review found an indefinite wait after cancellation and unbounded redirected-output drains in the existing verifier. Bound exit, drain, and owned-process cleanup separately, with disposal in finally. Offline verification of the native package still passes its locked commit check. Fault injection was not performed; process-tree termination is not job-object containment or a descendant-exit verdict. |
| M1 nonlaunching server policy (October 23) | Pinned [argument definitions](https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/common/arg.cpp), [HTTP middleware](https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/tools/server/server-http.cpp), [routes](https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/tools/server/server.cpp), [request settings](https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/tools/server/server-context.cpp), and [security guidance](https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/SECURITY.md) | Two independent reviews support a fixed PowerShell preflight before an enforcing broker. CORS changes response headers without rejecting Host/Origin; health/OPTIONS bypass authentication, control routes remain, and request output limits override `n-predict`. No `--no-model`, `--no-props`, or `--no-metrics` flag exists. Reject override namespaces including `AIP_*` (port/routes) and `MTMD_*`; keep launch disabled, with explicit guard/process/UI gates. |
| M1 paths, arguments, and sampling (October 23) | Microsoft [drive classification](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getdrivetypew), [ArgumentList](https://learn.microsoft.com/en-us/dotnet/api/system.diagnostics.processstartinfo.argumentlist), [inherited environment](https://learn.microsoft.com/en-us/dotnet/api/system.diagnostics.processstartinfo.environment); Qwen [publisher sampler and thinking contract](https://huggingface.co/Qwen/Qwen3.8-27B) | Reject remote/unknown drive types before descendant probes. Validate exact profile types/fields rather than PowerShell coercion. Keep Windows environment intact; return separate argument tokens and probe only with an explicit external cwd. Preserve embedded thinking defaults with temperature 1, top-p .95, top-k 20, min-p 0, presence 0, repetition 1. These defaults and advertised switches do not establish effective request behavior. |
| Foundation preflight handoff (October 23) | GitHub [hosted ARM64 runners](https://docs.github.com/en/actions/reference/runners/github-hosted-runners), [workflow triggers](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/trigger-a-workflow) | Extend the existing main-only job with runtime-only offline re-verification and planned-switch checks after provisioning its small archive. Focused local runtime-only verification passed with `model-not-checked`. Full regressions stay hosted; no weights, model process, benchmark, or laptop-performance claim is added. |
| M1 Rust admission, before implementation (October 23) | [RFC 9110](https://www.rfc-editor.org/rfc/rfc9110.html), [RFC 9112](https://www.rfc-editor.org/rfc/rfc9112.html), [`http` HeaderMap multiplicity](https://docs.rs/http/1.4.0/http/header/struct.HeaderMap.html), [`subtle` equality](https://docs.rs/subtle/2.6.1/subtle/trait.ConstantTimeEq.html) | Independently reviewed the policy in an isolated detached worktree. Preserve duplicate headers; reject ambiguous framing, hostile Host/Origin and routes before accepting a bounded body. Compare bearer bytes without logging credentials. Use parsed HTTP types rather than inventing a wire parser; reception/allocation bounds remain the future adapter's responsibility. |
| M1 JSON policy, before implementation (October 23) | [Serde strict container attributes](https://serde.rs/container-attrs.html), pinned [request schema](https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/tools/server/server-schema.cpp), [chat parsing](https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/tools/server/server-common.cpp) | Decode directly into strict structs so duplicate/unknown keys and null/wrong types cannot erase policy. Expose one positive completion-cap spelling, force one completion/reference sampler/cache policy, and reject media/tools/template overrides. Final assistant messages can trigger prefill, so accept optional initial system plus alternating turns ending on user. Return value-free errors. |
| M1 exact context admission, before implementation (October 23) | Pinned [count endpoint](https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/tools/server/server-context.cpp#L5686), [endpoint contract](https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/tools/server/README.md#post-v1chatcompletionsinput_tokens-token-counting) | Review found an existing exact chat count path shared with generation. Pass identical canonical bytes through a trusted counting callback and retain them unchanged for generation. Require a positive count plus requested output plus conservative 64-token reserve within 8192, using checked arithmetic. No production backend transport/counter exists yet; synthetic counts test the policy, not model tokenization. |
| JSON shape repair, before implementation (October 23) | Pinned [Serde struct derive](https://github.com/serde-rs/serde/blob/v1.0.228/serde_derive/src/de/struct_.rs), [JSON deserializer](https://github.com/serde-rs/json/blob/v1.0.149/src/de.rs), [streaming map adapter](https://docs.rs/serde/1.0.228/serde/de/value/struct.MapAccessDeserializer.html) | Independent review found that derived structs also accept positional arrays and unit enums accept tagged objects. A focused adversarial regression reproduced acceptance before repair. Require map-only visitors for request/messages and string-only roles, delegating the original map directly so duplicate-key rejection remains intact. |
| Broker validation handoff, before implementation (October 23) | [Cargo locked/offline/focused tests](https://doc.rust-lang.org/cargo/commands/cargo-test.html), [ARM64 runner image](https://github.com/actions/runner-images/blob/main/images/windows/Windows11-VS2026-Arm64-Readme.md), [Rust 1.98 release](https://blog.rust-lang.org/2026/08/20/Rust-1.98.0/) | Existing cached native toolchain permits offline compilation with exact direct dependencies and a checksum-bearing lockfile. Use focused local regressions and analyzer checks; add native Rust 1.98.0 formatting/Clippy/full tests to main-only hosted Foundation. Keep builds/native working directories in unique system temp. No benchmark or model process is involved. |

## M1 bounded HTTP reception: pre-implementation review, October 23

Literature search: [RFC 9112 framing and connection handling](https://www.rfc-editor.org/rfc/rfc9112.html), [Hyper 1.8.1 request parsing](https://github.com/hyperium/hyper/blob/v1.8.1/src/proto/h1/role.rs), [httparse 1.10.1 source](https://github.com/seanmonstar/httparse/blob/v1.10.1/src/lib.rs) and [configuration](https://docs.rs/httparse/1.10.1/httparse/struct.ParserConfig.html), [http 1.4.0 HeaderMap](https://docs.rs/http/1.4.0/http/header/struct.HeaderMap.html), and Tokio 1.50.0 [absolute timeouts](https://docs.rs/tokio/1.50.0/tokio/time/fn.timeout_at.html), [bounded reads](https://docs.rs/tokio/1.50.0/tokio/io/trait.AsyncReadExt.html), and [in-memory duplex streams](https://docs.rs/tokio/1.50.0/tokio/io/fn.duplex.html).

Before coding, the integrator and an independent reviewer in a dedicated detached worktree checked the boundary against the existing request policy. Hyper's inspected version discards duplicate identical Content-Length fields; the policy cannot then reject wire multiplicity. Use httparse's syntax parser with an explicit CRLF-only envelope and no leading empty lines, append every header value, and call the existing head policy before allocating the body. httparse alone permits bare LF and leading blank lines; its default configuration is insufficient for this deliberately narrow wire contract. Continued review also found [http 1.4.0 fragment normalization](https://github.com/hyperium/http/blob/v1.4.0/src/uri/path.rs#L117-L119): validate the exact raw request-target before constructing a Uri, so `/v1/chat/completions#suffix` cannot become the permitted route.

The receiver will own one supplied stream, bound the complete raw head to 16 KiB including request line and whitespace, retain at most 64 headers, and collect only the admitted Content-Length up to 1 MiB. Read-ahead is limited to the fixed head buffer; excess buffered bytes fail closed. Cap header reception at five seconds and the complete intake at thirty seconds, both shortened by the supervisor's absolute deadline; never reset a timer per read. Check expiry explicitly because Tokio permits immediately ready futures to complete after a timeout's deadline. Drop the owned stream on errors/cancellation, spawn no tasks, and require one response followed by close on success. There is no listener, backend connection, or launcher in this increment. Exact model counting, live responses/cancellation and process containment remain separate gates.

The post-implementation review found no blocking defect and recommended a [`'static` bound](https://doc.rust-lang.org/rust-by-example/scope/lifetime/static_lifetime.html#trait-bound) to discourage borrowed-stream callers from defeating the drop contract. Applied that refinement while retaining the explicit requirement that the supplied stream closes on Drop; the lifetime bound alone cannot prove a wrapper's ownership/close semantics. Tests use Tokio's paused time and in-memory streams, not model execution, network listeners or performance workloads.

## M1 bounded HTTP responses: pre-implementation review, October 23

Before implementation, the integrator and an independent reviewer in an isolated
detached worktree checked [RFC 9112 framing](https://www.rfc-editor.org/rfc/rfc9112.html),
Tokio 1.50.0 [absolute timeouts](https://docs.rs/tokio/1.50.0/tokio/time/fn.timeout_at.html)
and [write/flush/shutdown semantics](https://docs.rs/tokio/1.50.0/tokio/io/trait.AsyncWriteExt.html),
and the pinned server's [HTTP response provider](https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/tools/server/server-http.cpp#L587-L623)
and [SSE completion/error paths](https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/tools/server/server-context.cpp#L4548-L4675).

Sanity check: raw backend socket EOF cannot prove a complete HTTP body, and
close-delimited downstream SSE would hide truncation. Accept only an owned,
already HTTP-decoded backend response whose trusted transport reports truncated
framing as an I/O error. Validate status and the admitted request's expected media
type before writing. Rebuild fixed headers; never copy upstream headers. Buffer
at most 4 MiB for a JSON object, or stream SSE in 8 KiB chunks under the same byte
cap. Only clean decoded EOF permits a downstream terminating zero chunk. Preserve
SSE bytes, including error events and pings; transport completion is not a
generation-success verdict and must not synthesize `[DONE]`.

Use the supervisor's absolute deadline for backend reads and shorten each entire
client write/flush operation to five seconds, without resetting for partial
writes. Check expiry before and after ready operations. Own both endpoints and
drop them on error or cancellation; do not flush/shutdown on error or retry a
partially written response. A write-only client cannot detect disconnect while
awaiting backend data: real disconnect monitoring and generation cancellation
remain supervisor/transport integration work. This step adds no listener, HTTP
client, process launch or model execution.

The independent post-implementation review found no blocking defect. It clarified
that a final flush/shutdown failure can occur after the terminating zero chunk
has reached the client; an error return is not proof that no complete HTTP body
was transmitted. The contract documents that commitment point. The JSON check
uses a map-only visitor and explicit UTF-8 validation without constructing a
response value tree. The isolated test implementation was reviewed separately;
paused-time fixtures exercise ownership, truncation and partial I/O without
opening sockets or running performance workloads.

## M1 backend transport: pre-implementation review, October 23

Before implementation, the integrator and an independent reviewer in a dedicated
detached worktree checked [RFC 9112 framing](https://www.rfc-editor.org/rfc/rfc9112.html),
[`httparse` 1.10.1 response parsing](https://docs.rs/httparse/1.10.1/httparse/struct.Response.html),
Tokio 1.50.0 [`AsyncRead`](https://docs.rs/tokio/1.50.0/tokio/io/trait.AsyncRead.html)
and [absolute timeouts](https://docs.rs/tokio/1.50.0/tokio/time/fn.timeout_at.html),
and the pinned server's [response provider](https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/tools/server/server-http.cpp#L587-L623)
and [HTTP implementation](https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/vendor/cpp-httplib/httplib.cpp).

Sanity check: a raw socket cannot satisfy the existing response writer's decoded
EOF contract. Add an owned supplied-stream exchange and framing decoder, without
creating sockets or tasks. Rebuild private numeric Host, bearer credential,
Content-Length and fixed routes; never accept client headers or arbitrary URLs.
Require a bounded strict HTTP/1.1 200 head with exactly one Content-Length or
exact chunked encoding. Reject encodings, trailers and upgrades. The pinned
provider emits bare hexadecimal chunks without extensions/trailers; this narrow
adapter may reject those general HTTP features. Bound decoded payload to 4 MiB,
chunk lines to 32 bytes and cumulative chunk metadata to 64 KiB. Incomplete
framing must remain an error after yielded payload, never a clean EOF. One
absolute supervisor deadline covers response head/body; the complete request
write/flush is also shortened to five seconds. Bound work per decoder poll.

For the next counting step, separately checked the pinned [count handler](https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/tools/server/server-context.cpp#L5686)
and [endpoint contract](https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/tools/server/README.md#post-v1chatcompletionsinput_tokens-token-counting).
The response requires both a positive integer `input_tokens` and
`object: "response.input_tokens"`. Limit it to 4 KiB and reject duplicate,
unknown or malformed fields before admission. Count and generation must send the
same immutable canonical bytes. Supplied streams alone cannot attest backend
identity: the future supervisor must own the immutable backend and prevent
bypass or replacement between operations. This increment cannot qualify launch,
real disconnect cancellation or isolation.

The post-implementation review found that a derived Serde unit enum for the
object tag also accepted an externally tagged JSON map. Require a `String`
and exact equality instead; the regression includes that alternate map shape.
Independent code and test review found no further blocking defect. The decoder
closes its stream immediately on an error, keeps errors sticky, and requires
the complete chunk terminator before exposing framed EOF. Tests include an
end-to-end decoder/response-writer truncation case.

## Build and allocation improvements (October 23)

Before this pass, checked Microsoft's [shared MSBuild compiler scheduling](https://devblogs.microsoft.com/cppblog/improved-parallelism-in-msbuild/)
and the pinned engine's [MultiToolTask configuration](https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/CMakeLists.txt).
CMake project parallelism alone does not cap compiler workers. Apply the same
bounded automatic budget to project and shared compiler scheduling, retaining
explicit overrides and unconditional configuration/integrity checks. The budget
is an engineering starting point, not a measured optimum.

Checked Cargo's [CI download-cache guidance](https://doc.rust-lang.org/cargo/guide/cargo-home.html#caching-the-cargo-home-in-ci),
[build artifacts](https://doc.rust-lang.org/cargo/reference/build-cache.html) and
the pinned [GitHub cache action](https://github.com/actions/cache/tree/caa296126883cff596d87d8935842f9db880ef25).
Cache registry downloads/index, Git dependency objects and the external target
directory, excluding credentials and duplicate extracted registry sources. Bind
reuse to runner image, architecture, compiler and dependency/build-recipe identity;
allow Cargo to decide which restored artifacts remain fresh. Always execute
formatting, analyzers and correctness checks, including on an exact cache hit.

The October 23 CI scheduling review used the [last successful Foundation run](https://github.com/symbiogenesis/SwiftBonsai2/actions/runs/39994714096):
Prism's source/build step took 307 seconds; standards and the remaining Foundation
checks ran serially. Following the [ccache directory guidance](https://ccache.dev/manual/latest.html#_compiling_in_different_directories),
cache compiler objects with paths normalized relative to each fresh build lane,
compiler-content checking and no relaxed correctness options. Reverify the pinned
source ZIP and run the native regression on every invocation. Use at most four
compiler workers in hosted Prism CI, retaining two as the script default.
Run standards separately from the complete correctness lane, cache the pinned
PowerShell analyzer, and isolate manual dispatch concurrency from superseded
pushes. These changes preserve every gate. The [populated-cache Prism job on
45f81dc](https://github.com/symbiogenesis/SwiftBonsai2/actions/runs/39997978790/job/120566391977)
reported 341/341 compiler cache hits, passed the native regression and completed
in 46 seconds, compared with 311 seconds for the baseline job. These are observed
hosted CI timings, without an inference-performance claim. Validation also
exposed a marker-publication race in the lifecycle test: wait for the writer to
release its marker handle within the existing deadline before reading it.

Checked Rust's [Vec allocation contract](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.with_capacity)
and Tokio's [complete writes](https://docs.rs/tokio/1.50.0/tokio/io/trait.AsyncWriteExt.html#method.write_all).
Small received bodies can occupy the unused head-buffer tail; larger bodies keep
the separate bounded allocation. Read SSE payload directly into a reusable wire
frame to remove per-chunk heap formatting and separate framing writes. Start
buffered JSON capacity at 8 KiB and grow within the existing cap. Preserve all
absolute deadlines, cap-plus-one probes, flushes and terminal-framing rules.

For GGUF metadata, preserve the pinned format's [boolean and array types](https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/ggml/include/gguf.h)
and [.NET bounded stream reads](https://learn.microsoft.com/en-us/dotnet/api/system.io.stream.read).
Validate every discarded boolean in chunks using the existing scratch buffer,
and omit unreported nested descriptors. Depth/element/byte limits, strict values,
artifact verification and raw tokenizer digests retain their original meaning.
Focused regressions cover buffer/hex/chunk boundaries and malformed arrays;
full suites remain in CI and no local benchmark is performed.

Integrated validation passed on native ARM64: Release broker build, Release
Clippy across all targets with warnings denied, formatting, the request-buffer
boundary case, fragmented SSE across hexadecimal-size boundaries, exact 4 MiB
and cap-plus-one JSON/SSE responses, and one deadline across partial writes.
PowerShell parsing, the focused scheduling case and GGUF Arrays case passed.
A tiny external two-target CMake/MSBuild compilation accepted the generated
scheduling arguments; its temporary sources were removed and logs retained.
This confirms build-path compatibility, not a full engine build or a speedup.

## Development orchestration and numeric loopback integration (October 23)

Before implementation, reviewed Tokio 1.50's [fair semaphore and close contract](https://docs.rs/tokio/1.50.0/tokio/sync/struct.Semaphore.html),
[Notify waiter registration](https://docs.rs/tokio/1.50.0/tokio/sync/struct.Notify.html),
[absolute timeout behavior](https://docs.rs/tokio/1.50.0/tokio/time/fn.timeout_at.html),
[stream splitting](https://docs.rs/tokio/1.50.0/tokio/io/fn.split.html),
[cancel-safe reads and EOF](https://docs.rs/tokio/1.50.0/tokio/io/trait.AsyncReadExt.html#method.read),
[TCP connections](https://docs.rs/tokio/1.50.0/tokio/net/struct.TcpStream.html)
and [listener ownership](https://docs.rs/tokio/1.50.0/tokio/net/struct.TcpListener.html).
Immediately ready operations can finish after a Tokio timeout, so explicit
expiry checks accompany all new boundaries. A semaphore limits total reserved
connections before accept and one active operation; fair waiting starts when
the acquisition future is polled. Notify registration precedes the durable
closed-state check to avoid losing a shutdown wakeup.

The supervisor joins the existing adapters with the same canonical request,
private endpoint/policy and absolute deadline. A one-byte read races backend
work using safe pinned [poll_fn](https://doc.rust-lang.org/std/future/fn.poll_fn.html).
EOF, read failure or extra input cancels owned I/O. Treat write-half-close as
abandonment explicitly; TCP cannot distinguish it from full departure using
EOF alone. Numeric IPv4 addresses avoid DNS and alternate-address fallback.
No executable, model process, key generation, UI or automatic startup is added.

Independent research checked Microsoft's [SO_REUSEADDR/SO_EXCLUSIVEADDRUSE security discussion](https://learn.microsoft.com/en-us/windows/win32/winsock/using-so-reuseaddr-and-so-exclusiveaddruse)
and the pinned runtime's [socket defaults](https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/vendor/cpp-httplib/httplib.cpp)
and [server cancellation path](https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/tools/server/server-context.cpp).
Fixed loopback metadata and a bearer credential do not authenticate backend
process identity or prevent replacement/bypass. Socket closure alone does not
acknowledge model-slot cancellation. Therefore interrupted generation closes
the entire development service gate before releasing its active permit,
including when the caller drops the future. No automatic reset/restart is
provided. A future launcher must qualify exclusive ownership or an authenticated
channel, restricted process policy and backend termination/cancellation before
daily launch. Existing launch-disabled gates are retained.

Release build, all-target/all-feature Release Clippy, formatting and rustdoc
pass on native ARM64 with locked offline dependencies. Focused supplied-stream
tests cover canonical flow, rejected counts, EOF/surplus cancellation, absolute
deadlines, FIFO, queue expiry, guard cancellation and shutdown. Selected real
loopback checks cover identical count/generation bodies and private headers,
disconnect, external caller cancellation and active shutdown. Independent review
found no blocking defect and independently passed the caller-cancellation case.
The full broker suite, including the new socket cases, remains in the main-only
Foundation CI workflow. No model execution, benchmark, full local suite, driver
change or process-isolation verdict is implied.

## Development runner and private credentials (October 23)

Before implementation, reviewed Tokio 1.50's [Ctrl-C registration and error contract](https://docs.rs/tokio/1.50.0/tokio/signal/fn.ctrl_c.html)
and [current-thread runtime builder](https://docs.rs/tokio/1.50.0/tokio/runtime/struct.Builder.html).
The executable requires explicit development and unverified-key-file opt-ins;
it does not supply a model launcher or imply immutable backend identity. Poll a
fixed-capacity collection of owned request futures, with shutdown checked before
acceptance and no detached request tasks. Close admission and drain cancellation
on Ctrl-C, signal failure, accept failure or the generation-cancellation latch.
Use the service's validated capacity rather than a separately configured bound.
No automatic reset, retry or backend restart follows unacknowledged generation.

For credentials, checked .NET's [cryptographic random-byte API](https://learn.microsoft.com/en-us/dotnet/api/system.security.cryptography.randomnumbergenerator.getbytes?view=net-10.0),
[creation-time file/directory ACL overloads](https://learn.microsoft.com/en-us/dotnet/api/system.io.filesystemaclextensions.create?view=net-10.0)
and [ACL inheritance protection](https://learn.microsoft.com/en-us/dotnet/api/system.security.accesscontrol.objectsecurity.setaccessruleprotection?view=net-10.0).
Generate distinct 32-byte keys only after creating and checking a new private
directory; apply protected owner/LocalSystem ACLs when creating each file with
CreateNew. Never return or log key values. Existing owner-controlled local
parents are required, and the script rejects Git/reparse/device/ADS paths through
the shared path validator. Same-user processes, privileged access and parent
replacement remain outside this credential helper's assurance. The development
CLI retains file handles with read sharing only but explicitly does not verify
ACLs or ancestor ownership. This does not close the backend bypass gate.

The focused key-creation regression passed on Windows ARM64, checking protected
ACLs/ownership, exact distinct encodings, path-only output, repeated setup without
overwrite and unchanged environment. Independent review found no blocking issue
and passed the unsafe-path/WhatIf case in its own isolated lane. PowerShell and
strict C# standards passed. The native SDK needed its normal user NuGet settings
readable for that validation; no inherited OS/profile variables were redirected.
A focused real-loopback regression also confirms an unread queued socket expires
at thirty seconds without connecting to the synthetic backend.

The integrated runner passes a native ARM64 Release build, formatting, all-target
Release Clippy with warnings denied, rustdoc and the focused outer-runner
cancellation regression. Its executable help probe passes. Independent review
passed the exact-capacity/shutdown and runner-cancellation cases. The new
regressions distinguish normal delivery, generation-latch exit, notification
failure, shutdown/drain and strict bounded key-file configuration. Windows may
reset a queued socket containing unread input during cancellation; the fixture
accepts reset or EOF as closed. Full broker tests remain hosted-only.

## Owned Windows process lifecycle (October 23)

Before implementation, reviewed Microsoft's [process-creation attributes](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute),
[Job limits](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_limit_information)
and [job-wide committed-memory contract](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_extended_limit_information).
Assign the noninheritable Job through `PROC_THREAD_ATTRIBUTE_JOB_LIST` during
CreateProcessW, with a suspended child and explicit inherited standard-handle
list. This avoids a child-execution or abrupt-parent-exit gap before Job
assignment. Configure kill-on-close, aggregate committed-memory and active
process limits without breakaway flags. Resume only after owning the process
handle; setup failure closes the Job, including a still-suspended child.

`Bonsai.Process.cs` supplies this primitive and the CPU check uses an 18 GiB
commit limit and one active process. Explicit stop terminates the Job and waits
for zero active processes, while final handle closure also covers abrupt owner
death. Native argv quoting, an absolute external working directory and a null
environment block preserve the inherited Windows/profile environment. Fixed
synthetic stdout/stderr evidence goes directly to external files. The CPU check
also rejects AIP/MTMD overrides and retains its 22 GiB admission requirement,
one-second current-memory watchdog and ten-minute deadline. This is lifetime
and committed-memory enforcement, not a restricted token, file, egress or
resident-physical-memory boundary.

Focused synthetic checks passed for explicit stop, disposal and abrupt owner
death with descendant cleanup; process-count denial; single-process and
cross-descendant aggregate native commit denial; argv quoting/Unicode; absolute
cwd; and unchanged required environment. The allocation-denial result is the
limit assertion: Windows' reported Job peak may include a denied request and
is retained only as a diagnostic. Integrated PowerShell/strict Release C# checks
and the focused Lifetime case passed on main. All lifecycle cases are wired
into hosted Foundation CI. No model inference, performance workload or full
local suite was run. The post-WSL admission check reported 21,596,512,256 bytes
available against 23,622,320,128 required, with no model process started.

## Owner-prioritized Swift and actual PQ2 census (October 23)

The owner explicitly chose Swift Bonsai 2 for the memory work after the Qwen
baseline's conservative 22 GiB admission remained above free RAM. The separate
[assessment](swift-memory.md) retains the old reference and records the new
priority without presenting a local candidate benchmark. Public model metadata
pins Swift revision `a3bdac086bbb6b04d87d0108d18943b5f63868f7` and the current
PQ2 artifact's exact 7,206,168,928 bytes/SHA-256. Prism's released native Windows
ARM64 CPU package pins `73bfe6cc485554822f80c473a3c0c4f25e80f41b`; offline
verification confirms package contents, architecture and reported commit.

Source inspection confirms packed PQ2 ARM NEON dots and shared Hadamard/sign
construction. It also finds that Prism's ARM source route requires Clang; the
earlier upstream MSVC builder must not silently build this fork. Completion
supports the selected mmap/no-repack switches but does not accept server-only
checkpoint/prompt-cache/speculation flags. Source arithmetic supports an explicit
12 GiB service allowance with about 4.64 GiB remaining after known persistent
weights/caches/transforms, while retaining the 4 GiB physical reserve. A fixed
policy regression preserves both this assessment and the original Qwen default.

Full size/SHA verification and a streaming census of the actual Swift file found
26,869,760,000 PQ2 codes across 402 tensors: 8,802,947,576 zeros (32.76154%), no
fourth code and no zero/nonfinite scales. The exact source census is retained in
[the portable record](../config/swift.pq2-census.json). The current-weight PTQ1
export below preserves source symbols and scales.
Default stock GGUF interpretation remains unchanged; private type IDs require an
explicit codec-pinned Prism dialect.

Strict integrated PowerShell/C# analyzers and focused block/profile checks pass.
Independent review also passes provisioning, census and policy regressions.
The 16-token Swift CPU check passes memory admission, reaches initialization,
then exits with native fast-fail status `0xc0000409` before output. No watchdog
failure was reported. A version-only owned-child probe succeeds, as does a
separate model-free native backend/system-info probe. The precise failing call
is unlocalized; asynchronous logs are not a call stack. No decode correctness,
runtime RAM/speed result, full local suite or performance benchmark is claimed.

## Evidence obtained

### Additional Swift implementation evidence (October 23)

An owned debugger localized the earlier fast-fail to an uncaught template
exception: completion submitted a system-only list before the user query. The
Swift-only assessment omits that preamble and now completes its fixed 16-token
decode. The Qwen reference and embedded template are unchanged.

The bounded current-weight PTQ exporter converted all 209,920,000 PQ2 blocks,
verifying byte-identical reconstruction after every batch. The final file is
5,946,648,928 bytes (1.173 GiB smaller); its identity is in the
[portable receipt](../config/swift.ptq1-export.json). A separate owned model-free
probe confirms native PQ2/PTQ dequantizer FP32 bit parity on 853,760 values.
The actual derived model completes bounded native decode alone and with the
approved, pinned pretrained adapter. PTQ activation rounding differs from PQ2;
neither this result nor weight parity qualifies logits, creative quality or speed.

The [adapter assessment](swift-lora.md) records immutable provisioned identity,
129 matching target pairs, bounded loading, extra Job-commit diagnostics and an
unresolved authoring-basis question. It also quantifies exact B deduplication
at 2.5 MiB and reviews initial graph reservation and rank-1 fusion as future
engine changes. No training, weight merge, precision reduction or adapter
modification is performed.

The alternate BITCOS experiment was removed after its size and hosted synthetic
speed results failed to justify continued maintenance. The concise
[retirement record](bitcos-retirement.md) retains the decision, qualifications
and links to historical raw evidence. Shared safe Rust PQ2/PTQ1 packing and
numerical utilities remain in `tools/ternary` for the exporter and native Rust
runtime. The pinned Prism engine/LoRA overlay remain separate native replacement
work; Rust SIMD, throughput and end-to-end runtime qualification remain pending.

The first full export regression handoff exposed a PowerShell type collision
between two tests sharing a process. Separate CI steps now load those assemblies
independently. All full suites remain hosted; local work uses strict builds and
focused correctness. No laptop benchmark, daily-service or accepted performance
verdict is implied.

- Host inventory: native ARM64 Windows/PowerShell; **33,897,037,824 usable bytes** (31.57 GiB); Snapdragon X Elite X1E78100; Adreno X1-85 driver **31.0.128.0**; native CMake **4.3.1-msvc1** and MSVC **19.51.36260.0**. Presence does not qualify inference or GPU execution.
- Focused synthetic download correctness checks passed locally. The pinned native runtime was downloaded to session temp, size/hash-checked, extracted and verified. Its bounded version-only process reported **0.7.0-dev, build 11669, commit 620d2225c**, built for Windows arm64. It loaded no model.
- On October 23 the selected model and runtime were provisioned under the owner-selected external directory. The **13,146,393,504-byte** model passed its independently recorded publisher LFS SHA-256. Full inspection confirmed **866 tensors, 14 tensor types, 15 MTP tensors**, all source-only dimension expectations, and embedded-template SHA-256 `12827f24b742ea4e80cdc12dbcf9622227056b9f797252a3149263d4f9aaadce`. The [portable metadata record](../config/model.metadata.json) omits the local path and vocabulary contents.
- The CPU check's native Windows memory query found **8,412,327,936 available bytes** at its recorded admission check. It required **23,622,320,128 bytes** (18 GiB service allowance plus a conservative 4 GiB reserve), so it returned `not-run-insufficient-available-memory` before starting a model process. No user applications were closed. This does not show that the selected model exceeds its memory allowance; actual load/decode and peak residency remain untested.
- Full-suite results belong to hosted foundation CI on settled main. Focused synthetic GGUF parser checks passed, including twelve malformed fixtures. Model loading, streaming/cancellation/security, and creative/comfort/performance gates remain pending.
- M1 focused ServerPolicy regressions pass: no default process/storage creation, strict profile types/fields, sensitive override rejection without value disclosure, unchanged parent environment, safe local paths, and exact help-switch matching. The offline artifact preflight passed existing model/runtime integrity and advertised-switch checks. It reported **9,938,030,592 available bytes** against **23,622,320,128 required**, with no model process started. No HTTP behavior, option-value enforcement, daily isolation, or creative-output verdict is implied.
- The Rust request-admission core builds in Release on native ARM64 with locked cached dependencies; formatting and Clippy pass with warnings denied. Five focused local regressions cover duplicate-header rejection, duplicate-key/type rejection, canonical-body preservation, context boundary/overflow admission, and the reviewed JSON-shape repair. The count tests use synthetic callbacks; they do not qualify real model tokenization. Independent review found and prompted repair of Serde's alternate struct/enum input shapes. The full broker suite is handed to hosted Foundation CI; live HTTP, real template/token counts, cancellation, backend bypass prevention and restricted process policy remain unqualified. See [policy contract](broker-policy.md).

- The M1 one-request receiver builds on native ARM64 with locked cached dependencies. Formatting and Release Clippy with warnings denied pass. Six selected local regressions pass: wire duplicates/framing, raw routes, stalled absolute deadlines, fragmented valid requests, cancellation/drop, and raw head byte/count boundaries. Independent review found no blocking defect. Thirteen receiver regressions are committed for the existing hosted full broker suite; tests not selected locally await that handoff. These are synthetic reception checks, not live HTTP responses, real token counting, generation cancellation or isolation qualification.

- The M1 response writer builds in Release on native ARM64 with locked offline
  dependencies; formatting and Release Clippy across all targets pass with
  warnings denied. Six selected local regressions pass: backend status/media/
  encoding rejection before I/O, exact cap-plus-one probes for JSON/SSE, one
  deadline across partial chunk writes, unchanged JSON with fresh headers,
  fragmented SSE with bounded flushed chunks, and cancellation while awaiting
  backend data or client flush. Sixteen response regressions are added to the
  existing hosted broker suite; the other cases remain for CI. No local full
  suite, actual backend decoder, real-disconnect cancellation, model execution,
  benchmark or daily-isolation verdict is implied.

- The M1 private backend adapter builds in Release on native ARM64 with locked
  offline dependencies. Seven selected local regressions pass: canonical body
  and private header identity, fragmented framed completion without socket EOF,
  malformed/truncated chunks, strict count schema including alternate tag shape,
  the shared five-second request write/flush deadline, total response head/body
  deadlines, and prevention of downstream terminal framing after truncation.
  Eighteen new cases are included in the existing hosted broker suite. These
  are synthetic supplied-stream checks; live backend connection/identity, actual
  model tokenization, real disconnect cancellation and process isolation remain
  unqualified. No dependency, model execution or benchmark was added.

Only the selected candidate was downloaded. No projector, fallback, inference benchmark, full local suite, driver change, or global system policy change was performed. Technical captures remain outside Git in unique session temp lanes; artifact storage uses the owner-selected external directory.

## Enforced language standards — October 23, 2026

Reviewed the primary [Cargo lint manifest reference](https://doc.rust-lang.org/cargo/reference/manifest.html#the-lints-section), [Clippy lint groups](https://doc.rust-lang.org/clippy/lints.html), [PSScriptAnalyzer settings](https://learn.microsoft.com/en-us/powershell/utility-modules/psscriptanalyzer/using-scriptanalyzer), [Add-Type compiler options](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.utility/add-type?view=powershell-7.6), and [.NET code-analysis configuration](https://learn.microsoft.com/en-us/dotnet/fundamentals/code-analysis/configuration-options). Compiler/analyzer versions are pinned; safety settings live in the package/project and every runtime C# compiler call, with Foundation CI rejecting violations. PowerShell also checks typed parameters, advanced-function/output contracts, strict preferences, and every embedded C# helper through AST inspection.

Independent review caught the interaction between Cargo profile settings and the pinned [subtle 2.6.1 constant-time contract](https://github.com/dalek-cryptography/subtle/blob/2.6.1/src/lib.rs#L50-L53): dependency debug assertions can contain secret-dependent branches. Release debug assertions remain disabled while overflow checks stay enabled; actual compiler arguments and cfg output were inspected. Production panic paths exposed by the new Rust lints were replaced with value-free rejection/sticky transport errors.

The external native engine remains immutable. Its [pinned warning options](https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/cmake/common.cmake) and [MSVC conformance mode](https://learn.microsoft.com/en-us/cpp/build/reference/permissive-standards-conformance?view=msvc-170) were checked before changing the builder. A real warning-as-error build stopped at upstream C4244: int64_t to uint32_t in llama-graph.cpp through make_unique. The final source builder uses required C11/C++17, no extension fallback, /permissive- and /sdl, retaining upstream warning handling. That final native ARM64 Release server build and version/identity probe passed. It ran no model, test suite or benchmark. Changing the recorded compiler configuration requires a fresh owned build lane.

Local language validation passed Release Rust build/Clippy across all targets/features, rustdoc and formatting, selected authentication/context/framing/cancellation regressions, all enabled C# analyzers including private/internal code, and focused GGUF Parser and synthetic Download cases. C# negative controls rejected nullable violations, unsafe compilation and unused imports; checked arithmetic threw on overflow. The final shared checker and CI validate the integrated revision; full suites remain hosted-only. Build logs, compiler-negative controls and the engine receipt remain in session temp.

## Rust GGUF consolidation — October 23, 2026

The migration keeps the existing bounded GGUF v3 report and pinned Prism dialect
contract while moving parsing, PQ2 census and streamed PTQ1 serialization into
`tools/gguf`. The PowerShell entry points retain lock selection, external-path
policy and `ShouldProcess`. The exporter calls the existing safe Rust
`bonsai-ternary` packing routines and reconstructs every converted PQ2 block.
The independent small C# PTQ1 block codec remains; the duplicated C# GGUF parser and
repacker are removed. Provisioning and process supervision remain in PowerShell.

Reviewed the primary [SHA-256 incremental API](https://docs.rs/sha2/0.10.9/sha2/),
[Rust exclusive creation contract](https://doc.rust-lang.org/std/fs/struct.OpenOptions.html#method.create_new),
and [hard-link publication behavior](https://doc.rust-lang.org/std/fs/fn.hard_link.html).
A write/delete-denying source handle spans full-artifact hashing and inspection
or export on Windows. Output creation is exclusive, failed partials are retained,
and validated output publication cannot replace an existing destination. The
same-directory hard link requires filesystem support such as NTFS; an alias
cleanup failure after publication is reported in the successful receipt. The
conversion timeout additionally covers output hashing, copying and padding;
source verification and final output validation remain outside that timeout.
Independent review caught JSON integer type erasure: export rechecks the raw
UINT32 metadata tag before relabeling `general.file_type`.

Existing synthetic GGUF/parser/census/export regressions now invoke the Rust CLI;
Foundation builds it once and runs strict Rust checks and complete correctness
suites on main. No inference, benchmark or measured speedup is part of this
maintenance/packaging migration.
Local verification passed the Release build, all-target strict Clippy, rustdoc,
PowerShell standards and strict C# analyzer build. Focused GGUF Parser and PTQ1
Repack cases passed through the CLI; one selected Rust census test checked raw
symbols across the batch boundary and zero-scale/fourth-code rejection. A real
auto-build wrapper invocation and same-session export `WhatIf` also passed.
Complete Rust and PowerShell suites remain hosted-only. Build outputs, diagnostic
logs and synthetic fixtures are retained in external session-temp lanes.

## Windows integration review (October 23)

The review retains the existing SafeHandle/RAII owners, atomic Job assignment,
explicit inherited-handle list, protected credential creation, artifact read
guards and bounded async socket ownership. Changes address specific Windows
contracts rather than assuming every optional OS feature improves inference.

- [Winsock exclusive address use](https://learn.microsoft.com/en-us/windows/win32/winsock/using-so-reuseaddr-and-so-exclusiveaddruse)
  must be selected before binding. The broker explicitly requests it for its
  frontend listener and temporary backend-port reservation. This makes exclusive
  ownership explicit; current Windows enhanced socket security already rejects
  some conflicting binds even without this option. The publisher backend still
  owns its own socket policy, and listener-PID checks remain necessary.
- [Job limits](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_limit_information)
  include `JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION`. Both process launchers
  use it so an unhandled exception cannot strand a headless owned process at a
  debugger dialog. C# shutdown waits for the root process as well as empty Job
  accounting, and serializes native queries/shutdown with disposal. Active
  stdout/stderr evidence handles allow readers while denying unrelated writers.
- [CreateFile sharing and cache hints](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew)
  apply to the lifetime of an opened handle. Artifact verification reads size
  and SHA-256 through one write/delete-denying stream with a sequential-scan
  hint. Provisioning keeps the exclusive partial handle through hashing and
  [same-volume hard-link publication](https://learn.microsoft.com/en-us/windows/win32/fileio/hard-links-and-junctions),
  without replacing an existing final name. Publication requires filesystem
  support such as NTFS and retains the partial on failure.

[Byte-range locks](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-lockfileex)
do not prevent access through mapped views. Existing whole-file sharing modes
are a better fit for immutable model/runtime artifacts and credentials; adding
range locks would not strengthen their existing contract. Mapped-view mutation
by a preexisting mapping is not a newly established isolation boundary.

[Hard thread affinity](https://learn.microsoft.com/en-us/windows/win32/procthread/multiple-processors)
can interfere with Windows scheduling. CPU sets, EcoQoS, priority changes and
large pages are workload-dependent candidates, not unconditional improvements.
[Large pages](https://learn.microsoft.com/en-us/windows/win32/memory/large-page-support)
also require a privilege and nonpageable allocations. This pass introduces no
affinity, priority, privilege or residency change and claims no measured
throughput, latency, power or thermal improvement. Such tuning needs the existing
device qualification and CI-only benchmark process.

Local verification passed Release all-target/all-feature Clippy for the broker
and Windows process boundary, Windows API documentation and formatting checks,
focused download/publication, exclusive-listener, native crash and C# lifecycle
regressions. Independent review removed a `Process.Dispose`/`Exited` callback
lock inversion; a temporary mutation restoring that inversion correctly failed
the callback regression. Complete correctness suites remain in Foundation CI.
