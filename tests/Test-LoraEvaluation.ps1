#requires -Version 7.6
[CmdletBinding()]
[OutputType([string])]
param([Parameter(Mandatory)][ValidateSet('Timing', 'Arguments', 'All')][string]$Case)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.LoraEvaluation.psm1') -Force
function Assert {
    [CmdletBinding()]
    [OutputType([void])]
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw $Message }
}
function Assert-Failure {
    [CmdletBinding()]
    [OutputType([void])]
    param([scriptblock]$Action)
    $failed = $false
    try { & $Action | Out-Null } catch { $failed = $true }
    Assert $failed 'Expected invalid evaluation input to fail.'
}
if ($Case -in @('Timing', 'All')) {
    $log = @'
main: number of tokens in prompt = 80
common_perf_print:    samplers time =       5.00 ms /   208 tokens
common_perf_print: prompt eval time =     125.50 ms /    80 tokens ( 1.57 ms per token, 637.45 tokens per second)
common_perf_print:        eval time =    1000.25 ms /   127 runs   ( 7.88 ms per token, 126.97 tokens per second)
'@
    $oldCulture = [Threading.Thread]::CurrentThread.CurrentCulture
    try {
        [Threading.Thread]::CurrentThread.CurrentCulture = [Globalization.CultureInfo]::GetCultureInfo('fr-FR')
        $timing = Read-BonsaiCompletionTiming $log
        Assert ($timing.DecodeTokens -eq 127 -and $timing.DecodeMilliseconds -eq 1000.25 -and
            $timing.PromptTokens -eq 80 -and $timing.PromptMilliseconds -eq 125.5) 'Timing parse changed under a comma-decimal culture.'
        Assert ([Math]::Abs($timing.DecodeTokensPerSecond - (127000 / 1000.25)) -lt 0.000001) 'Throughput used the rounded printed rate.'
        $counts = Read-BonsaiCompletionTokenCount -Text $log -Timing $timing
        Assert ($counts.GeneratedSampledTokens -eq 128 -and $counts.SamplerAcceptedTokens -eq 208 -and
            $counts.PromptTokenCount -eq 80) 'Sampler accepts include prompt tokens and the final undecoded sample.'
        Assert-Failure { Read-BonsaiCompletionTokenCount -Text ($log.Replace('208 tokens', '207 tokens')) -Timing $timing }
        Assert-Failure { Read-BonsaiCompletionTokenCount -Text ($log + "`nmain: number of tokens in prompt = 80") -Timing $timing }
        Assert-Failure { Read-BonsaiCompletionTiming ($log + "`n" + $log) }
        Assert-Failure { Read-BonsaiCompletionTiming ($log.Replace('1000.25', 'NaN')) }
        Assert-Failure { Read-BonsaiCompletionTiming ($log.Replace('1000.25', '0.00')) }
        Assert-Failure { Read-BonsaiCompletionTiming ($log.Replace('127 runs', '513 runs')) }
        Assert-Failure { Read-BonsaiCompletionTiming ($log.Replace('80 tokens', '0 tokens')) }
        Assert-Failure { Read-BonsaiCompletionTiming ($log.Replace('common_perf_print:', 'llama_perf_context_print:')) }
    } finally { [Threading.Thread]::CurrentThread.CurrentCulture = $oldCulture }
    Assert ((Get-BonsaiResponseDigest '') -ceq 'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855') 'UTF-8 digest changed.'
    Assert ((Get-BonsaiResponseDigest "answer`n") -cne (Get-BonsaiResponseDigest 'answer')) 'Response digest dropped trailing bytes.'
}
if ($Case -in @('Arguments', 'All')) {
    foreach ($arm in @('baseline', 'zero', 'adapter')) {
        $argsForArm = @(Get-BonsaiLoraEvaluationArgument -ModelPath 'C:\model & data\base.gguf' -AdapterPath 'bonsai-abliterate-lora.gguf' -Arm $arm -Prompt 'Write a short scene.')
        Assert ($argsForArm[1] -ceq 'C:\model & data\base.gguf') 'Model path must remain one argv entry.'
        Assert ($argsForArm -cnotcontains '--no-perf' -and $argsForArm -cnotcontains '--system-prompt' -and
            $argsForArm -ccontains '--single-turn' -and $argsForArm -ccontains '--no-log-prefix') 'Timing or Swift single-turn policy changed.'
        $index = [Array]::IndexOf($argsForArm, '--lora-scaled')
        if ($arm -eq 'baseline') { Assert ($index -eq -1) 'Baseline silently enabled the adapter.' }
        else {
            $scale = if ($arm -eq 'zero') { '0' } else { '1' }
            Assert ($index -ge 0 -and $argsForArm[$index + 1] -ceq ('bonsai-abliterate-lora.gguf:' + $scale)) 'Pinned parser needs one relative FNAME:SCALE argument.'
        }
    }
    Assert-Failure { Get-BonsaiLoraEvaluationArgument -ModelPath 'base.gguf' -AdapterPath 'C:\data\adapter.gguf' -Arm zero -Prompt 'Write.' }
    Assert-Failure { Get-BonsaiLoraEvaluationArgument -ModelPath 'base.gguf' -AdapterPath 'bonsai-abliterate-lora.gguf' -Arm baseline -Prompt ('x' * 4097) }
}
'Focused LoRA evaluation contract checks passed.'
