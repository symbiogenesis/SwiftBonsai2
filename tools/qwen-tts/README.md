# Native Qwen3-TTS VoiceDesign

Read Aloud uses **Qwen3-TTS 12Hz 1.7B VoiceDesign** through the
[native Rust companion](../../src/speech-tts/README.md) and a reviewed GGML C++
core. A plain-language voice description controls synthesis; no voice preset,
speaker file or reference recording is required.

`./scripts/Start-Chat.ps1` downloads and verifies the pinned Q8_0 talker and
codec, builds native ARM64 companions, and configures Read Aloud automatically.
Enter a plain-language **Voice profile**, for example “A warm, low-pitched
narrator with a calm pace and clear consonants.” It applies to the next explicit
read-aloud request. Only the visible completed answer is spoken; reasoning is
excluded. The voice description guides generation without guaranteeing a stable
speaker identity between segments.

Qwen loads on demand after Swift startup, with microphone input released first.
The app synthesizes sequential text segments of at most 256 characters and plays
bounded PCM buffers while synthesis continues. It prepares subsequent audio while
earlier audio plays. After a successful readout, the monitored owned service stays
warm for at most 30 idle seconds. Stop speech, failure, transcription and shutdown
release it immediately. Stop speech cancels loading, synthesis and playback.
The independent speech Job has a 16 GiB commit ceiling, requires 9 GiB available
before startup and stops only speech below a 5 GiB physical-memory reserve.
Text chat and the completed answer survive a speech resource failure.

Use `-NoTts` to disable automatic output setup, `-NoSpeechModels` for a text-only
automatic launch, or `-SpeechModelsOnly` to download without building/launching.
For only the native TTS model and codec:

```powershell
./scripts/Get-NativeSpeechModels.ps1 -DataDirectory $data -Model QwenTts
```

Use `-TtsQuantization Q4_K_M` on provisioning or `Start-Chat.ps1` to evaluate the
smaller pinned Q4_K_M talker with the same Q8_0 codec. Q8_0 remains the default.
Both files are verified against the selected immutable lock before loading;
changing filenames never bypasses model verification. Q4 voice quality and speed
must be evaluated before treating it as an equivalent replacement.

`Start-Chat.ps1 -TtsThreads 10` sets an explicit CPU thread budget (1 through
256); zero uses the automatic budget, which reserves two available logical CPUs.
The standalone service accepts `--threads` and `--quantization Q4_K_M`.

The native [model lock](../../src/speech-tts/model.lock.json) pins both files to
immutable conversions of the named Qwen checkpoint. The model is Apache-2.0;
preserve Qwen attribution and notices. The legacy `model.lock.json` here is only
for the explicit original-snapshot download utility, not the app runtime.
Existing raw snapshots and old private runtimes are not deleted or modified.

For an independently managed native companion, run from an external working
directory with absolute external artifact/key paths:

```powershell
& $ttsExecutable --model $speech.TtsModelPath --codec $speech.TtsCodecPath --token-file $ttsKey --port 8767
```

Pass `-TtsKeyFile $ttsKey` to the demo. Use the repository's protected development
key tooling: exactly 64 lowercase hex bytes, no newline, owner/LocalSystem access.
The key value must never appear in arguments, environment variables or logs.

The service binds exclusively to `127.0.0.1` only after verified models load.
HTTP/1.1 requires exact numeric-loopback Host, bearer authorization and
`Connection: close`. Authenticated `GET /health` reports the fixed model, ready,
`voice_design:true`, `sample_rate:24000`, `max_text_characters:2048`,
`max_voice_characters:1024` and `streaming_pcm:true`.
`POST /v1/audio/speech` accepts only:

```json
{"model":"Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign","input":"Hello.","voice_description":"A calm, clear narrator.","language":"Auto","response_format":"wav"}
```

Input is at most 2,048 Unicode characters, description at most 1,024, JSON at most
64 KiB. Automatic language selection is the supported native interface. Unknown
or duplicate fields, model aliases, speaker/reference overrides, browser/Origin
requests, forwarding, request encodings, request chunking and connection reuse are rejected.
Output is complete finite mono 24 kHz PCM16 WAV, at most 120 seconds. Text,
descriptions and audio stay in memory without request-bearing logs or audio files.
One inference runs at a time. A disconnect or the 180-second inference deadline
terminates only this companion; the UI owns its process tree.

`response_format:"pcm"` selects `audio/pcm` streamed with HTTP/1.1 chunked
response framing. Samples are signed 16-bit little-endian, mono, 24 kHz, with
the same 120-second total bound. The terminating zero chunk is sent only after
successful complete inference. Disconnects, invalid samples, model errors and
suspected token truncation leave an incomplete response; played partial audio
does not make that request successful. The client requires complete framing and
rejects malformed, oversized or incomplete audio. It requests WAV from older
independently managed services without the streaming capability.

Both native compilation and actual synthetic speech generation have passed on
Windows ARM64. These establish correctness of one path, not a performance
measurement or broad voice/language qualification. Full suites and all benchmarks
remain CI-only. GPU/NPU acceleration is not enabled. Thread counts, quantization,
streaming throughput and voice quality require separate evaluation on the target
hardware; successful protocol tests do not establish a speedup.
