#requires -Version 7.6
[CmdletBinding()]
[OutputType([string])]
param(
    [Parameter(Mandatory)][ValidateSet('Arguments', 'All')][string]$Case,
    [string]$ToolPath
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if ($Case -eq 'All' -and $env:GITHUB_ACTIONS -ne 'true') { throw 'Select the focused Arguments case locally.' }
Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.Tools.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')

# Rust owns the pure argument regressions. Exercise the packaged bridge with
# independently frozen control identities for both approved representations.
$fixture = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot 'fixtures/lora-qualification-v1.json') | ConvertFrom-Json -AsHashtable -DateKind String
$model = 'C:\model & data\base.gguf'
foreach ($vector in $fixture.controlVectors) {
    $result = Invoke-BonsaiTool -Command qualification -ToolPath $ToolPath -Request @{
        operation = 'invocations'; controls = $vector.controls; modelPath = $model
    }
    Assert ($result.ControlsSha256 -ceq $vector.sha256 -and $result.Samples.Count -eq 28) 'Packaged invocations changed the recorded control identity or sample count.'
    Assert ($result.Samples[0].id -ceq $fixture.plan.Schedule[0].sampleId) 'Packaged invocation order changed.'
    foreach ($sample in $result.Samples) {
        Assert ($sample.arguments[1] -ceq $model -and $sample.arguments -cnotcontains '--reasoning-effort') 'The bridge changed an argv token or inherited production reasoning settings.'
    }
}
$plan = & (Join-Path $PSScriptRoot '../scripts/Invoke-LoraQualification.ps1') -DataDirectory ([IO.Path]::GetTempPath()) -PlanOnly -UseRepackedPtq1 -ToolPath $ToolPath
Assert (-not $plan.ModelProcessStarted -and $plan.PairedSamples -eq 24 -and $plan.ZeroScaleSamples -eq 4 -and
    $plan.ServiceCeilingBytes -eq 12348030976 -and $plan.PhysicalReserveBytes -eq 4GB -and $plan.PerSampleTimeoutSeconds -eq 180) 'The nonlaunching wrapper lost the shared capture policy.'
'Focused packaged LoRA invocation checks passed.'
