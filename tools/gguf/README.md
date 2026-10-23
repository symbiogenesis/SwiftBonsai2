# GGUF tools

`bonsai-gguf` owns bounded GGUF v3 inspection, Prism PQ2 census, and experimental
weight-lossless PTQ1 export. It reuses the Rust PQ2/PTQ1 codecs in `tools/ternary`.
Every operation verifies the complete input artifact against an explicit byte
length and SHA-256. On Windows the source handle denies writes and deletion
through verification and use.

The library exposes exact GGUF scalar types and compact homogeneous arrays through
`Metadata`, `MetadataValue`, and `MetadataArray`. Parsing and inference retain
native floating-point values; JSON conversion happens only when producing a
report. Build with `--no-default-features --lib` to omit JSON and export tooling.
The default `report` feature retains the existing CLI, census, and export APIs.

The PowerShell entry points keep their existing artifact policy and defaults:

```powershell
./scripts/Read-Gguf.ps1 -ModelPath C:\Models\pinned.gguf
./scripts/Read-Gguf.ps1 -ModelPath C:\Models\swift.gguf -Dialect Prism-73bfe6cc485554822f80c473a3c0c4f25e80f41b -CensusPq2
./scripts/Export-SwiftPtq1.ps1 -ModelPath C:\Models\swift.gguf -OutputPath C:\Models\swift-ptq1.gguf -WhatIf
```

Inspection defaults to `config/runtime.lock.json`; export uses the pinned model
in `config/swift-assessment.lock.json`. Inspection accepts `-ExpectedBytes` and
`-ExpectedSha256` together for independently pinned artifacts and fixtures. All
model, report, and export paths must satisfy `Resolve-ExternalPath`. Outputs must
be new files in existing external directories. Export supports `-WhatIf` and
`-TimeoutSeconds` (1–3600, default 1800). It creates a unique partial, validates
the result, and publishes without replacing an existing file. Failed partials
remain available for diagnosis. The receipt includes the lock's `SourceRevision`.
Publication uses an exclusive hard link in the output directory and requires
hard-link support (for example NTFS). If removal of the partial alias fails after
publication, the successful receipt records `RetainedPartialPath` and
`PartialCleanupError`. The timeout bounds conversion, copying, padding and the
initial output hash; source verification and final validation are additional work.

Both wrappers accept `-GgufToolPath C:\Tools\bonsai-gguf.exe` for a prebuilt tool.
Without it, the shared `Bonsai.Gguf.psm1` helper runs `cargo build --locked
--release` using `+1.99.1-aarch64-pc-windows-msvc`. Cargo must be installed and
the pinned toolchain and locked dependencies must be available locally or
downloadable by Cargo. Its native working directory, incremental target directory,
and build logs stay in a unique system-temp lane. Each call asks Cargo to check
source freshness; only incremental build artifacts are shared within the current
PowerShell session. Child processes inherit the host environment. Diagnostics do
not enter the returned report. Inspection and export can run in the same session;
no C# types are loaded by either wrapper.

For packaging or direct use, build with an absolute external target directory:

```powershell
$manifest = [IO.Path]::GetFullPath('./tools/gguf/Cargo.toml')
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-GgufBuild-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($scratch)
Push-Location $scratch
try {
    cargo +1.99.1-aarch64-pc-windows-msvc build --locked --release --manifest-path $manifest --target-dir (Join-Path $scratch 'target')
} finally { Pop-Location }
```

The binary is `target/release/bonsai-gguf.exe` beneath that external lane. Its
commands require explicit artifact identities:

```text
bonsai-gguf inspect --model PATH --expected-bytes N --expected-sha256 HASH [--dialect Stock|Prism-73bfe6cc485554822f80c473a3c0c4f25e80f41b] [--census-pq2] [--output PATH]
bonsai-gguf export-ptq1 --model PATH --output PATH --expected-bytes N --expected-sha256 HASH [--timeout-seconds N]
```

Each successful command writes one JSON object to stdout. Inspection's optional
JSON output is created exclusively. Errors go to stderr with a nonzero exit code.
Metadata inspection reads descriptors and metadata after full-artifact hashing;
PQ2 census additionally reads PQ2 tensor payloads in bounded batches. PTQ1 export
preserves the raw scales, ternary symbols, other tensor payloads, and metadata,
and checks byte-identical PQ2 reconstruction. It does not qualify native PTQ1
activation quantization, logits, quality, inference speed, or an inference profile.
No performance improvement is claimed. Small C# codecs remain independent
correctness references.
