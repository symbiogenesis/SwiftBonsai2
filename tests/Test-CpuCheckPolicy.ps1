#requires -Version 7.6
[CmdletBinding()]
[OutputType([string])]
param(
    [Parameter(Mandatory)][ValidateSet('Profiles', 'Artifacts')][string]$Case,
    [string]$ToolPath
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.Tools.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')

# Pure policy and hostile receipt regressions live in bonsai-cpu-policy. These
# focused checks exercise the packaged JSON/PowerShell boundary without models.
$data = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Unprovisioned-' + [guid]::NewGuid().ToString('N'))
if ($Case -eq 'Profiles') {
    foreach ($selectedProfile in @('reference-cpu', 'swift-pq2-assessment')) {
        $report = & (Join-Path $PSScriptRoot '../scripts/Invoke-CpuCheck.ps1') -DataDirectory $data -Profile $selectedProfile -CheckOnly -ToolPath $ToolPath
        Assert (-not $report.ModelProcessStarted -and $report.AvailablePhysicalBytesAtAdmission -gt 0) 'Admission planning did not return nonlaunching memory facts.'
        Assert ($report.Profile -ceq $selectedProfile -and $report.EffectiveArguments[1] -ceq $report.ModelPath) 'The wrapper changed the selected profile or model argv token.'
        Assert ($report.ModelPath.StartsWith($data + '\') -and $null -eq $report.AdapterPath) 'Planning selected unexpected storage or an implicit adapter.'
    }
} else {
    $report = & (Join-Path $PSScriptRoot '../scripts/Invoke-CpuCheck.ps1') -DataDirectory $data -Profile swift-pq2-assessment -UseRepackedPtq1 -UseAbliterateLora -CheckOnly -ToolPath $ToolPath
    Assert ($report.WeightRepresentation -ceq 'current-repacked-PTQ1_0' -and $report.EstimatedWeightBytes -eq 5946648928) 'The derived selection was lost at the JSON boundary.'
    $index = [Array]::IndexOf($report.EffectiveArguments, '--lora')
    Assert ($index -ge 0 -and $report.EffectiveArguments[$index + 1] -ceq $report.AdapterPath -and $report.AdapterRequested) 'The verified adapter selection was lost at the JSON boundary.'
    Assert-Failure {
        Invoke-BonsaiTool -Command cpu-check-plan -ToolPath $ToolPath -Request @{
            operation = 'plan'; dataDirectory = $data; profile = 'reference-cpu'
            useAbliterateLora = $true; useRepackedPtq1 = $false
        }
    } 'The packaged command accepted a Swift adapter for the reference profile.'
}
Assert (-not (Test-Path -LiteralPath $data)) 'Admission planning created model/runtime storage.'
'Focused packaged CPU policy checks passed.'
