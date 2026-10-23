#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

function Get-BonsaiLoraEvaluationArgument {
    <# .SYNOPSIS
    Builds the fixed native single-turn argv for one paired evaluation arm.
    #>
    [CmdletBinding()]
    [OutputType([string], [array])]
    param(
        [Parameter(Mandatory)][string]$ModelPath,
        [Parameter(Mandatory)][string]$AdapterPath,
        [Parameter(Mandatory)][ValidateSet('baseline', 'zero', 'adapter')][string]$Arm,
        [Parameter(Mandatory)][string]$Prompt
    )
    if ([string]::IsNullOrWhiteSpace($ModelPath) -or [string]::IsNullOrWhiteSpace($AdapterPath) -or
        [string]::IsNullOrWhiteSpace($Prompt) -or $Prompt.Length -gt 4096 -or $Prompt.Contains([char]0)) {
        throw 'Evaluation requires verified artifact paths and one bounded fixed prompt.'
    }
    $arguments = @('--model', $ModelPath, '--ctx-size', '8192', '--parallel', '1',
        '--cache-type-k', 'f16', '--cache-type-v', 'f16', '--gpu-layers', '0', '--device', 'none',
        '--no-kv-offload', '--no-op-offload', '--fit', 'off', '--threads', '4', '--threads-batch', '4',
        '--poll', '0', '--poll-batch', '0', '--batch-size', '128', '--ubatch-size', '64',
        '--load-mode', 'mmap', '--no-repack', '--no-context-shift', '--offline', '--no-warmup',
        '--perf', '--log-verbosity', '3', '--no-log-prefix', '--no-log-timestamps', '--log-colors', 'off',
        '--color', 'off', '--simple-io', '--no-display-prompt', '--verbose-prompt',
        '--conversation', '--jinja', '--single-turn', '--n-predict', '512',
        '--seed', '42', '--temp', '0', '--top-k', '1', '--top-p', '1', '--min-p', '0', '--repeat-penalty', '1',
        '--presence-penalty', '0', '--frequency-penalty', '0')
    # The pinned parser splits FNAME:SCALE on every colon. The caller supplies
    # a verified scratch copy's simple relative filename, never a drive path.
    if ($AdapterPath -cne 'bonsai-abliterate-lora.gguf') { throw 'Use the verified adapter copy in the native working directory.' }
    if ($Arm -eq 'zero') { $arguments += @('--lora-scaled', ($AdapterPath + ':0')) }
    elseif ($Arm -eq 'adapter') { $arguments += @('--lora-scaled', ($AdapterPath + ':1')) }
    $arguments + @('--prompt', $Prompt)
}

function Read-BonsaiCompletionTiming {
    <# .SYNOPSIS
    Reads the unique pinned native prompt and single-token decode timing records.
    .DESCRIPTION
    The fixed Prism context logger reports decode eval runs, not total sampled
       tokens. In particular, the first sampled token follows prompt evaluation.
       Do not rename this denominator to generated tokens or use wall time. #>
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([Parameter(Mandatory)][string]$Text)
    $number = '[0-9]+(?:\.[0-9]+)?'
    $tail = '[ \t]+\([ \t]*' + $number + ' ms per token,[ \t]*' + $number + ' tokens per second\)\r?$'
    $evalPattern = '(?m)^common_perf_print:[ \t]+eval time[ \t]*=[ \t]*(?<ms>' + $number + ') ms[ \t]*/[ \t]*(?<count>[0-9]+) runs' + $tail
    $promptPattern = '(?m)^common_perf_print:[ \t]+prompt eval time[ \t]*=[ \t]*(?<ms>' + $number + ') ms[ \t]*/[ \t]*(?<count>[0-9]+) tokens' + $tail
    $evalMatches = [regex]::Matches($Text, $evalPattern)
    $promptMatches = [regex]::Matches($Text, $promptPattern)
    if ($evalMatches.Count -ne 1 -or $promptMatches.Count -ne 1) { throw 'Expected exactly one complete prompt and decode timing record from the pinned logger.' }
    $culture = [Globalization.CultureInfo]::InvariantCulture
    $milliseconds = [double]::Parse($evalMatches[0].Groups['ms'].Value, $culture)
    $count = [int]::Parse($evalMatches[0].Groups['count'].Value, $culture)
    $promptMilliseconds = [double]::Parse($promptMatches[0].Groups['ms'].Value, $culture)
    $promptTokens = [int]::Parse($promptMatches[0].Groups['count'].Value, $culture)
    if (-not [double]::IsFinite($milliseconds) -or $milliseconds -le 0 -or $count -lt 1 -or $count -gt 512 -or
        -not [double]::IsFinite($promptMilliseconds) -or $promptMilliseconds -le 0 -or $promptTokens -lt 1 -or $promptTokens -gt 8192) {
        throw 'Invalid or out-of-contract native timing counts.'
    }
    [pscustomobject]@{ DecodeTokens = $count; DecodeMilliseconds = $milliseconds
        DecodeTokensPerSecond = 1000 * $count / $milliseconds
        PromptTokens = $promptTokens; PromptMilliseconds = $promptMilliseconds }
}

function Read-BonsaiCompletionTokenCount {
    <# .SYNOPSIS
    Cross-checks fresh-completion counters and separates sampled and eval tokens.
    #>
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([Parameter(Mandatory)][string]$Text, [Parameter(Mandatory)][pscustomobject]$Timing)
    $prompt = [regex]::Matches($Text, '(?m)^main: number of tokens in prompt = (?<count>[0-9]+)[ \t]*\r?$')
    $sampler = [regex]::Matches($Text, '(?m)^common_perf_print:[ \t]+samplers time[ \t]*=[ \t]*[0-9]+\.[0-9]+ ms[ \t]*/[ \t]*(?<count>[0-9]+) tokens[ \t]*\r?$')
    if ($prompt.Count -ne 1 -or $sampler.Count -ne 1) { throw 'Expected unique prompt and sampler-accept counters from the pinned completion tool.' }
    $culture = [Globalization.CultureInfo]::InvariantCulture
    $promptTokens = [int]::Parse($prompt[0].Groups['count'].Value, $culture)
    $acceptedTokens = [int]::Parse($sampler[0].Groups['count'].Value, $culture)
    $generated = $acceptedTokens - $promptTokens
    if ($promptTokens -lt 2 -or $promptTokens -gt 8192 -or $generated -lt 1 -or $generated -gt 512 -or
        $acceptedTokens -ne ($Timing.PromptTokens + $Timing.DecodeTokens + 1)) {
        throw 'Native prompt, sampler and decode counters disagree for a fresh single-turn sample.'
    }
    [pscustomobject]@{ PromptTokenCount = $promptTokens; SamplerAcceptedTokens = $acceptedTokens
        GeneratedSampledTokens = $generated }
}

function Get-BonsaiResponseDigest {
    <# .SYNOPSIS
    Hashes exact captured UTF-8 response text without trimming or normalization.
    #>
    [CmdletBinding()]
    [OutputType([string])]
    param([Parameter(Mandatory)][AllowEmptyString()][string]$Text)
    [Convert]::ToHexStringLower([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($Text)))
}

Export-ModuleMember -Function Get-BonsaiLoraEvaluationArgument, Read-BonsaiCompletionTiming, Read-BonsaiCompletionTokenCount, Get-BonsaiResponseDigest
