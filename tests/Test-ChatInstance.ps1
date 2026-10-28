#requires -Version 7.6
<#
.SYNOPSIS
Checks the source-launcher bridge and cheap running-instance probe.
.DESCRIPTION
Rust regressions cover admission ownership and release. These copied PowerShell
fixtures verify forwarding and reject duplicates before tool resolution.
No model, compiler, native window, full suite or benchmark is executed.
#>
[CmdletBinding()]
[OutputType([string])]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if (-not $IsWindows) { throw 'Singleton launch contracts target Windows.' }
$repo = Split-Path $PSScriptRoot -Parent
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-ChatInstance-' + [guid]::NewGuid().ToString('N'))
$scripts = Join-Path $scratch 'scripts'
$data = Join-Path $scratch 'data'
$instanceStorage = Join-Path $scratch 'single-instance'
foreach ($directory in @($scripts, $data, $instanceStorage)) { [void][IO.Directory]::CreateDirectory($directory) }
$copy = Join-Path $scripts 'Start-Chat.ps1'
$source = [IO.File]::ReadAllText((Join-Path $repo 'scripts/Start-Chat.ps1'))
# Isolate the singleton scope without redirecting inherited profile variables.
$assignment = '$instanceDirectory = Resolve-ExternalPath -Path (Join-Path $env:LOCALAPPDATA ''SwiftBonsai2'')'
Assert ($source.Contains($assignment)) 'The launcher probe fixture needs its isolated lock path.'
$source = $source.Replace($assignment, ('$instanceDirectory = Resolve-ExternalPath -Path ''' + $instanceStorage.Replace("'", "''") + "'"))
[IO.File]::WriteAllText($copy, $source)
Copy-Item -LiteralPath (Join-Path $repo 'scripts/Bonsai.Common.psm1') -Destination $scripts
$receipt = Join-Path $scratch 'request.json'
$resolution = Join-Path $scratch 'resolved.json'
$lockPath = Join-Path $instanceStorage 'chat.lock'
$mock = @'
function Resolve-BonsaiRustTool {
    [CmdletBinding()]
    [OutputType([string])]
    param([string]$Name, [string]$ManifestRelativePath, [string]$ToolPath, [switch]$ForceBuild)
    [IO.File]::WriteAllText('__RESOLUTION__', (ConvertTo-Json @{ ToolPath = $ToolPath; ForceBuild = [bool]$ForceBuild }))
    return '__TOOL__'
}
function Invoke-BonsaiTool {
    [CmdletBinding()]
    [OutputType([Collections.IDictionary])]
    param([string]$Command, [Collections.IDictionary]$Request, [string]$ToolPath, [int]$TimeoutSeconds)
    if ($Command -cne 'chat-launch' -or $TimeoutSeconds -ne 0) { throw 'Unexpected maintenance invocation.' }
    if ($Request.Contains('SpeechModelsOnly') -and $Request.SpeechModelsOnly) { return @{ TotalBytes = 1 } }
    # The cheap probe must release its handle before native admission.
    $guard = [IO.FileStream]::new('__LOCK__', [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    $guard.Dispose()
    [IO.File]::WriteAllText('__RECEIPT__', (ConvertTo-Json -InputObject $Request))
    if ($Request.Contains('FrontendPort') -and $Request.FrontendPort -eq 6173) { return @{ Status = 'already-running'; Message = 'Native admission rejected a racing launcher.' } }
    if ($Request.Contains('BackendPort') -and $Request.BackendPort -eq 6181) { throw 'Deliberate native preparation failure.' }
    return @{ Status = 'chat-stopped'; ExitCode = 0 }
}
Export-ModuleMember -Function Resolve-BonsaiRustTool, Invoke-BonsaiTool
'@
$mock = $mock.Replace('__RESOLUTION__', $resolution.Replace("'", "''")).Replace('__RECEIPT__', $receipt.Replace("'", "''")).Replace('__TOOL__', (Join-Path $scratch 'bonsai-tools.exe').Replace("'", "''")).Replace('__LOCK__', $lockPath.Replace("'", "''"))
[IO.File]::WriteAllText((Join-Path $scripts 'Bonsai.Tools.psm1'), $mock)
$callerModules = @(Get-Module -Name Bonsai.Common, Bonsai.Tools -All)
# Forced imports of the copied modules must never replace the caller's modules
# or exported commands, including when a fixture assertion fails.
$fixtureShell = [PowerShell]::Create()
try {
    $invokeCopy = {
        param([Collections.IDictionary]$Parameters)
        $fixtureShell.Commands.Clear()
        $fixtureShell.Streams.ClearStreams()
        [void]$fixtureShell.AddCommand($copy)
        foreach ($name in $Parameters.Keys) { [void]$fixtureShell.AddParameter($name, $Parameters[$name]) }
        $invoked = $fixtureShell.Invoke()
        if ($fixtureShell.InvocationStateInfo.State -eq [Management.Automation.PSInvocationState]::Failed) {
            throw $fixtureShell.InvocationStateInfo.Reason
        }
        if ($fixtureShell.Streams.Error.Count -ne 0) { throw $fixtureShell.Streams.Error[0] }
        foreach ($record in $fixtureShell.Streams.Verbose) { Write-Verbose -Message $record.Message -Verbose }
        foreach ($record in $fixtureShell.Streams.Information) { Write-Information -MessageData $record.MessageData -InformationAction Continue }
        return $invoked
    }
    & $invokeCopy @{ DataDirectory = $data; NoSpeechModels = $true; NoAbliterateLora = $true; UseRepackedPtq1 = $true; ForceBuild = $true; ToolPath = (Join-Path $scratch 'selected.exe'); Verbose = $true }
    $request = ConvertFrom-Json -AsHashtable -InputObject ([IO.File]::ReadAllText($receipt))
    $resolved = ConvertFrom-Json -AsHashtable -InputObject ([IO.File]::ReadAllText($resolution))
    Assert ($request.RepositoryRoot -ceq $scratch -and $request.DataDirectory -ceq $data) 'Source/data roots were not forwarded.'
    Assert ($request.NoSpeechModels -eq $true -and $request.NoAbliterateLora -eq $true -and $request.UseRepackedPtq1 -eq $true -and $request.ForceBuild -eq $true) 'Switches did not become JSON booleans.'
    Assert (-not $request.Contains('ToolPath') -and -not $request.Contains('Verbose') -and -not $request.Contains('WorkingDirectory')) 'Bootstrap controls or session creation leaked into the native request.'
    Assert ($resolved.ToolPath -ceq (Join-Path $scratch 'selected.exe') -and $resolved.ForceBuild -eq $true) 'Tool override or ForceBuild was lost.'
    Assert ([IO.File]::Exists($lockPath)) 'The persistent probe sentinel was deleted.'
    & $invokeCopy @{ DataDirectory = $data; NoSpeechModels = $true; NoAbliterateLora = $true; NativeEngine = $true }
    $request = ConvertFrom-Json -AsHashtable -InputObject ([IO.File]::ReadAllText($receipt))
    Assert ($request.NativeEngine -eq $true -and $request.NoAbliterateLora -eq $true -and
        -not $request.Contains('UseRepackedPtq1')) 'The explicit Rust runtime selection was lost.'
    [IO.File]::Delete($receipt)
    [IO.File]::Delete($resolution)
    $guard = [IO.FileStream]::new($lockPath, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    try {
        & $invokeCopy @{ DataDirectory = $data; NoSpeechModels = $true }
        Assert (-not [IO.File]::Exists($receipt) -and -not [IO.File]::Exists($resolution)) 'A running-instance duplicate resolved or invoked the maintenance tool.'
        $models = & $invokeCopy @{ DataDirectory = $data; SpeechModelsOnly = $true }
        Assert ($models.TotalBytes -eq 1) 'The running-instance probe blocked speech-only maintenance.'
    } finally { $guard.Dispose() }
    & $invokeCopy @{ DataDirectory = $data; FrontendPort = 6173; NoSpeechModels = $true }
    Assert-Failure { & $invokeCopy @{ DataDirectory = $data; BackendPort = 6181; NoSpeechModels = $true } } 'Native launch failure was hidden.'
    & $invokeCopy @{ DataDirectory = $data; NoSpeechModels = $true }
    [IO.File]::Delete($lockPath)
    [void][IO.Directory]::CreateDirectory($lockPath)
    try { Assert-Failure { & $invokeCopy @{ DataDirectory = $data; NoSpeechModels = $true } } 'An unusable probe path was reported as a duplicate.' }
    finally { [IO.Directory]::Delete($lockPath) }
} finally {
    $fixtureShell.Dispose()
    $remainingModules = @(Get-Module -Name Bonsai.Common, Bonsai.Tools -All)
    Assert ($remainingModules.Count -eq $callerModules.Count) 'Copied launcher modules leaked into the caller.'
    foreach ($module in $callerModules) {
        Assert (@($remainingModules | Where-Object { [object]::ReferenceEquals($_, $module) }).Count -eq 1) 'A copied launcher replaced a caller module.'
    }
}
Write-Output "Native chat launch bridge checks passed. Evidence: $scratch"
