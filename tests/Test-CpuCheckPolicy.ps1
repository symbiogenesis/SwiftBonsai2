#requires -Version 7.6
[CmdletBinding()]
[OutputType([string])]
param([Parameter(Mandatory)][ValidateSet('Profiles')][string]$Case)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

if ($Case -ne 'Profiles') { throw 'Unsupported focused CPU check policy case.' }

# Extract only pure profile/argument helpers. No inventory, native executable,
# artifact probe, model execution or filesystem writes occur in this check.
$path = Join-Path $PSScriptRoot '../scripts/Invoke-CpuCheck.ps1'
$tokens = $null; $parseErrors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($path, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count) { throw ($parseErrors.Message -join '; ') }
foreach ($name in @('Get-CpuCheckPolicy', 'Get-CpuCheckArgument')) {
    $definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $false)
    if ($null -eq $definition) { throw "Missing CPU check policy helper: $name" }
    . ([scriptblock]::Create($definition.Extent.Text))
}
function Assert {
    [CmdletBinding()]
    [OutputType([void])]
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw $Message }
}
function Assert-Failure {
    [CmdletBinding()]
    [OutputType([void])]
    param([scriptblock]$Action, [string]$Message)
    $failed = $false
    try { & $Action | Out-Null } catch { $failed = $true }
    Assert $failed $Message
}
function Assert-ArgumentValue {
    [CmdletBinding()]
    [OutputType([void])]
    param([string[]]$Arguments, [string]$Name, [string]$Value)
    $index = [Array]::IndexOf($Arguments, $Name)
    Assert ($index -ge 0 -and $index + 1 -lt $Arguments.Count -and $Arguments[$index + 1] -ceq $Value) "Unexpected CPU argument for $Name."
}

$reference = Get-CpuCheckPolicy
Assert ($reference.Name -ceq 'reference-cpu' -and $reference.LockRelativePath -ceq '../config/runtime.lock.json') 'The default model profile changed.'
Assert ($reference.ServiceCeilingBytes -eq 18GB -and $reference.RequiredAvailableBytes -eq 22GB -and
    $reference.AvailableMemoryFloorBytes -eq 4GB) 'The reference memory admission changed.'
Assert ($reference.ContextTokens -eq 8192 -and $reference.BatchTokens -eq 256 -and $reference.MicrobatchTokens -eq 128) 'The reference context/batch policy changed.'
Assert ($reference.EstimatedKnownSubtotalBytes -eq 13840157600 -and
    $reference.EstimatedRuntimeHeadroomBytes -eq 5487195232) 'The reference memory accounting changed.'

$swift = Get-CpuCheckPolicy -Profile swift-pq2-assessment
Assert ($swift.LockRelativePath -ceq '../config/swift-assessment.lock.json') 'Swift assessment uses the serving lock.'
Assert ($swift.ServiceCeilingBytes -eq 12GB -and $swift.RequiredAvailableBytes -eq 16GB -and
    $swift.AvailableMemoryFloorBytes -eq 4GB) 'Swift assessment weakened the physical reserve or changed its fixed allowance.'
Assert ($swift.EstimatedWeightBytes -eq 7206168928 -and $swift.EstimatedKvBytes -eq 536870912 -and
    $swift.EstimatedRecurrentBytes -eq 156893184 -and $swift.EstimatedTransformBytes -eq 4423680 -and
    $swift.EstimatedTransformConstructionTemporaryBytes -eq 4MB) 'Swift assessment omits known model/cache/transform costs.'
Assert ($swift.EstimatedKnownSubtotalBytes -eq 7904356704 -and
    $swift.EstimatedRuntimeHeadroomBytes -eq 4980545184) 'Swift assessment headroom is inconsistent with its known subtotal.'
$withAdapter = Get-CpuCheckPolicy -Profile swift-pq2-assessment -UseAbliterateLora
Assert ($withAdapter.EstimatedAdapterBytes -eq 9682464 -and $withAdapter.EstimatedKnownSubtotalBytes -eq 7914039168 -and
    $withAdapter.ServiceCeilingBytes -eq 12GB -and $withAdapter.RequiredAvailableBytes -eq 16GB) 'Adapter allowance changed the fixed safety policy or omitted adapter bytes.'
Assert ($swift.EstimatedAdapterBytes -eq 0 -and $reference.EstimatedAdapterBytes -eq 0) 'An adapter was silently enabled.'

