# Inference defaults and quality evidence

The defaults preserve the publisher's evaluated configuration and native chat
template. They are the best-supported baseline found in the October 23, 2026
literature review, not a measured optimum for this laptop or creative writing.
Daily chat and Swift serving remain under development.

## Applied defaults

| Setting | Default | Reason |
| --- | --- | --- |
| Template | Verified embedded Jinja template | Preserve the model's trained role, turn and reasoning format |
| Thinking / effort | Enabled / `xhigh` | Both pinned templates support this and default to it; request it explicitly |
| Separate thinking budget | `-1` (no extra cutoff) | Avoid forcing an early reasoning exit; total generation and context caps still apply |
| Sampler | Temperature **1**, top-p **0.95**, top-k **20**, min-p **0** | Matches Swift's published evaluation configuration and Qwen's thinking guidance |
| Penalties | Repetition **1**, presence **0**, frequency **0** | No additional repetition/logit correction by default |
| Added system prompt | None | Retain publisher instructions and user-supplied task/voice constraints |
| Generation harness | One completion, EOS honored | No automatic critique/rewrite, forced extra thinking, majority voting or best-of-N |
| Assistant history | Preserve supplied `reasoning_content` with `preserve_thinking: true` | Both verified templates support replaying prior assistant reasoning |
| Attention / recurrent state | FP16 K/V; F32 recurrent/convolution state | Preserve the current numerical baseline |
| Context handling | Exact rendered token count; reject overflow | Preserve supplied history rather than silently shifting or truncating it |
| RoPE | Model metadata, no override | Context extension can change short-context behavior |
| Cache reuse / speculation | Disabled | Current memory/state and execution baseline; these are not established quality improvements |

The CPU load/decode checks now pass the sampler and reasoning arguments
explicitly. The PowerShell server plan and Rust development launcher do the same.
The pinned completion renderer omits template kwargs on its actual formatting
call, so its parsed thinking/effort switches alone do not prove enforcement;
that single-turn check relies on the verified embedded on/`xhigh` defaults.
The server request path applies the explicit template settings.
The broker supplies `chat_template_kwargs` with `enable_thinking: true`,
`reasoning_effort: "xhigh"` and `preserve_thinking: true` in the identical canonical
body used for prompt counting and generation. Client inference overrides remain
rejected. User messages, including an optional system message, retain their
original content. Clients can replay the returned assistant `reasoning_content`
alongside its answer text; both consume the exact context reservation. The broker
does not invent or recover omitted history. Reasoning history is optional, text
only, and accepted only on assistant turns.

The fixed paired LoRA qualification deliberately keeps its separately recorded
greedy sampler and template defaults. Changing its controls would change the
experiment; it does not inherit the interactive sampler. The token-level native
Rust reference defaults to greedy decoding and also offers explicit seeded
reference sampling. Its independent SplitMix64 contract does not establish Prism
sampler/RNG parity; it is not the chat backend.

## Experimental extra effort mode

The [native Rust UI](ui-architecture.md) offers **Standard** (the default) and
**Extra effort (experimental)**. The native response-mode control is an explicit
opt-in; it explains that extra passes take longer and quality improvements are
unproven. The workflow is implemented in the Rust client and adds no broker
inference option.

