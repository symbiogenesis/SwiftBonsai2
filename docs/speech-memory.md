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
