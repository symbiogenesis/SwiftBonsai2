#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

function Invoke-BonsaiStandardsAnalyzer {
    <#
    .SYNOPSIS
    Runs the complete configured analyzer policy without concurrent export resolution.
    .DESCRIPTION
    PSScriptAnalyzer 1.25.0 runs built-in rules concurrently. These two rules share
    Export-ModuleMember.ResolveParameter, whose PowerShell 7.6 merged metadata is
    runspace-bound. Preserve the rules and configuration while scheduling those
    calls separately. Every other configured rule retains its ordinary execution.
    #>
    [CmdletBinding()]
    [OutputType([object])]
    param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][string]$Settings)
    $configuration = Import-PowerShellDataFile -Path $Settings
    $excluded = @()
    if ($configuration.ContainsKey('ExcludeRules')) { $excluded = @($configuration.ExcludeRules) }
    $serial = @('PSReservedCmdletChar', 'PSProvideCommentHelp')
    Invoke-ScriptAnalyzer -Path $Path -Settings $Settings -ExcludeRule @($excluded + $serial) -ErrorAction Stop
    foreach ($rule in $serial) {
        Invoke-ScriptAnalyzer -Path $Path -Settings $Settings -IncludeRule $rule -ErrorAction Stop
    }
}

Export-ModuleMember -Function Invoke-BonsaiStandardsAnalyzer
