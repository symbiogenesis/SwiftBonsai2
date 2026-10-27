# Portable native chat packages

The portable package path installs reviewed prebuilt Windows ARM64 application
files in versioned directories, activates updates atomically and retains an
explicit rollback choice. Launch needs native ARM64 PowerShell 7.6+ and the
ordinary model/runtime installation, without a source checkout or Rust/C++
compiler. Native speech also requires a compatible installed ARM64 Microsoft
Visual C++ runtime: the TTS build uses the dynamic MSVC runtime. This package
does not install runtime DLLs. The UI remains the same native Rust application.

This checked-directory distribution path does not provide a signed installer,
publisher authentication or automatic update discovery. Its synthetic packaging
checks do not establish notice completeness, actual-model acceptance or daily
service isolation. Remaining delivery work is tracked only in
[PLAN.md](../PLAN.md#application-and-daily-service).
SHA-256 detects changed bytes; review the manifest digest
through a trusted channel before installation. Same-user replacement remains
outside this boundary.

## Assemble a package

Build and review the five executables from the intended settled `main` revision:
`bonsai-chat.exe`, `bonsai-broker.exe`, `bonsai-asr.exe`, `bonsai-tts.exe` and
`bonsai-tools.exe`. Use the pinned compiler and existing native speech build
procedure. Package creation does not compile or execute these inputs. Version
and source revision are supplied declarations, not proof of build provenance.

Prepare the applicable third-party notice text from the exact shipped
dependencies; see [redistribution requirements](licensing.md#redistribution).
The script includes project `LICENSE`/`NOTICE` and this supplied text. It does
not generate or certify a complete license inventory. Models, adapters and the
inference runtime remain separately provisioned artifacts with their own pins
and terms.

From the reviewed checkout, using expanded absolute external paths:

```powershell
$package = ./scripts/New-ChatPackage.ps1 `
    -OutputDirectory $newPackageDirectory -Version 0.1.0 `
    -SourceRevision $reviewedSourceRevision `
    -ChatPath $chatExecutable -BrokerPath $brokerExecutable `
    -AsrPath $asrExecutable -TtsPath $ttsExecutable -ToolPath $maintenanceExecutable `
    -ThirdPartyNoticePath $reviewedNotices
$package.ManifestSha256
```

The fixed layout contains the five ARM64 executables, required launch modules,
notices and a bounded `package.json` with every file's exact size and SHA-256.
Creation uses new staging and never replaces an existing output. Distribute
this directory intact, with its reviewed manifest digest supplied independently.
There is no archive extraction or package-provided install hook.

## Install, update and roll back

Run the installer from a trusted checkout/tooling copy:

```powershell
./scripts/Install-ChatPackage.ps1 -InstallationDirectory $installationDirectory `
    -PackageDirectory $receivedPackageDirectory -ManifestSha256 $reviewedManifestSha256
```

The installer validates the fixed inventory, native architecture, sizes, hashes
and external paths before selecting a version. A single-writer lock serializes
operations. It copies into unique staging, verifies again, publishes a new
version directory and atomically replaces `active.json`. Failed verification
preserves the current selection. Existing versions are retained.

Install the next reviewed package with the same command and a new manifest
digest. Rollback checks the retained previous package before switching:

```powershell
./scripts/Install-ChatPackage.ps1 -InstallationDirectory $installationDirectory -Rollback
```

Rollback retains the displaced version, allowing an explicit switch back. An
unchanged install preserves the rollback choice. Updating does not restart a
running app; close it and launch again. Installation and rollback leave model
files and conversation recovery data alone.

## Launch without a checkout

Use the installed version's `scripts/Start-InstalledChat.ps1` as the trusted
entry point, passing the installation root rather than an executable path. A
retained entry point resolves the current active version on each launch:

```powershell
& $installedLauncher -InstallationDirectory $installationDirectory -DataDirectory $data
```

PowerShell verifies the selected manifest, maintenance executable and loader
inventory before executing package code. One Rust operation then verifies all
package files, checks that the bootstrap selection is unchanged and launches
the fixed packaged executables. It avoids a separate inspection process,
version probe and source-launcher import. Pinned speech model provisioning and
credential creation use the packaged maintenance executable. Native inference
continues to use existing artifact checks, admission, Jobs and retry policies.

Provision Swift model/runtime and the optional adapter first using the
[setup commands](development-guide.md#native-chat-app). `-NoSpeechModels` keeps
the packaged launch text-only; `-NoTts` skips output speech. The existing
`-NoAbliterateLora`, `-UseRepackedPtq1`, speech settings and port options remain
explicit. No microphone activates at installation or launch.

## Validation

Foundation runs the complete package lifecycle, rejection and copied-launcher
checks on `main`. A focused local check is
`./tests/Test-ChatPackage.ps1 -Case Lifecycle`. The fixtures use synthetic ARM64
images and mocked process operations; they establish packaging behavior and
dependency closure, not actual-model or speech acceptance.
