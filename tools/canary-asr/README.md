# Canary-Qwen speech companion

This optional inference service runs **NVIDIA Canary-Qwen-2.5B** for English
speech recognition. It receives clips recorded by the native Rust application's
microphone control and returns editable transcript text. It has no microphone
API and cannot start listening. It does not implement presentation, a browser
interface, speech synthesis, image understanding, or a cloud fallback.

The model integration uses NVIDIA's actual SpeechLM2 `SALM.generate` API. The
published checkpoint is a Hugging Face directory containing `config.json` and
`model.safetensors`, **not a `.nemo` file**. `SALM` uses `HFHubMixin`; a generic
`ASRModel.restore_from()` example is not the correct loader for this model.

## Runtime and explicit provisioning

The demo launcher downloads the pinned model and all required tokenizer files
automatically into `<DataDirectory>/speech/` before opening the native app. To
download without compiling or launching:

```powershell
$speech = ./scripts/Start-Demo.ps1 -DataDirectory $data -SpeechModelsOnly
```

Use `$speech.CanarySnapshotPath` for `--snapshot` and
`$speech.CanaryTokenizerSnapshotPath` for `--tokenizer-snapshot` below. For STT
files alone, run `./scripts/Get-SpeechModels.ps1 -DataDirectory $data -Model Canary`;
add `-VerifyOnly` to check the existing cache without network requests. The
downloader verifies exact sizes/SHA-256, resumes partial transfers and reuses
valid files. `-NoSpeechModels` on the demo skips provisioning. Python runtime
setup and starting the companion remain separate from downloading its files.

Use a separate, tested Python environment with a compatible PyTorch stack,
NVIDIA NeMo SpeechLM2, Transformers, and the audio dependencies specified by
NVIDIA. The model card specifies PyTorch 2.6+ and NeMo 2.5+; SpeechLM2 APIs have
changed since that release, so record the exact tested package/source revisions.
Current NeMo installation guidance provides the `speechlm2` extra. This repository
does not silently install or upgrade that environment during demo startup.

NVIDIA's published performance results use NVIDIA GPUs. Native Windows ARM64
NeMo/PyTorch compatibility, memory admission alongside Swift, CPU latency and
real-model recognition quality have **not** been qualified here. The
Snapdragon Adreno/Hexagon devices are not CUDA devices. `--device cpu` requests
CPU execution, but does not guarantee that an arbitrary ARM64 package stack or
the checkpoint's BF16 operations work. `--device cuda` requires an actually
compatible local NVIDIA CUDA environment. Do not substitute another recognizer
when Canary cannot load.

Provision the model separately using the immutable URLs in
[`model.lock.json`](model.lock.json), selecting revision
`b1469e1bba1cfe140205529c79c434ca47180960` of
[`nvidia/canary-qwen-2.5b`](https://huggingface.co/nvidia/canary-qwen-2.5b).
The weights contain 5,119,120,624 bytes (approximately 5.12 GB). Keep a regular-file
local snapshot in an owner-controlled directory outside the checkout. The
service requires the exact locked sizes and SHA-256 of `config.json` and
`model.safetensors`; there is no caller hash override. Weight identity comes
from publisher LFS metadata; the configuration digest was independently computed
from the small immutable publisher file. These identify approved bytes, rather
than attesting a tested runtime or source-to-binary equivalence. Keep the
snapshot immutable while serving it.

Canary-Qwen-2.5B is Copyright NVIDIA Corporation and released under
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). Preserve NVIDIA's
attribution, model card and license when provisioning or distributing the model.
This repository's Python adapter does not redistribute the model weights or
modify their licensing.

Provision the six small configuration/tokenizer files in
[`tokenizer.lock.json`](tokenizer.lock.json) to a separate private directory.
These are from `Qwen/Qwen3-1.7B` revision
`70d244cc86ccca08cf5af4e1e306ecf908b1ad5e`; its additional model weights are not
needed because the Canary checkpoint already contains the trained LLM weights.
The pinned Canary configuration also includes the full speech encoder and
preprocessor architecture. No separate Canary-1b-flash weights are loaded.

Both snapshots permit only their locked runtime files and root README,
LICENSE/NOTICE or `.gitattributes` documentation. Optional runtime overrides,
directories and reparse/symlink entries fail preflight. The worker copies and
re-verifies the six tokenizer files into a fresh private temporary HF cache,
with `refs/main` fixed to the pinned revision. It sets the HF/Transformers cache
environment variables before imports, so an inherited unversioned user cache is
not consulted. All temporary cache files are removed with the worker directory.

The service sets `HF_HUB_OFFLINE=1`, `TRANSFORMERS_OFFLINE=1`, and passes
`local_files_only=True` plus `trust_remote_code=False`. A missing dependency
causes startup failure; there is no automatic model or tokenizer download.
These library flags are not OS-enforced network isolation. Qwen3-1.7B is released
by the Alibaba Qwen team under Apache 2.0; preserve its attribution/license with
the provisioned tokenizer files. The new cache contract is covered by a
synthetic regression; actual model execution remains unqualified.

Create a separate protected development key using the repository's key
provisioning tooling. Pass the absolute path to a **64-byte lowercase hex key
file without a newline**, stored outside Git with access restricted to the
owner and LocalSystem. The companion checks file type/length and key format;
the provisioning tool and owner remain responsible for the file/ancestor ACLs.
Never put a key value in command arguments, environment variables or logs.

