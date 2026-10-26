# Native Canary Qwen speech recognition

The demo uses the selected **NVIDIA Canary Qwen 2.5B** model through the
[native Rust companion](../../src/speech-asr/README.md) and the CPU-only
transcribe.cpp engine. Python, NeMo, PyTorch and FFmpeg are not required.
The first-party UI remains Rust and native; inference includes a reviewed C++
GGML core. This is not a claim of pure-Rust tensor inference.

Run `./scripts/Start-Demo.ps1` to provision the immutable Q8_0 GGUF, build native
ARM64 companions and configure the microphone button. It remains off until
explicitly enabled and Windows microphone permission is granted. Canary loads
only after the first captured clip, remains warm while that microphone generation
is enabled, and stops when input is disabled or cancelled. Starting Read Aloud
releases input first. Recognized text goes to the composer without sending.

`./scripts/Start-Demo.ps1 -SpeechModelsOnly` downloads without compiling or
opening the microphone. For only Canary use:

```powershell
./scripts/Get-NativeSpeechModels.ps1 -DataDirectory $data -Model Canary
```

The native [model lock](../../src/speech-asr/model.lock.json) pins the conversion,
source model revision, exact size and SHA-256. Canary is Copyright NVIDIA
Corporation, CC BY 4.0; retain attribution and the model license. The legacy
`model.lock.json` and `tokenizer.lock.json` here describe original reference
snapshots for the explicit `Get-SpeechModels.ps1` utility; the demo does not load
them, and existing downloaded snapshots are preserved.

For a separately managed native companion, run its built executable from an
external working directory, with absolute external model and protected key paths:

```powershell
& $asrExecutable --model $speech.CanaryModelPath --token-file $canaryKey --port 8766
```

Then pass `-CanaryKeyFile $canaryKey` to the demo. A key contains exactly 64
lowercase hex bytes without a newline, protected for the owner and LocalSystem.
Never pass its value through arguments, environment variables or logs.

The service binds exclusively to `127.0.0.1` after loading and retaining verified
model files. All requests require exact `Host: 127.0.0.1:<port>`, bearer auth,
HTTP/1.1 and `Connection: close`. Authenticated `GET /health` reports model,
ready, `sample_rate:16000`, `max_clip_seconds:30`, `channels:1` and
`input_format:"pcm_s16le_wav"`.

`POST /v1/audio/transcriptions` accepts bounded multipart fields `model` (exact
`nvidia/canary-qwen-2.5b`), `file` (mono 16 kHz signed PCM16 RIFF WAV), and optional
`language:"en"`. The complete body is at most 1 MiB; clips are at most 30 seconds.
The UI records five-second clips in memory, pausing while each is transcribed.
A successful response is `{"text":"...","model":"nvidia/canary-qwen-2.5b"}`.
Unknown or duplicate fields, aliases, Origin/forwarding headers, chunked/encoded
uploads and connection reuse are rejected. Errors contain only bounded codes.

The [shared Rust service](../../src/speech-service/src/lib.rs) implements HTTP,
authentication, validation, PCM handling and deadlines. One request runs at a
time. Cancellation, disconnect or the 120-second inference deadline terminates
only the separately owned speech process. Model input and audio remain in memory
and are not logged or persisted. No cloud fallback is configured.

A native Windows ARM64 build and one real public-fixture transcription have
passed locally. General recognition accuracy, GPU/NPU acceleration and performance
remain unqualified. Full correctness suites and every benchmark run only in CI.