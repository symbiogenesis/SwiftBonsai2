#requires -Version 7.6
[CmdletBinding()]
[OutputType([string])]
param([Parameter(Mandatory)][ValidateSet('Digest', 'Arguments', 'All')][string]$Case)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.LoraEvaluation.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')


if ($Case -in @('Digest', 'All')) {
    Assert ((Get-BonsaiResponseDigest '') -ceq 'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855') 'UTF-8 digest changed.'
    Assert ((Get-BonsaiResponseDigest "answer`n") -cne (Get-BonsaiResponseDigest 'answer')) 'Response digest dropped trailing bytes.'
}
if ($Case -in @('Arguments', 'All')) {
    foreach ($arm in @('baseline', 'zero', 'adapter')) {
        $argsForArm = @(Get-BonsaiLoraEvaluationArgument -ModelPath 'C:\model & data\base.gguf' -AdapterPath 'bonsai-abliterate-lora.gguf' -Arm $arm -Prompt 'Write a short scene.')
        Assert ($argsForArm[1] -ceq 'C:\model & data\base.gguf') 'Model path must remain one argv entry.'
        Assert ($argsForArm -cnotcontains '--no-perf' -and $argsForArm -cnotcontains '--system-prompt' -and
            $argsForArm -ccontains '--single-turn' -and $argsForArm -ccontains '--no-log-prefix') 'Timing or Swift single-turn policy changed.'
        # Production sampler/effort defaults must not leak into the immutable
        # paired qualification controls or silently change its evidence identity.
        foreach ($flag in @('--temp', '--top-k', '--top-p', '--min-p', '--repeat-penalty', '--presence-penalty', '--frequency-penalty')) {
            Assert (@($argsForArm | Where-Object { $_ -ceq $flag }).Count -eq 1) "Duplicate qualification sampler: $flag."
        }
        Assert ($argsForArm -cnotcontains '--reasoning-effort' -and $argsForArm -cnotcontains '--reasoning-budget') 'Qualification inherited unrecorded reasoning overrides.'
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
