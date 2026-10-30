# Inference defaults and quality evidence

Chat preserves the native template and publisher's evaluated sampler, with thinking off
by default. These settings are not qualified as optimal for laptop use or creative
writing. Actual-model application acceptance is pending.

## Applied defaults

| Setting | Default | Reason |
| --- | --- | --- |
| Template | Verified embedded Jinja template | Preserve the model's trained role, turn and reasoning format |
| Thinking / effort | Disabled; `xhigh` when enabled | Chat defaults to off; Settings → Response can enable thinking for the next response |
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

CPU checks, server plans and the Rust development launcher pass sampler/reasoning arguments
explicitly. The pinned completion renderer nevertheless omits template kwargs: its single-turn
check uses the template's thinking-on/`xhigh` defaults. Server requests apply selected settings.

For exact counting and generation, the broker uses the identical canonical body with
`chat_template_kwargs`: selected `enable_thinking` (default false), `reasoning_effort: "xhigh"`,
`preserve_thinking: true`. Other client overrides reject. Original user/optional-system content
is preserved. Clients must retain/supply assistant-only text `reasoning_content` alongside answers;
both consume context. The broker cannot recover omitted history.

The [paired LoRA qualification](swift-lora.md#fixed-acceptance-contract) uses separate
greedy/template controls. Native Rust token-level reference also defaults to greedy
with explicit seeded sampling; its SplitMix64 contract proves no Prism RNG parity
and is not the chat backend.

## Experimental extra effort mode

The [native UI](ui-architecture.md) defaults to **Standard**. Opt-in **Extra effort (experimental)**
is a Rust client workflow with no new broker option, following
[GVS5H's scoped calls/working notes](https://github.com/slee-persis/GVS5H/blob/master/codebase/v2-current/escalation/multiagent.py):

1. Plan around user constraints and distinct approaches.
2. Draft from the original task, relevant sources and plan.
3. Review against original constraints and revise, retaining the draft for comparison.

The same model makes at most three sequential calls, each with a fresh context containing
the original task and relevant state. User instructions and approved facts stay authoritative;
generated notes/references are data. There are no autonomous tools or open-ended manager loop.

Each call retains sampler/template/exact admission: 8K context, reasoning-inclusive caps
512/1,024/2,048/4,096 (default 4,096), and the broker's 64-token margin. Smaller caps leave more
prompt room but can stop reasoning before an answer. Count included plan/draft text; reject
overflow rather than dropping history or raising limits. A shared deadline/cancellation covers
all stages, with progress labels. Retain failed/incomplete stages; truncated revisions neither
replace complete drafts nor become finished answers.

Focused checks cover single-call defaults, stage order, bounds, cancellation and draft retention.
The experimental option has no qualified quality/speed gain; GVS5H coding results do not establish
Swift creative quality. Promotion requires evidence, with independent service/security gates and
CI-only benchmarks.

## What the benchmarks establish

[Swift's published evaluations](performance-expectations.md#current-swift-evidence) support the
sampler baseline, with separate current-weight and historical results. They establish neither an
optimal sampler/effort/prompt nor creative-writing gains.

[Qwen guidance](https://huggingface.co/Qwen/Qwen3.8-27B#best-practices) supports the thinking sampler,
warns stronger presence penalties can hurt, and separately defines non-thinking settings.
Both exact GGUF templates (SHA-256 checked against committed metadata) accept `xhigh`/`medium`/`low`,
default to `xhigh`, and support `preserve_thinking`. Pinning effort is not evidence of a gain.
Clients must retain reasoning for replay; the two sampler modes stay separate.

No reviewed Swift PQ2 creative A/B supports a superior custom system prompt.
[Persona](https://aclanthology.org/2025.emnlp-main.1364/) and
[self-correction](https://arxiv.org/abs/2310.01798) experiments show irrelevant personas or revision
without external feedback can hurt; neither supports mandatory expert personas/rewrite loops.
Supply relevant references, audience, voice and constraints; request useful revisions.

[KIVI](https://arxiv.org/abs/2402.02750) validates a specific asymmetric cache method on tested
models, not arbitrary Q4/Q8 Swift caches. Retain FP16 (512 MiB at 8K) until a supported alternative
passes quality. Prefix reuse saves computation, not added knowledge/reasoning.

## Capacity and execution limits

Profiles admit **8,192 total context tokens** and **4,096 maximum completion tokens**, including
reasoning. The 16-token load/decode and paired LoRA 512-token caps are experiment controls,
not chat recommendations or answer-quality evidence. See [memory](swift-memory.md) and
[broker admission](broker-policy.md).

These limits cannot reproduce Swift's 32K example or longer published reasoning runs. Truncation
may precede an answer and must not be labelled complete or benchmark-equivalent. Larger context
requires revised memory/execution/quality qualification; no automatic token floor bypasses exact
reservation. Retain native RoPE: Qwen warns static YaRN can affect short texts. More allocated
context does not itself improve accuracy. Offload, flash, threads, batches and prefix reuse need
backend/device evidence; other-hardware throughput is not better answer quality.

Optional abliteration retains [independent TPS/quality gates](swift-lora.md#standard-profile-qualification); fewer refusals
prove neither reasoning nor prose gains. Defaults do not promote adapters, representations,
accelerators or daily-service profiles.

## 2026 literature: token budgets are a separate experiment

The [source register](research-register.md#2026-systems-measurement-and-output-budget-work) records
the **2026-10-29** review. These methods change output counts/distributions, candidate answers or
trained artifacts, so compare them separately from faster kernels:

| Method | Admission boundary |
| --- | --- |
| ESTAR; [BudgetThinker](https://github.com/MobileLLM/BudgetThinker) | Require compatible training/components/control tokens. |
| [Budget Guidance](https://github.com/UMass-Embodied-AGI/BudgetGuidance) | Avoids base-model fine-tuning but needs a compatible length predictor. Evaluate guided sampling, predictor overhead and creative quality separately. |
| CASE | Saves work against multi-sample self-consistency; that does not establish a gain over the current single completion or justify parallel candidates. |

Measure time/energy to a useful completed answer, including predictor work, extra prefills, failed
attempts and human revision. Preserve selected thinking/Extra effort, exact reservations,
cancellation and truthful truncation. These methods are disabled and unqualified on current
Swift/LoRA. See [measurement](performance-expectations.md#2026-literature-measurement-and-comparison).
