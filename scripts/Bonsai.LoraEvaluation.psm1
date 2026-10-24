#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Completion.psm1')

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
    $arguments = @(Get-BonsaiCompletionArgument -ModelPath $ModelPath -Profile swift-pq2-assessment) + @(
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

Microsoft.PowerShell.Core\Export-ModuleMember -Function Get-BonsaiLoraEvaluationArgument
