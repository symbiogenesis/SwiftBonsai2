# Native Qwen3-TTS VoiceDesign

The speech companion uses **Qwen3-TTS 12Hz 1.7B VoiceDesign/Base** through the
[native Rust companion](../../src/speech-tts/README.md) and a reviewed GGML C++
core. A plain-language voice description controls synthesis; no voice preset,
speaker file or reference recording is required.

The [speech setup and operation guide](../../docs/development-guide.md#speech-setup-and-operation)
owns app provisioning, voice controls, manual/streaming read-aloud, CPU budgets,
service lifetime and resource limits. The native library owns
[fixed voice conditioning](../../src/speech-tts/README.md#voice-conditioning);
[streaming resources](../../docs/streaming-resources.md#speech-scheduling-and-playback)
defines bounded segmentation and playback.

## Artifacts

The native [model lock](../../src/speech-tts/model.lock.json) pins all three files to
immutable conversions of the named Qwen checkpoints. The models are Apache-2.0;
preserve Qwen attribution and notices. The legacy `model.lock.json` here is only
for the explicit original-snapshot download utility, not the app runtime.
Existing raw snapshots and old private runtimes are not deleted or modified.

`--quantization Q8_0` selects the default compiled artifact lock.
`--quantization Q4_K_M` selects the separately reviewed four-bit VoiceDesign/Base talker lock with the
same Q8_0 codec. All three files must match the selected immutable lock before
loading; arbitrary files or changed filenames cannot bypass verification. Q4
quality and speed remain unqualified.

## Standalone service

For an independently managed native companion, run from an external working
directory with absolute external artifact/key paths. `$speech` contains the
verified paths returned by the setup guide's `-SpeechModelsOnly` command:

```powershell
& $ttsExecutable --model $speech.TtsModelPath --codec $speech.TtsCodecPath --voice-model $speech.TtsVoiceModelPath --token-file $ttsKey --port 8767
```

Pass `-TtsKeyFile $ttsKey` to `Start-Chat.ps1` as described in the setup guide.
`--threads 1` through `256` selects an explicit inference budget; omitting it
uses the library's automatic budget. Use the repository's protected development
key tooling: exactly 64 lowercase hex bytes, no newline, owner/LocalSystem access.
The key value must never appear in arguments, environment variables or logs.

## HTTP contract

The service binds exclusively to `127.0.0.1` only after verified models load.
HTTP/1.1 requires exact numeric-loopback Host, bearer authorization and
`Connection: close`. Authenticated `GET /health` reports the fixed model, ready,
`voice_design:true`, `sample_rate:24000`, `max_text_characters:2048`,
`max_voice_characters:1024`, `streaming_pcm:true` and `consistent_voice:true`.
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
the same 120-second total bound; each chunk contains complete samples. PCM and
WAV retain the same model, language, plain-text and voice-description subset.
The terminating zero chunk is sent only after successful complete inference.
Disconnects, invalid samples, model errors and suspected token truncation leave
an incomplete response; played partial audio
does not make that request successful. The client requires complete framing and
rejects malformed, oversized or incomplete audio. Independently managed services
must advertise `consistent_voice:true`; services without streaming support may
still return WAV. Older services need an update before the UI will read aloud.

## Qualification

Both native compilation and actual synthetic speech generation have passed on
Windows ARM64. These establish correctness of one path, not a performance
measurement or broad voice/language qualification. Full suites and all benchmarks
remain CI-only. GPU/NPU acceleration is not enabled. Thread counts, quantization,
streaming throughput and voice quality require separate evaluation on the target
hardware; successful protocol tests do not establish a speedup.
