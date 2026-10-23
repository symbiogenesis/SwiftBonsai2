#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

function Assert {
    <# .SYNOPSIS
    Fails a focused regression with its caller-supplied explanation.
    #>
    [CmdletBinding()]
    [OutputType([void])]
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw $Message }
}

function Assert-Failure {
    <# .SYNOPSIS
    Requires an invalid-input action to throw.
    #>
    [CmdletBinding()]
    [OutputType([void])]
    param([scriptblock]$Action, [string]$Message = 'Expected invalid input to fail.')
    $failed = $false
    try { & $Action | Out-Null } catch { $failed = $true }
    Assert $failed $Message
}

Export-ModuleMember -Function Assert, Assert-Failure
