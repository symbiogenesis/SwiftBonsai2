#requires -Version 7.6
<# .SYNOPSIS
Checks the nonlaunching PTQ1 screen bridge and local capture rejection.
#>
[CmdletBinding()]
[OutputType([string])]
param([Parameter(Mandatory)][ValidateSet('Plan', 'LocalRejection', 'WorkerPreflight', 'All')][string]$Case, [string]$ToolPath)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if ($Case -ceq 'All' -and $env:GITHUB_ACTIONS -cne 'true') { throw 'Select one focused case locally.' }
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
$script = Join-Path $PSScriptRoot '../scripts/Invoke-Ptq1Screen.ps1'
if ($Case -cin @('Plan', 'All')) {
    $plan = & $script -DataDirectory ([IO.Path]::GetTempPath()) -PlanOnly -ToolPath $ToolPath
    Assert ($plan.TimedSamples -eq 12 -and $plan.ZeroScaleSamples -eq 2 -and $plan.Schedule.Count -eq 12) 'The small screen sample budget changed.'
    Assert ($plan.Contract.controls.maxOutputTokens -eq 64 -and $plan.Contract.adapterScale -eq 1.3 -and
        $plan.Contract.repeatCount -eq 3) 'The declared screen output cap, adapter scale or repetitions changed.'
    Assert (-not $plan.DefaultMayChange -and -not $plan.StandardProfileMayEnable) 'A screen plan authorized promotion.'
}
if ($Case -cin @('LocalRejection', 'All') -and $env:GITHUB_ACTIONS -cne 'true') {
    Assert-Failure { & $script -DataDirectory ([IO.Path]::GetTempPath()) -ToolPath $ToolPath } 'Local execution reached a model timing capture.'
}
if ($Case -cin @('WorkerPreflight', 'All')) {
    # Execute the real bridge against model-free fixture dependencies, preserving
    # its CI/device guards and retaining its ordinary failure evidence.
    $fixture = Join-Path ([IO.Path]::GetTempPath()) ('Bonsai-Ptq1Preflight-' + [guid]::NewGuid().ToString('N'))
    [void][IO.Directory]::CreateDirectory($fixture)
    Copy-Item -LiteralPath $script -Destination (Join-Path $fixture 'Invoke-Ptq1Screen.ps1')
    @'
function Resolve-ExternalPath { param([string]$Path) return $Path }
function Read-BonsaiLock { param([string]$Path) return @{ runtime = @{} } }
Export-ModuleMember -Function Resolve-ExternalPath, Read-BonsaiLock
'@ | Set-Content -LiteralPath (Join-Path $fixture 'Bonsai.Common.psm1') -Encoding utf8
    @'
function Invoke-BonsaiTool {
    param([string]$Command, [string]$ToolPath, [Collections.IDictionary]$Request, [int]$TimeoutSeconds)
    switch ($Command) {
        'cpu-check-plan' {
            'Plan' | Add-Content -LiteralPath $env:BONSAI_PREFLIGHT_FIXTURE_EVENTS
            return @{ Policy = @{ RequiredAvailableBytes = 16642998272; AvailableMemoryFloorBytes = 4294967296 } }
        }
        'completion' {
            'Preflight' | Add-Content -LiteralPath $env:BONSAI_PREFLIGHT_FIXTURE_EVENTS
            $Request | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $env:BONSAI_PREFLIGHT_FIXTURE_REQUEST
            if ($env:BONSAI_PREFLIGHT_FIXTURE_MODE -ceq 'failed') { throw 'Fixture worker startup failure.' }
            return @{ Status = 'completed' }
        }
        'qualification' {
            'Capture' | Add-Content -LiteralPath $env:BONSAI_PREFLIGHT_FIXTURE_EVENTS
            throw 'Fixture capture boundary reached; no model executed.'
        }
        default { throw 'Unexpected fixture tool command.' }
    }
}
Export-ModuleMember -Function Invoke-BonsaiTool
'@ | Set-Content -LiteralPath (Join-Path $fixture 'Bonsai.Tools.psm1') -Encoding utf8
    @'
param([string]$DataDirectory, [string]$LockPath, [switch]$VerifyOnly, [string]$ToolPath)
'Runtime' | Add-Content -LiteralPath $env:BONSAI_PREFLIGHT_FIXTURE_EVENTS
@{ Commit = 'fixture-runtime'; Runtime = @{ runtimeDirectory = $DataDirectory } }
'@ | Set-Content -LiteralPath (Join-Path $fixture 'Get-Engine.ps1') -Encoding utf8
    @'
param([string]$DataDirectory, [string]$Profile, [switch]$UseAbliterateLora, [switch]$UseRepackedPtq1, [switch]$CheckOnly, [string]$ToolPath)
'Admission' | Add-Content -LiteralPath $env:BONSAI_PREFLIGHT_FIXTURE_EVENTS
@{ Status = 'admission-passed' }
'@ | Set-Content -LiteralPath (Join-Path $fixture 'Invoke-CpuCheck.ps1') -Encoding utf8
    @'
param([string]$DataDirectory, [switch]$VerifyOnly)
'Adapter' | Add-Content -LiteralPath $env:BONSAI_PREFLIGHT_FIXTURE_EVENTS
@{}
'@ | Set-Content -LiteralPath (Join-Path $fixture 'Get-Adapter.ps1') -Encoding utf8
    function Get-CimInstance {
        <# .SYNOPSIS
        Supplies the fixture's already-identified native Snapdragon device.
        #>
        [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidOverwritingBuiltInCmdlets', '', Justification = 'This scoped test seam supplies a fixed processor identity and is removed in finally.')]
        [CmdletBinding()]
        [OutputType([pscustomobject])]
        param([string]$ClassName)
        Assert ($ClassName -ceq 'Win32_Processor') 'Unexpected fixture CIM query.'
        return [pscustomobject]@{ Name = 'Snapdragon X2 Elite Extreme - fixture' }
    }
    $names = @('GITHUB_ACTIONS', 'GITHUB_REF', 'GITHUB_REPOSITORY', 'RUNNER_ARCH', 'GITHUB_RUN_ID',
        'RUNNER_NAME', 'GITHUB_SHA', 'GITHUB_RUN_ATTEMPT', 'GITHUB_ENV', 'ACTIONS_RUNTIME_TOKEN',
        'ACTIONS_ID_TOKEN_REQUEST_TOKEN', 'GITHUB_TOKEN', 'GH_TOKEN', 'ACTIONS_RUNNER_INPUT_TOKEN',
        'BONSAI_PREFLIGHT_FIXTURE_EVENTS', 'BONSAI_PREFLIGHT_FIXTURE_REQUEST', 'BONSAI_PREFLIGHT_FIXTURE_MODE')
    $saved = @{}
    foreach ($name in $names) { $saved[$name] = [Environment]::GetEnvironmentVariable($name) }
    try {
        $env:GITHUB_ACTIONS = 'true'; $env:GITHUB_REF = 'refs/heads/main'
        $env:GITHUB_REPOSITORY = 'symbiogenesis/SwiftBonsai2Runner'; $env:RUNNER_ARCH = 'ARM64'
        $env:GITHUB_RUN_ID = '123'; $env:RUNNER_NAME = 'fixture'; $env:GITHUB_SHA = 'a' * 40; $env:GITHUB_RUN_ATTEMPT = '1'
        foreach ($name in @('ACTIONS_RUNTIME_TOKEN', 'ACTIONS_ID_TOKEN_REQUEST_TOKEN', 'GITHUB_TOKEN', 'GH_TOKEN', 'ACTIONS_RUNNER_INPUT_TOKEN')) {
            Remove-Item -LiteralPath ('Env:' + $name) -ErrorAction SilentlyContinue
        }
        foreach ($mode in @('failed', 'passed', 'credential')) {
            $env:BONSAI_PREFLIGHT_FIXTURE_MODE = $mode
            $env:BONSAI_PREFLIGHT_FIXTURE_EVENTS = Join-Path $fixture ($mode + '-events.txt')
            $env:BONSAI_PREFLIGHT_FIXTURE_REQUEST = Join-Path $fixture ($mode + '-request.json')
            $env:GITHUB_ENV = Join-Path $fixture ($mode + '-environment.txt')
            if ($mode -ceq 'credential') { $env:ACTIONS_RUNTIME_TOKEN = 'fixture-only' }
            $failure = Assert-Failure -PassThru { & (Join-Path $fixture 'Invoke-Ptq1Screen.ps1') -DataDirectory $fixture }
            if ($mode -ceq 'credential') {
                Assert ($failure -like '*credentials omitted*' -and -not [IO.File]::Exists($env:BONSAI_PREFLIGHT_FIXTURE_EVENTS)) 'Credential rejection performed runtime or model work.'
                continue
            }
            Assert ([IO.File]::Exists($env:BONSAI_PREFLIGHT_FIXTURE_EVENTS)) ('Fixture stopped before runtime verification: ' + $failure)
            $events = @(Get-Content -LiteralPath $env:BONSAI_PREFLIGHT_FIXTURE_EVENTS)
            $evidence = (Get-Content -LiteralPath $env:GITHUB_ENV).Substring('BONSAI_PTQ1_EVIDENCE='.Length)
            $receipt = Get-Content -LiteralPath (Join-Path $evidence 'worker-preflight.json') -Raw | ConvertFrom-Json
            $request = Get-Content -LiteralPath $env:BONSAI_PREFLIGHT_FIXTURE_REQUEST -Raw | ConvertFrom-Json
            Assert ($request.arguments.Count -eq 1 -and $request.arguments[0] -ceq '--version' -and $request.artifacts.Count -eq 0) 'Startup preflight selected model inputs.'
            Assert ($request.memoryLimitBytes -eq 536870912 -and $request.timeoutSeconds -eq 10 -and $request.stdoutLimitBytes -eq 65536 -and $request.stderrLimitBytes -eq 65536) 'Startup preflight lost its fixed process/output bounds.'
            Assert ($request.requiredAvailableBytes -eq 16642998272 -and $request.physicalReserveBytes -eq 4294967296) 'Startup preflight weakened admission or the physical reserve.'
            if ($mode -ceq 'failed') {
                Assert (($events -join ',') -ceq 'Runtime,Plan,Preflight' -and $receipt.Status -ceq 'failed' -and -not $receipt.ModelTrialsRequested) 'Failed startup reached model admission or capture.'
            } else {
                Assert (($events -join ',') -ceq 'Runtime,Plan,Preflight,Admission,Admission,Adapter,Capture' -and $receipt.Status -ceq 'passed' -and $receipt.ModelTrialsRequested) 'Passing startup did not precede both admissions and capture.'
            }
            Assert (-not $receipt.ModelLoaded -and -not $receipt.DefaultMayChange) 'Startup evidence authorized a model/profile claim.'
        }
    } finally {
        foreach ($name in $names) {
            if ($null -eq $saved[$name]) { Remove-Item -LiteralPath ('Env:' + $name) -ErrorAction SilentlyContinue }
            else { [Environment]::SetEnvironmentVariable($name, $saved[$name]) }
        }
        Remove-Item Function:Get-CimInstance
    }
}
'Focused PTQ1 screen bridge checks passed.'