The workflow follows the scoped calls and compact working notes described by
[GVS5H](https://github.com/slee-persis/GVS5H/blob/master/codebase/v2-current/escalation/multiagent.py)
as a small client workflow above the broker:

1. Make a short plan that captures the user's constraints and considers distinct
   approaches.
2. Draft from the original task, relevant source material and that plan.
3. Review the draft against the original constraints and produce a targeted
   revision, retaining the draft so the user can compare or keep it.

Use at most three sequential calls to the same loaded model. Each call gets a
fresh context assembled from the original task and the relevant working state,
rather than a growing internal transcript. Keep the user's supplied instructions
and approved facts authoritative; generated notes and reference text are data,
not new instructions or approved canon. This needs no autonomous tool use or
open-ended manager/worker loop.

Each call keeps the existing sampler, native template and exact prompt/output
admission. The current 8K context and 4K maximum completion caps still apply,
including reasoning. The native output control selects 512, 1,024, 2,048 or
4,096 tokens per call, with 4,096 as the default. A smaller output reservation
leaves more room for the prompt; it can also stop reasoning before an answer.
The broker includes its existing 64-token template margin in admission.
Count the plan and draft when included in later prompts; reject
overflow instead of silently dropping history or raising limits. Apply one
overall deadline and cancellation signal to the complete workflow, with progress
labels for planning, drafting and reviewing. Keep incomplete or failed stages
visible; a truncated revision must not replace a complete draft or be labelled a
finished answer.

Initial implementation needs focused behavior checks for default single-call
behavior, stage order, bounds, cancellation and draft retention. An extensive
quality/performance benchmark campaign is not a prerequisite for exposing this
opt-in experimental mode. Do not claim an established quality or speed gain, or
promote it to the default, without appropriate evidence. Any benchmarks remain
CI-only; existing service/security requirements apply independently of the
native MVP UI's availability.
The GVS5H coding results do not establish a Swift creative-writing improvement.

## What the benchmarks establish

[Swift's publisher configuration](https://ukisai.com/swift-bonsai-2) and
[evaluation artifact](https://huggingface.co/ukisai/Swift-Bonsai-2-GGUF/blob/main/benchmark_results.json)
support the sampler baseline. Only IFBench and AIME were refreshed on the current
merged PQ2 files; GPQA and C-Eval used the historical runtime correction. The
current AIME final-answer-only score ties the base. These results do not sweep
effort, system prompts or samplers, and do not establish a creative-writing
improvement. The publisher also reports uneven practical instruction following
and tool use.

[Qwen's guidance](https://huggingface.co/Qwen/Qwen3.8-27B#best-practices) agrees
on the thinking sampler and warns that stronger presence penalties can reduce
performance. Its non-thinking sampler is a different configuration, not a reason
to mix those settings into this mode. The exact pinned GGUF templates, checked
against the SHA-256 values in the committed metadata, both accept `xhigh`,
`medium` and `low`; `xhigh` is already their default. Explicit effort pins that
behavior rather than claiming a new measured gain.

Qwen also recommends preserving historical thinking. Both exact templates
support `preserve_thinking`; the broker now accepts assistant reasoning history
and pins that template setting. Replaying the history still requires the client
to retain and supply it.

There is no reviewed Swift PQ2 creative-writing A/B result showing a superior
custom system prompt. [Persona experiments](https://aclanthology.org/2025.emnlp-main.1364/)
show that irrelevant persona details can hurt performance. Likewise,
[intrinsic self-correction experiments](https://arxiv.org/abs/2310.01798) show
that self-revision without external feedback can degrade reasoning. Neither
establishes a universal benefit from a large expert persona or mandatory rewrite
loop. Supply relevant reference material, a clear audience, voice and constraints
in the actual task; request revision when useful.

The original [KIVI experiments](https://arxiv.org/abs/2402.02750) show that a
particular asymmetric cache-quantization method can preserve quality on tested
models. That is not evidence that arbitrary Q4/Q8 cache types preserve Swift's
quality in the pinned Prism runtime. At the current 8K context, FP16 attention
caches cost only 512 MiB. Retain FP16 until a supported alternative demonstrates
the required quality. Prompt reuse primarily saves computation; it does not add
knowledge or reasoning ability.

## Capacity and execution limits

The development profiles retain **8,192 total context tokens**, with **4,096
maximum completion tokens** in the broker. Completion tokens include reasoning.
The 16-token CPU check verifies loading/decoding, not finished-answer quality.
The paired LoRA test's 512-token cap is also an experiment control, not a chat
recommendation. See [memory accounting](swift-memory.md) and
[broker admission](broker-policy.md).

These bounds cannot reproduce published long-reasoning results: Swift's example
uses a 32K context, and some reported evaluations permit much longer completions.
A request can exhaust its completion allowance before producing a final answer.
Do not label truncation as a completed response or claim benchmark-equivalent
quality from these development limits. A larger-context serving profile needs
revised memory accounting and separate execution/quality qualification. No
automatic token-floor increase may bypass the broker's exact reservation.

Keep native RoPE settings at the admitted length; Qwen warns that static YaRN
can affect shorter texts. More allocated context alone does not make a short
task more accurate. GPU offload, flash attention, thread count, batching and
prompt reuse need supported-backend and device evidence; their throughput
results on other hardware do not prove higher answer quality here.

The optional abliteration adapter retains its existing
[independent quality and throughput gates](lora-qualification.md). Fewer refusals
do not establish better reasoning or prose. This defaults change does not
promote an adapter, weight representation, accelerator or daily-service profile.
