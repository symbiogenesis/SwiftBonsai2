# Q4 speech resource comparison

The [paired hosted ARM64 run](https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/40445129576)
at Runner revision `90db7f71033adeb2e262ec6f97f7dc360fceb9c8` completed all eight
fresh-process sessions and sixteen bootstrap/warm WAV records on a four-core
Cobalt 100. Independent framing checks passed for every clip. No spontaneous
worker exits occurred; all termination codes were observed after supervisor stop.

Positive differences below mean lower Q4 usage. Counter domains are separate.

| Threads | Repetition | Q8 minus Q4 peak Job commit | Q8 minus Q4 peak working set |
| ---: | ---: | ---: | ---: |
| 2 | 1 | 0.734196 GiB | 1.532074 GiB |
| 2 | 2 | 0.734276 GiB | 1.532158 GiB |
| 4 | 1 | 0.734226 GiB | 1.532124 GiB |
| 4 | 2 | 0.734653 GiB | 1.532566 GiB |
| Overall median | | **0.734251 GiB** | **1.532141 GiB** |

The active talker file difference is 860,203,008 bytes (0.801126 GiB). Design and
Base talkers are sequential, so their reductions count once for active weights.
The Q8 codec is unchanged. The larger observed working-set reduction does not
identify its allocation cause. Job commit excludes mapped residency; neither
counter is added to the other or treated as complete system RAM.

Phase private/resident maxima were sampled every 25 ms and can miss brief
allocations. The lowest sampled system availability was 7.015308 GiB, above the
unchanged 5 GiB reserve. Admission remained 9 GiB with a 16 GiB commit ceiling.
These results describe this hosted CPU and short public fixtures. Snapdragon,
long-clip and independent listening qualification remain outstanding; Q8 stays
the default. Work items remain in [PLAN.md](../PLAN.md).

The report SHA-256 is
`9b9aecf03a2c0a2467a3b3c8f316c4ee0c2c237dbe67e22975fa5c9b07285eb4`.
The TTS executable was 1,914,368 bytes, SHA-256
`2ac52dddc913ca4e193a2614262d154bd63632cc12a23521c8f2cdfc9e4999d8`.
Artifact `12708687596` retains the exact model identities, individual observations,
failure diagnostics and synthetic listening clips.

## 2026 speech inference research

Sources reviewed **2026-10-29**. The hosted comparison above remains the local
evidence record; these papers do not qualify a different model, quantization,
thread budget or accelerator. The [speech contracts](../tools/qwen-tts/README.md)
retain sequential VoiceDesign/Base residency, reused voice conditioning,
bounded playback and the existing admission/reserve. The
[research register](research-register.md) indexes related runtime literature.

| Source and date | Mechanism and possible experiment | Evidence limit |
| --- | --- | --- |
| [Qwen3-TTS Technical Report](https://arxiv.org/abs/2601.15621), 2026-01-22 | Distinguish the text-conditioned talker, residual code prediction and causal codec. Account for their invocation rates and first-packet prerequisites separately. | The publisher's 97 ms first-packet result does not measure this native GGML 1.7B path, fixed-reference bootstrap, playback pre-roll or CPU. The 12Hz and 25Hz tokenizer variants are different architectures. |
| [RVQ Position Aware Speculative Decoding for On Device Text to Speech](https://arxiv.org/html/2609.37007v1), 2026-09-29 preprint | Reuses frozen codebook heads with 3,072 learned residual-offset parameters and sparse tree verification to reduce sequential residual-code calls. | Reported 2-2.2x is RVQ-token generation on Qwen3-TTS 0.6B CustomVoice on Apple devices. Offsets/calibration are new artifacts; top-k distributional equality does not mean identical seeded audio. No 1.7B VoiceDesign/Base, GGML ARM64 or complete readout result is established. |
| [One Megakernel Is Not Enough](https://arxiv.org/html/2610.35336v1), 2026-10-26 preprint | Select fusion boundaries using producer/codec rates, register pressure, retained resources and first-audio dependencies; the best tested layout varies with batch size. | H100 NVL/CUDA results across Qwen3-TTS, NH-TTS and Llama. Its planner selects among supported measured designs, not all possible implementations. Useful scheduling concept, not a portable megakernel or laptop speed forecast. |
| [Pushing the Limits of On-Device Streaming ASR](https://arxiv.org/html/2604.14493v2), v2 2026-04-19 preprint | Cache-aware streaming, chunk/right-context choices, selective quantization and graph fusion expose a latency/quality/storage tradeoff. | CPU timings use 32 cores of an AMD EPYC 7V12. Its selected Nemotron model and Canary-1B-v2 comparison are distinct from this app's Canary-Qwen-2.5B. Model-file compression is not a process-memory result, and streaming requires compatible trained state semantics. |
| [Beyond the Benchmark: Conditioning, Domains and Failure Modes of Qwen3-TTS](https://arxiv.org/html/2610.23678v1), 2026-10-16 preprint | Conditioning and domain affect timbre, timing and prosody differently; automatic measures can rank outputs differently from listeners. | Voice-cloning evaluation informs listening design, not proof of the app's description-designed reference quality. Preserve the current reference/seed while isolating optimization effects; WER alone cannot qualify voice continuity or prosody. |

For the existing model, first identify time and allocations spent in voice
bootstrap, warm talker prefill, each residual-code step, codec chunks and playback.
Reusing speaker conditioning and bounding retained decode graphs already exist;
paper descriptions are not evidence those opportunities remain unimplemented.
Candidate workspace reuse or fusion must retain cancellation, PCM framing and
voice identity. A new speculative path needs verified accepted-prefix state,
sampling correction and bounded extra scratch; training a tiny drafter still
creates a separate artifact identity.

Report first generated PCM separately from first audible output. Average
faster-than-real-time synthesis can still underrun when individual chunks stall,
and overlapping talker/codec work can contend with Swift or native rendering.
Include chunk supply gaps, long-read peak memory and combined chat/speech
resource use in target-device CI. Existing framing/ownership checks establish
their specific behavior; they do not substitute for those measurements or
independent listening. No benchmark or model execution is part of this survey.
