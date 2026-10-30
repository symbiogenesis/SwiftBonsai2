#requires -Version 7.6
<#
.SYNOPSIS
Opens the native Rust Swift Bonsai chat application.
.DESCRIPTION
Rust admits one launcher per Windows user through preparation and app teardown.
A duplicate returns immediately; SpeechModelsOnly remains available.
Rust validates options and opens the native app after checking its executable.
An owned Rust worker prepares the broker, speech files and credentials while
the window remains available for drafting. Unchanged launches skip Cargo and
native compiler setup. ForceBuild repeats pinned build checks.
SpeechModelsOnly fully verifies speech files without building or launching apps.
NoSpeechModels disables automatic speech setup. Models load only when used.
NativeEngine explicitly selects the pinned Rust runtime. Original PQ2 remains
the weight default; UseRepackedPtq1 selects the verified current-weight export.
NativeRowWorkers selects one or four workers; four requires NativeEngine and
the reviewed 0.1.9 runtime. One remains the default.
NoAbliterateLora explicitly omits the approved adapter. Selection is not promotion.
.EXAMPLE
./scripts/Start-Chat.ps1 -UseRepackedPtq1
.EXAMPLE
./scripts/Start-Chat.ps1 -NoSpeechModels
#>
[CmdletBinding()]
[OutputType([string], [pscustomobject])]
param(
    [string]$DataDirectory = (Join-Path $env:LOCALAPPDATA 'SwiftBonsai2'),
    [string]$ChatPath, [string]$BrokerPath, [string]$AsrPath, [string]$TtsPath, [string]$ToolPath,
    [ValidateSet('Q8_0', 'Q4_K_M')][string]$TtsQuantization = 'Q8_0',
    [ValidateRange(0, 256)][int]$TtsThreads = 0,
    [switch]$NoAbliterateLora, [switch]$UseRepackedPtq1, [switch]$NativeEngine,
    [ValidateSet(1, 4)][int]$NativeRowWorkers = 1,
    [ValidateRange(1024, 65535)][int]$FrontendPort = 5173,
    [ValidateRange(1024, 65535)][int]$BackendPort = 8081,
    [string]$CanaryKeyFile, [ValidateRange(1024, 65535)][int]$CanaryPort = 8766,
    [string]$TtsKeyFile, [ValidateRange(1024, 65535)][int]$TtsPort = 8767,
    [switch]$NoTts, [switch]$NoSpeechModels, [switch]$SpeechModelsOnly, [switch]$ForceBuild
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if ($NativeRowWorkers -eq 4 -and -not $NativeEngine) { throw 'NativeRowWorkers 4 requires NativeEngine.' }
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
# A short probe avoids resolving/building the maintenance tool for a running app.
# Rust arbitrates racing launches and owns this persistent lock through teardown.
if (-not $SpeechModelsOnly) {
    $instanceDirectory = Resolve-ExternalPath -Path (Join-Path $env:LOCALAPPDATA 'SwiftBonsai2')
    [void][IO.Directory]::CreateDirectory($instanceDirectory)
    $instancePath = Resolve-ExternalPath -Path (Join-Path $instanceDirectory 'chat.lock')
    try {
        $probe = [IO.FileStream]::new($instancePath, [IO.FileMode]::OpenOrCreate,
            [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
        $probe.Dispose()
    } catch [IO.IOException] {
        if (($_.Exception.HResult -band 0xffff) -notin @(32, 33)) { throw }
        Write-Information -MessageData 'Swift Bonsai 2 is already starting or running. No second instance was opened.' -InformationAction Continue
        return
    }
}
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Tools.psm1') -Force
$request = @{ RepositoryRoot = (Split-Path $PSScriptRoot -Parent); DataDirectory = $DataDirectory }
foreach ($name in $PSBoundParameters.Keys) {
    if ($name -notin @('ToolPath', 'Verbose', 'Debug', 'ErrorAction', 'WarningAction', 'InformationAction', 'ProgressAction', 'ErrorVariable', 'WarningVariable', 'InformationVariable', 'OutVariable', 'OutBuffer', 'PipelineVariable')) {
        $value = $PSBoundParameters[$name]
        $request[$name] = if ($value -is [switch]) { [bool]$value } else { $value }
    }
}
if (-not $ToolPath -and $env:BONSAI_TOOLS_PATH) { $ToolPath = $env:BONSAI_TOOLS_PATH }
$tool = Resolve-BonsaiRustTool -Name 'bonsai-tools' -ManifestRelativePath '../tools/bonsai/Cargo.toml' -ToolPath $ToolPath -ForceBuild:$ForceBuild
$result = Invoke-BonsaiTool -Command chat-launch -Request $request -ToolPath $tool -TimeoutSeconds 0
if ($SpeechModelsOnly) { [pscustomobject]$result }
elseif ($result.Contains('Status') -and $result.Status -ceq 'already-running') {
    Write-Information -MessageData $result.Message -InformationAction Continue
}
