#requires -Version 7.6
<# .SYNOPSIS
Checks candidate identity rejection, fixed model-free arguments and aggregate admission.
#>
[CmdletBinding()]
[OutputType([string])]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.Common.psm1')
$script = Join-Path $PSScriptRoot '../scripts/Invoke-HeadlessPublisherStartup.ps1'
$tokens = $null
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($script, [ref]$tokens, [ref]$errors)
Assert ($errors.Count -eq 0) 'Candidate startup has a PowerShell syntax error.'
foreach ($name in @('Get-HeadlessStartupPlan', 'Get-HeadlessStartupAllowance')) {
    $definition = $ast.Find({ param($node)
        $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq $name
    }, $true)
    Assert ($null -ne $definition) 'A candidate startup boundary is missing.'
    . ([scriptblock]::Create($definition.Extent.Text))
}
$scratch = Resolve-ExternalPath (Join-Path ([IO.Path]::GetTempPath()) ('Bonsai-StartupPlan-' + [guid]::NewGuid().ToString('N')))
[void][IO.Directory]::CreateDirectory($scratch)
$files = @('llama-completion.exe', 'llama-server.exe', 'llama-bench.exe', 'test-headless-argv.exe', 'ggml.dll')
$pins = @($files | ForEach-Object {
    $path = Join-Path $scratch $_
    [IO.File]::WriteAllBytes($path, [byte[]]@(1, 2, 3))
    [pscustomobject]@{ Name=$_; Bytes=3; Sha256=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() }
})
try {
    $plan = Get-HeadlessStartupPlan -Bundle $scratch -PackageFiles $pins
    Assert ($plan.Artifacts.Count -eq $pins.Count) 'Supervision must verify every pinned candidate file.'
    for ($index=0; $index -lt $pins.Count; $index++) {
        Assert ($plan.Artifacts[$index].sha256 -ceq $pins[$index].Sha256 -and $plan.Artifacts[$index].bytes -eq $pins[$index].Bytes) 'Candidate identity was weakened before native verification.'
    }
    $evidence = Get-Content -LiteralPath (Join-Path $PSScriptRoot '../docs/evidence/headless-publisher-startup.json') -Raw | ConvertFrom-Json -DateKind String
    Assert ($plan.Cases.Count -eq 10) 'All ten hosted model-free probes must remain requested.'
    for ($index=0; $index -lt $plan.Cases.Count; $index++) {
        $case = $plan.Cases[$index]
        $prior = $evidence.results.probes[$index]
        Assert ($case.Name -ceq $prior.name -and
            (ConvertTo-Json -InputObject $case.Arguments -Compress) -ceq (ConvertTo-Json -InputObject $prior.arguments -Compress)) 'Owner startup arguments differ from the retained hosted fixture.'
    }
    Assert-Failure -Action { Get-HeadlessStartupPlan -Bundle $scratch -PackageFiles (@($pins) + @($pins[0])) } -Message 'Duplicate pins were accepted.'
    $pins[0].Name='../llama-completion.exe'
    Assert-Failure -Action { Get-HeadlessStartupPlan -Bundle $scratch -PackageFiles $pins } -Message 'A redirected package filename was accepted.'
    $pins[0].Name=$files[0]
    [IO.File]::WriteAllBytes((Join-Path $scratch 'unreviewed.dll'), [byte[]]@(4))
    Assert-Failure -Action { Get-HeadlessStartupPlan -Bundle $scratch -PackageFiles $pins } -Message 'An additional unpinned dependency was accepted.'
    Remove-Item -LiteralPath (Join-Path $scratch 'unreviewed.dll')
    [IO.File]::WriteAllBytes((Join-Path $scratch $files[0]), [byte[]]@(4))
    Assert-Failure -Action { Get-HeadlessStartupPlan -Bundle $scratch -PackageFiles $pins } -Message 'Changed candidate length was accepted.'
    Assert-Failure -Action { Get-HeadlessStartupPlan -Bundle $scratch -PackageFiles @($pins | Select-Object -Skip 1) } -Message 'A missing executable pin was accepted.'
    $clock = [Diagnostics.Stopwatch]::StartNew()
    Assert ((Get-HeadlessStartupAllowance -Clock $clock -BudgetSeconds 60) -le 50) 'Setup was not charged or reporting time was not reserved.'
    Assert-Failure -Action { Get-HeadlessStartupAllowance -Clock $clock -BudgetSeconds 30 } -Message 'A new probe was admitted without room for its deadline and teardown.'
} finally {
    # Only the newly created flat fixture files are removed; no recursive delete.
    foreach ($name in $files) { Remove-Item -LiteralPath (Join-Path $scratch $name) -ErrorAction SilentlyContinue }
    Remove-Item -LiteralPath (Join-Path $scratch 'unreviewed.dll') -ErrorAction SilentlyContinue
    [IO.Directory]::Delete($scratch)
}
Write-Output 'Pinned candidate, model-free argument parity and aggregate budget checks passed without executable targets or models.'
