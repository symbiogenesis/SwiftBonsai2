#requires -Version 7.6
<# .SYNOPSIS
Checks bounded native log retention, strict decoding and credential nondisclosure without a model.
#>
[CmdletBinding()]
[OutputType([string])]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
$script = Join-Path $PSScriptRoot '../scripts/Save-NativeStartupDiagnostics.ps1'
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('Bonsai-Startup-Diagnostics-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($scratch)
$encoding = [Text.UTF8Encoding]::new($false, $true)
$frontend = 'a' * 64
$backend = 'b' * 64

function New-Fixture {
    [CmdletBinding(SupportsShouldProcess)]
    [OutputType([string])]
    param([Parameter(Mandatory)][string]$Name)
    $root = Join-Path $scratch $Name
    $private = Join-Path $root ('SwiftBonsai-Keys-' + ('1' * 32))
    if ($PSCmdlet.ShouldProcess($root, 'Create synthetic diagnostic fixture')) {
        [void][IO.Directory]::CreateDirectory($private)
        [IO.File]::WriteAllText((Join-Path $private 'frontend.key'), $frontend, $encoding)
        [IO.File]::WriteAllText((Join-Path $private 'backend.key'), $backend, $encoding)
    }
    return $root
}

try {
    $root = New-Fixture -Name 'redaction'
    $private = Join-Path $root ('SwiftBonsai-Keys-' + ('1' * 32))
    $second = Join-Path $root ('SwiftBonsai-Keys-' + ('2' * 32))
    [void][IO.Directory]::CreateDirectory($second)
    [IO.File]::WriteAllText((Join-Path $second 'frontend.key'), ('c' * 64), $encoding)
    [IO.File]::WriteAllText((Join-Path $second 'backend.key'), ('d' * 64), $encoding)
    [IO.File]::WriteAllText((Join-Path $private 'backend.stdout'), "ready $frontend $($backend.ToUpperInvariant()) $('c' * 64) $('e' * 64)`n", $encoding)
    [IO.File]::WriteAllText((Join-Path $private 'backend.stderr'), "fixed startup failure`n$([char]27)control", $encoding)
    [IO.File]::WriteAllBytes((Join-Path $second 'backend.stdout'), [byte[]]::new(0))
    $nested = Join-Path $private 'nested'
    [void][IO.Directory]::CreateDirectory($nested)
    [IO.File]::WriteAllText((Join-Path $nested 'backend.stderr'), 'MUST-NOT-BE-COPIED', $encoding)
    & $script -EvidenceDirectory $root -AcceptanceExitCode 101
    $receipt = Get-Content -LiteralPath (Join-Path $root 'startup-diagnostics.json') -Raw | ConvertFrom-Json
    Assert ($receipt.Classification -ceq 'bounded-sanitized-logs-retained') 'Valid diagnostics were lost.'
    Assert ($receipt.AcceptanceProcessExitCode -eq 101 -and 'BackendExitCode' -cnotin $receipt.PSObject.Properties.Name) 'Test-process status was confused with backend status.'
    Assert (@($receipt.Sources).Count -eq 4 -and @($receipt.PublishedFiles).Count -eq 2) 'The exact log/session whitelist changed.'
    $public = @(Get-ChildItem -LiteralPath $root -File | ForEach-Object { [IO.File]::ReadAllText($_.FullName) }) -join "`n"
    foreach ($secret in @($frontend, $backend, ('c' * 64), ('d' * 64), ('e' * 64), 'MUST-NOT-BE-COPIED')) {
        Assert (-not $public.Contains($secret, [StringComparison]::OrdinalIgnoreCase)) 'A credential or nested private file escaped.'
    }
    Assert ($public.Contains('ready [REDACTED]') -and $public.Contains('fixed startup failure')) 'Useful sanitized diagnostic content was discarded.'
    Assert (-not $public.Contains([string][char]27)) 'Terminal controls escaped.'
    Assert (@(Get-ChildItem -LiteralPath $root -File).Count -eq 3) 'Unexpected public output was created.'

    $root = New-Fixture -Name 'bounds'
    $private = Join-Path $root ('SwiftBonsai-Keys-' + ('1' * 32))
    [IO.File]::WriteAllBytes((Join-Path $private 'backend.stdout'), [byte[]]::new(65537))
    [IO.File]::WriteAllBytes((Join-Path $private 'backend.stderr'), [byte[]]@(0xC3, 0x28))
    & $script -EvidenceDirectory $root -BudgetExpired
    $receipt = Get-Content -LiteralPath (Join-Path $root 'startup-diagnostics.json') -Raw | ConvertFrom-Json
    Assert ($receipt.AggregateBudgetExpired -and $receipt.Classification -ceq 'no-log-published') 'A failed/budget-limited collection was reported as retained.'
    Assert ($receipt.Sources[0].Classification -ceq 'read-bound-exceeded' -and $receipt.Sources[1].Classification -ceq 'invalid-utf8') 'Read bounds or strict UTF-8 decoding changed.'
    Assert (@(Get-ChildItem -LiteralPath $root -File).Count -eq 1) 'Rejected raw logs were copied.'

    $root = New-Fixture -Name 'missing-key'
    $private = Join-Path $root ('SwiftBonsai-Keys-' + ('1' * 32))
    Remove-Item -LiteralPath (Join-Path $private 'frontend.key')
    [IO.File]::WriteAllText((Join-Path $private 'backend.stderr'), "sensitive $backend", $encoding)
    [IO.File]::WriteAllText((Join-Path $root 'startup-backend.stderr.txt'), 'stale public content', $encoding)
    & $script -EvidenceDirectory $root
    $receipt = Get-Content -LiteralPath (Join-Path $root 'startup-diagnostics.json') -Raw | ConvertFrom-Json
    Assert ($receipt.Classification -ceq 'key-validation-failed') 'Missing actual keys did not close publication.'
    Assert (-not (Test-Path -LiteralPath (Join-Path $root 'startup-backend.stderr.txt'))) 'A failed collection left stale output.'

    $root = New-Fixture -Name 'session-bound'
    foreach ($digit in 2..9) { [void][IO.Directory]::CreateDirectory((Join-Path $root ('SwiftBonsai-Keys-' + ([string]$digit * 32)))) }
    & $script -EvidenceDirectory $root
    $receipt = Get-Content -LiteralPath (Join-Path $root 'startup-diagnostics.json') -Raw | ConvertFrom-Json
    Assert ($receipt.Classification -ceq 'session-bound-exceeded' -and @($receipt.PublishedFiles).Count -eq 0) 'Session enumeration exceeded its fixed bound.'
    Write-Output 'Focused native diagnostic checks passed: actual/cross-session credentials, strict UTF-8, size/session bounds, empty logs and stale-output rejection.'
} finally {
    $resolved = [IO.Path]::GetFullPath($scratch)
    $parent = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
    if (-not $resolved.StartsWith($parent, [StringComparison]::OrdinalIgnoreCase) -or
        [IO.Path]::GetFileName($resolved) -cnotmatch '^Bonsai-Startup-Diagnostics-[a-f0-9]{32}$') { throw 'Unsafe fixture cleanup path.' }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