# Native ArgumentList must receive a model path as exactly one argument, even
# when the path includes spaces or shell metacharacters.
$model = 'C:\external model & fixtures\Swift-Bonsai-2-PQ2_0.gguf'
$defaultArguments = @(Get-CpuCheckArgument -ModelPath $model)
Assert-ArgumentValue -Arguments $defaultArguments -Name '--batch-size' -Value '256'
Assert-ArgumentValue -Arguments $defaultArguments -Name '--ubatch-size' -Value '128'
Assert-ArgumentValue -Arguments $defaultArguments -Name '--log-verbosity' -Value '2'
Assert-ArgumentValue -Arguments $defaultArguments -Name '--system-prompt' -Value 'You are a helpful assistant.'
$swiftArguments = @(Get-CpuCheckArgument -ModelPath $model -Profile swift-pq2-assessment)
Assert-ArgumentValue -Arguments $swiftArguments -Name '--batch-size' -Value '128'
Assert-ArgumentValue -Arguments $swiftArguments -Name '--ubatch-size' -Value '64'
Assert-ArgumentValue -Arguments $swiftArguments -Name '--log-verbosity' -Value '3'
# The pinned Swift template rejects system-only messages. Completion applies an
# explicit system message before appending the user, causing an uncaught native
# template exception. The synthetic Swift check must start with its user query.
Assert ($swiftArguments -cnotcontains '--system-prompt') 'Swift completion would format a rejected system-only message.'
Assert-ArgumentValue -Arguments $swiftArguments -Name '--prompt' -Value 'Reply with the word ready.'
$adapterPath = 'C:\external model & fixtures\bonsai-abliterate-lora.gguf'
$adapterArguments = @(Get-CpuCheckArgument -ModelPath $model -Profile swift-pq2-assessment -UseAbliterateLora -AdapterPath $adapterPath)
Assert-ArgumentValue -Arguments $adapterArguments -Name '--lora' -Value $adapterPath
Assert ($swiftArguments -cnotcontains '--lora' -and $defaultArguments -cnotcontains '--lora') 'Default CPU arguments include an adapter.'
Assert-Failure { Get-CpuCheckPolicy -UseAbliterateLora } 'Swift adapter was accepted for Qwen.'
Assert-Failure { Get-CpuCheckArgument -ModelPath $model -Profile swift-pq2-assessment -UseAbliterateLora } 'Unverified empty adapter path reached native arguments.'
Assert-Failure { Get-CpuCheckArgument -ModelPath $model -Profile swift-pq2-assessment -AdapterPath $adapterPath } 'Adapter path silently enabled an adapter.'
foreach ($arguments in @($defaultArguments, $swiftArguments)) {
    Assert-ArgumentValue -Arguments $arguments -Name '--model' -Value $model
    Assert-ArgumentValue -Arguments $arguments -Name '--ctx-size' -Value '8192'
    Assert-ArgumentValue -Arguments $arguments -Name '--parallel' -Value '1'
    Assert-ArgumentValue -Arguments $arguments -Name '--cache-type-k' -Value 'f16'
    Assert-ArgumentValue -Arguments $arguments -Name '--cache-type-v' -Value 'f16'
    Assert-ArgumentValue -Arguments $arguments -Name '--gpu-layers' -Value '0'
    Assert-ArgumentValue -Arguments $arguments -Name '--device' -Value 'none'
    Assert-ArgumentValue -Arguments $arguments -Name '--fit' -Value 'off'
    Assert-ArgumentValue -Arguments $arguments -Name '--load-mode' -Value 'mmap'
    Assert-ArgumentValue -Arguments $arguments -Name '--n-predict' -Value '16'
    foreach ($flag in @('--no-repack', '--no-context-shift', '--offline', '--no-warmup', '--no-perf', '--single-turn')) {
        Assert ($arguments -ccontains $flag) "The CPU check omitted $flag."
    }
    foreach ($flag in @('--spec-type', '--ctx-checkpoints', '--cache-ram')) {
        Assert ($arguments -cnotcontains $flag) "The CPU check passed an unsupported completion flag: $flag."
    }
}
foreach ($policy in @($reference, $swift)) {
    Assert ($policy.TokenLimit -eq 16 -and $policy.TimeoutSeconds -eq 600) 'The bounded correctness workload changed.'
}
Assert-Failure { Get-CpuCheckPolicy -Profile unsupported } 'An unknown CPU profile was accepted.'
Assert-Failure { Get-CpuCheckArgument -ModelPath $model -Profile unsupported } 'An unknown profile reached native arguments.'
Write-Output 'CPU check profile policy checks passed.'
