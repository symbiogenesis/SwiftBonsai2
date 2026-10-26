#requires -Version 7.6
<#
.SYNOPSIS
Opens the native Rust Swift Bonsai chat application.
.DESCRIPTION
Allows one launcher per Windows user from preparation until the app exits.
A duplicate returns immediately; SpeechModelsOnly remains available.
Rust validates options, reuses verified builds, provisions pinned speech files,
creates credentials and launches the app. Unchanged launches skip Cargo and
native compiler setup. ForceBuild repeats pinned build checks.
SpeechModelsOnly fully verifies speech files without building or launching apps.
NoSpeechModels disables automatic speech setup. Models load only when used.
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
    [switch]$NoAbliterateLora, [switch]$UseRepackedPtq1,
    [ValidateRange(1024, 65535)][int]$FrontendPort = 5173,
    [ValidateRange(1024, 65535)][int]$BackendPort = 8081,
    [string]$CanaryKeyFile, [ValidateRange(1024, 65535)][int]$CanaryPort = 8766,
    [string]$TtsKeyFile, [ValidateRange(1024, 65535)][int]$TtsPort = 8767,
    [switch]$NoTts, [switch]$NoSpeechModels, [switch]$SpeechModelsOnly, [switch]$ForceBuild
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
# Share one handle per Windows user across source and installed launchers, even
# with different checkouts, data directories or ports. Leave the file in place:
# deleting it on release would introduce a release/delete/recreate race.
$instanceGuard = $null
try {
    if (-not $SpeechModelsOnly) {
        $instanceDirectory = Resolve-ExternalPath -Path (Join-Path $env:LOCALAPPDATA 'SwiftBonsai2')
        [void][IO.Directory]::CreateDirectory($instanceDirectory)
        $instancePath = Resolve-ExternalPath -Path (Join-Path $instanceDirectory 'chat.lock')
        try {
            $instanceGuard = [IO.FileStream]::new($instancePath, [IO.FileMode]::OpenOrCreate,
                [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
        } catch [IO.IOException] {
            # Only sharing/lock violations identify a duplicate. Report other I/O errors.
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
    $request.WorkingDirectory = Resolve-ExternalPath -Path (Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Chat-' + [guid]::NewGuid().ToString('N')))
    [void][IO.Directory]::CreateDirectory($request.WorkingDirectory)
    Write-Information -MessageData "Preparing native chat. Checked unchanged builds are reused. Evidence: $($request.WorkingDirectory)" -InformationAction Continue
    if (-not $ToolPath -and $env:BONSAI_TOOLS_PATH) { $ToolPath = $env:BONSAI_TOOLS_PATH }
    $tool = Resolve-BonsaiRustTool -Name 'bonsai-tools' -ManifestRelativePath '../tools/bonsai/Cargo.toml' -ToolPath $ToolPath -ForceBuild:$ForceBuild
    $result = Invoke-BonsaiTool -Command chat-launch -Request $request -ToolPath $tool -TimeoutSeconds 0
    if ($SpeechModelsOnly) { [pscustomobject]$result }
} finally {
    if ($null -ne $instanceGuard) { $instanceGuard.Dispose() }
}
