# Qwen3-TTS voice-design companion

This optional local inference service uses the exact
**`Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign`** checkpoint. The native Rust UI supplies
the answer text and a plain-language voice description; the companion passes
that description to Qwen's `generate_voice_design(..., instruct=...)` method.
Users can describe an invented voice without choosing a preset or providing
reference audio. There is no SAPI, other-model or cloud fallback.

This process performs inference only. Presentation, controls, playback and
microphone permission remain in the native application. This service has no
microphone access and does not implement a browser or WebView interface.

## Automatic demo setup

`./scripts/Start-Demo.ps1` now configures Read Aloud automatically. In addition
to the model files it provisions a private, pinned CPython 3.13 x64 environment
and the complete Qwen dependency set. Windows x64 emulation supplies the runtime
on ARM64: upstream torchaudio does not publish a Windows ARM64 wheel. Installation
does not change global Python, PATH or file associations. Downloads are pinned
by URL, byte length and SHA-256 in [`runtime.lock.json`](runtime.lock.json);
package installation uses those local artifacts with dependency resolution and
package index access disabled. Existing installations run an import/version probe.

The launcher creates a separate protected credential and copies this service
and its model lock to external demo storage. The native Rust speech worker
starts the service only when Read Aloud is selected, waits for authenticated
model readiness, and owns its complete process tree in a Windows Job. Finishing,
cancelling or failing a readout closes that Job and releases the model. Another
Read Aloud starts a fresh service, including after a cancelled synthesis.
This avoids preloading Qwen before Swift's unchanged startup memory admission.

Use `-NoTts` to skip automatic runtime setup, or `-NoSpeechModels` to skip speech
downloads and automatic TTS. `-SpeechModelsOnly` downloads weights without
installing a runtime. A supplied `-TtsKeyFile` selects an independently managed
service and bypasses automatic runtime ownership.

## Independently managed local setup

The PowerShell demo launcher downloads the pinned VoiceDesign model, bundled
speech codec and tokenizer/config files automatically. To obtain the snapshot
path without compiling or opening the demo:

```powershell
$speech = ./scripts/Start-Demo.ps1 -DataDirectory $data -SpeechModelsOnly
$speech.TtsSnapshotPath
```

For TTS files alone, use
`./scripts/Get-SpeechModels.ps1 -DataDirectory $data -Model QwenTts`. Add
`-VerifyOnly` to check existing files offline. Downloads resume and exact size/
SHA-256 checks guard publication; valid files are reused. `-NoSpeechModels` on
the demo skips this provisioning step. For independently managed service
operation, prepare the supported Python runtime and service below.