Run from an external working directory, preserving normal Windows environment
variables:

```powershell
$service = 'C:\source\SwiftBonsai2Runner\tools\canary-asr\server.py'
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('Canary-Service-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($scratch)
$previous = [Environment]::CurrentDirectory
Push-Location $scratch
try {
    [Environment]::CurrentDirectory = $scratch
    python -B $service --snapshot 'D:\Models\Canary\reviewed-snapshot' `
        --tokenizer-snapshot 'D:\Models\Canary\reviewed-qwen-tokenizer' `
        --token-file 'D:\Private\canary-key' --port 8766 --device cpu
} finally {
    [Environment]::CurrentDirectory = $previous
    Pop-Location
}
```

The numeric IPv4 loopback listener opens only after local model loading succeeds.
An unavailable model/runtime does not produce a false ready endpoint. The service
does not generate keys, load arbitrary requested models or download checkpoints.

## HTTP supported subset

All requests use HTTP/1.1, exact `Host: 127.0.0.1:<port>`,
`Authorization: Bearer <key>`, and `Connection: close`. Defaults are
`127.0.0.1:8766`. Origin/browser requests, forwarding metadata, duplicate headers,
chunked/encoded uploads, pipelining and connection reuse are unsupported.

Authenticated `GET /health` has no body and returns:

```json
{
  "model": "nvidia/canary-qwen-2.5b",
  "ready": true,
  "sample_rate": 16000,
  "max_clip_seconds": 30,
  "input_format": "pcm_s16le_wav",
  "channels": 1
}
```

Authenticated `POST /v1/audio/transcriptions` requires one Content-Length and
`multipart/form-data; boundary=<ASCII token>`. Its only fields are:

| Field | Contract |
| --- | --- |
| `model` | Exactly `nvidia/canary-qwen-2.5b`; required |
| `file` | One complete RIFF WAV, mono 16 kHz, signed 16-bit PCM; required |
| `language` | Exactly `en`; optional, defaults to `en` |

File parts include a filename and `Content-Type: audio/wav` (`audio/x-wav` and
`application/octet-stream` also accepted). Field parts have no Content-Type.
Unknown/duplicate fields, model aliases, prompt overrides and other languages
are rejected. Filenames are never used as filesystem paths. Valid clips are
longer than zero and at most **30 seconds**. The whole multipart body is at most
1 MiB; headers are at most 16 KiB/64 fields. Upload intake has an absolute
10-second deadline.

Successful transcription returns HTTP 200:

```json
{"text":"The recognized English speech.","model":"nvidia/canary-qwen-2.5b"}
```

Errors return a value-free `{"error":{"code":"..."}}` JSON object and an
appropriate 4xx/5xx status. There are no transcript-bearing access/error logs.
Responses set `Cache-Control: no-store` and explicit Content-Length. This is a
small OpenAI-style transcription subset, not unrestricted SDK compatibility.

## Model work, cancellation and storage

One connection is handled at a time, with an OS backlog of one and no handler
threads or application request queue. The persistent model lives in a separate
process. Generation uses only NVIDIA's fixed ASR prompt:
`Transcribe the following: <|audioplaceholder|>`, one supplied WAV, and a bounded
256-token generation budget. A budget-exhausted transcript is rejected.

Inference has a 120-second deadline. Disconnect, extra input, timeout or model
failure makes the service terminate its model process and refuse further
transcriptions until an explicit service restart. Health then reports
`ready: false`. Closing a native microphone session should close its request
socket rather than merely abandoning a UI callback. A normal client keeps its
sending half open until the response completes.

Multipart upload data stays in memory in the service. NeMo's file-based API
receives a WAV in a unique external system-temp directory with a fresh subdirectory
per clip, removed immediately after each generation attempt. Cancellation kills
the worker before cleanup. User/audio data is never put in a repository. The
companion cannot guarantee cleanup after a forced OS/process crash; retain normal
owner-controlled system-temp permissions and inspect any such remnants before
sharing diagnostics. It performs no audio/transcript persistence or telemetry.

## Focused verification

The tests use a fake recognizer and synthetic PCM. They establish HTTP framing,
authorization, model/language restrictions, input validation, capability shape,
and hash-failure behavior. They do **not** establish actual Canary loading,
recognition quality, ARM64 support, memory safety or performance.

From an external working directory, a focused selection is:

```powershell
python -B C:\source\SwiftBonsai2Runner\tools\canary-asr\test_server.py `
    HttpContractTests.test_authenticated_health_and_transcription_use_exact_model `
    HttpContractTests.test_auth_framing_models_and_language_fail_before_inference
```

Complete suites and every benchmark remain CI-only. A real-model device check
requires independently provisioned artifacts and the qualified runtime; do not
claim the fake-recognizer results as a model acceptance gate.

References: [NVIDIA's model card and exact `generate` example](https://huggingface.co/nvidia/canary-qwen-2.5b),
[SpeechLM2 documentation](https://docs.nvidia.com/nemo-framework/user-guide/latest/nemotoolkit/speechlm2/intro.html),
[SALM source](https://github.com/NVIDIA-NeMo/Speech/blob/main/nemo/collections/speechlm2/models/salm.py),
[local/offline HFHubMixin loader](https://github.com/NVIDIA-NeMo/Speech/blob/main/nemo/collections/speechlm2/parts/hf_hub.py).