Provision a dedicated, tested Python environment following
[Qwen's official installation guidance](https://github.com/QwenLM/Qwen3-TTS#quickstart).
The inspected upstream package declares `qwen-tts` 0.1.1, Transformers 4.57.3 and
Accelerate 1.12.0, plus PyTorch/audio dependencies. Record the exact environment
versions used for device acceptance. The automatic demo uses its separate pinned
private environment; it never changes this independently managed environment or
launches Qwen's Gradio/web demo.

Native Windows ARM64 package compatibility, CPU latency, synthesis quality and
combined memory admission alongside Swift and Canary have **not** been qualified.
`--precision bfloat16` is the default for both CPU and CUDA, using eager
attention. The CPU path keeps weights in half-sized BF16 storage rather than
expanding them to float32 alongside Swift. It uses four intra-op threads and
one inter-op thread. CPU synthesis remains slow; clients should send short
sequential clips within the existing request deadline. Explicit
`--precision float16` and `--precision float32` are available for independently
qualified environments, with no automatic precision or voice-engine fallback.
`--device cuda` uses CUDA device zero and requires a compatible local NVIDIA
environment. These execution choices do not promise that arbitrary native
ARM64 PyTorch/audio packages work or are fast enough.
Snapdragon Adreno/Hexagon devices are not CUDA devices. A runtime that cannot
load the model fails startup rather than choosing another voice engine.

Provision every file in [`model.lock.json`](model.lock.json) in a private,
owner-controlled regular-file directory outside Git. The lock selects immutable
revision **`5ecdb67327fd37bb2e042aab12ff7391903235d3`** and includes:

- The 3,833,402,552-byte VoiceDesign `model.safetensors`.
- The bundled 682,293,092-byte `speech_tokenizer/model.safetensors`.
- All nine required root/tokenizer configuration, vocabulary and merge files.

The service checks every locked size and SHA-256 before loading, with no caller
hash/model override. Keep only these runtime files and optional root README,
LICENSE/NOTICE files or `.gitattributes` in the snapshot. Extra runtime files,
unrecognized directories and reparse/symlink entries are rejected before model
imports so optional tokenizer/config files cannot override verified assets.
Weight hashes come from immutable official publisher LFS
metadata. Hashes of all configuration/tokenizer assets were computed from their
small immutable publisher files; neither weight file was downloaded during
implementation. Preserve the locked directory unchanged while the service runs:
model libraries reopen paths after preflight verification, and owner-controlled
storage remains necessary.

Qwen3-TTS and this model are released by the Alibaba Qwen team under
[Apache 2.0](https://www.apache.org/licenses/LICENSE-2.0). Preserve the upstream
license, attribution and any notices when provisioning or redistributing the
model. This adapter does not redistribute model weights.

The worker sets `HF_HUB_OFFLINE=1` and `TRANSFORMERS_OFFLINE=1`, requests
`local_files_only=True` and `trust_remote_code=False`, and loads the complete
locked local snapshot. All required tokenizer assets are bundled in that
snapshot. Missing dependencies/artifacts fail explicitly; there is no automatic
download. Library offline flags do not establish OS-enforced egress isolation.

Create an independent, protected 64-byte lowercase-hex key file without a
newline using the repository's development-key provisioning tooling. Store it
outside Git with owner/LocalSystem-only access. Use a different key from Canary
and the chat broker. The service validates key format/type/length; provisioning
and the owner remain responsible for file/ancestor ACLs. Never pass key values
as arguments, environment variables or logs.

Run from a unique external working directory, preserving inherited Windows
system/profile environment variables:

```powershell
$service = 'C:\source\SwiftBonsai2Runner\tools\qwen-tts\server.py'
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('Qwen-TTS-Service-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($scratch)
$previous = [Environment]::CurrentDirectory
Push-Location $scratch
try {
    [Environment]::CurrentDirectory = $scratch
    python -B $service --snapshot 'D:\Models\QwenVoiceDesign\locked-snapshot' `
        --token-file 'D:\Private\qwen-tts-key' --port 8767 --device cpu
} finally {
    [Environment]::CurrentDirectory = $previous
    Pop-Location
}
```

The listener opens only after the actual model loads and verifies its
`voice_design` model type. The default endpoint is numeric loopback
`127.0.0.1:8767`; Windows binding uses exclusive address ownership.

## Supported HTTP contract

Every request uses HTTP/1.1, exact `Host: 127.0.0.1:<port>`,
`Authorization: Bearer <key>`, and `Connection: close`. Browser/Origin requests,
forwarding metadata, duplicate headers, chunked/encoded uploads and connection
reuse are unsupported. This is a bounded OpenAI-style speech endpoint with an
explicit voice-design extension, rather than unrestricted SDK compatibility.

Authenticated `GET /health`, with no body, returns:

```json
{
  "model": "Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign",
  "ready": true,
  "voice_design": true,
  "sample_rate": 24000,
  "max_text_characters": 2048,
  "max_voice_characters": 1024
}
```

Authenticated `POST /v1/audio/speech` requires `Content-Type: application/json`
and one matching Content-Length. Its complete JSON object is:

```json
{
  "model": "Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign",
  "input": "The answer to read aloud.",
  "voice_description": "A warm low-pitched narrator with a gentle rasp and relaxed pace.",
  "language": "Auto",
  "response_format": "wav"
}
```

All five values must be strings. Unknown/duplicate/null fields, speaker presets,
reference audio and arbitrary generation overrides are rejected. Input text and
voice description must be nonblank and contain at most 2048 and 1024 Unicode
characters respectively. `language` accepts `Auto` or the official supported
names: Chinese, English, Japanese, Korean, German, French, Russian, Portuguese,
Spanish and Italian. The UI uses `Auto` so supported languages can follow the
answer text. Lone JSON surrogate escapes are rejected.

HTTP 200 contains `Content-Type: audio/wav`, an explicit Content-Length and one
complete mono signed-16 PCM WAV at 24 kHz. The maximum wire size is 5,760,044
bytes (120 seconds of samples plus the canonical header). Generated output near
the model's fixed 1500-codec-token ceiling is conservatively rejected because
the public Qwen wrapper does not expose an EOS finish reason; possibly truncated
speech is not played as a completed answer. Invalid/nonfinite/wrong-rate output
also fails. Errors use value-free `{"error":{"code":"..."}}` JSON and 4xx/5xx
status. Responses use `Cache-Control: no-store`.

Long answers are split by the native client at word boundaries into sequential
requests of at most 32 characters to bound half-precision CPU synthesis latency, within the
service's 2048-character request ceiling. Reusing the same description
guides the voice in each segment; it does not promise identical timbre across
separately sampled clips. This adapter does not clone a real or reference voice.

## Bounds, cancellation and privacy

The automatically owned service uses BF16 weights, 9 GiB startup availability
and a separate 25 ms host-memory monitor. If available RAM falls below 5 GiB,
the app stops only the speech Job, preserving Swift's independent 4 GiB reserve
and chat session. The speech Job has a 16 GiB commit ceiling because loader
commitment can temporarily exceed resident model storage. These limits do not
establish a hard physical reservation against unrelated instantaneous allocations.

The complete request body is at most 64 KiB, with at most 16 KiB/64 HTTP headers
and a ten-second absolute intake deadline. One connection is handled at a time,
with an OS backlog of one and no handler threads/application queue. Model
inference runs in a dedicated persistent worker process under a 180-second
per-request deadline. Client disconnect, extra input or timeout terminates the
worker and requires an explicit service restart; health then reports
`ready: false`. A normal client keeps its sending half open until completion.

Text, descriptions and audio remain in memory; generated WAV is never written
to disk or logged. The worker uses a unique directory beneath the service's
external working directory. Graceful shutdown removes it; terminating the owned
Job may leave its empty directory in the retained evidence lane.
Third-party output/tracebacks are suppressed so
they cannot reveal text, voice descriptions or audio in service diagnostics.
The service prints only a value-free startup failure or ready endpoint. It
performs no transcript/audio persistence, remote fetch or telemetry itself.

## Verification and limitations

`test_server.py` uses a fake synthesizer and synthetic PCM. Its checks establish
the freeform request mapping, HTTP/auth/schema/Unicode restrictions, bounded
output validation, artifact hash rejection, unavailable-model behavior and
timeout cleanup. They do not prove actual Qwen loading, designed voice quality,
multilingual quality, Windows ARM64 compatibility, memory or latency.

Run named focused tests from an external working directory, for example:

```powershell
python -B C:\source\SwiftBonsai2Runner\tools\qwen-tts\test_server.py `
    HttpContractTests.test_health_and_freeform_voice_design_return_pcm_without_presets `
    HttpContractTests.test_auth_unknown_model_language_and_text_limits_fail_before_generation
```

Complete suites and all benchmarks remain CI-only. Real-model/device acceptance
requires separately provisioned artifacts and an explicitly tested runtime.

References: [official model](https://huggingface.co/Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign),
[voice-design examples](https://github.com/QwenLM/Qwen3-TTS#voice-design),
[actual loader and `generate_voice_design` implementation](https://github.com/QwenLM/Qwen3-TTS/blob/main/qwen_tts/inference/qwen3_tts_model.py).
