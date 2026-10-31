#requires -Version 7.6
<# .SYNOPSIS
Checks the fixed acceptance and inherited owner deadlines without processes or models.
#>
[CmdletBinding()]
[OutputType([string])]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.NativeAcceptanceBudget.psm1')

$origin = [DateTime]::new(2026, 10, 31, 0, 0, 0, [DateTimeKind]::Utc)
$arguments = @{
    JobStartedUtc=$origin.AddSeconds(30); NowUtc=$origin.AddSeconds(50)
    NowTick=500000L; TickFrequency=1000L; RunId='12345'; Revision=('a' * 40)
}
$empty = Get-NativeAcceptanceBudget @arguments
Assert ($empty.HardLimitSeconds -eq 250 -and $empty.CaseBudgetSeconds -eq 240 -and
    -not $empty.OwnerDeadlineApplied -and $empty.PreparationElapsedSeconds -eq 20) 'Omitted owner binding changed existing limits.'
$nulls = @{ RunId=$null; Revision=''; StartedTick=$null; StartedUtc=''; WorkSeconds=$null }
$emptyFields = Get-NativeAcceptanceBudget @arguments -OwnerDeadline $nulls
Assert ($emptyFields.HardLimitSeconds -eq 250 -and -not $emptyFields.OwnerDeadlineApplied) 'Absent inherited environment fields changed existing limits.'
$owner = @{
    RunId='12345'; Revision=('a' * 40); StartedTick='450000'
    StartedUtc=$origin.ToString('o'); WorkSeconds='210'
}
$bounded = Get-NativeAcceptanceBudget @arguments -OwnerDeadline $owner
Assert ($bounded.OwnerDeadlineApplied -and $bounded.OwnerElapsedSeconds -eq 50 -and
    $bounded.HardLimitSeconds -eq 145 -and $bounded.CaseBudgetSeconds -eq 135) 'The original monotonic owner deadline did not shorten both limits.'
$lastSecond = $owner.Clone()
$lastSecond.StartedTick = '316000'
$minimum = Get-NativeAcceptanceBudget @arguments -OwnerDeadline $lastSecond
Assert ($minimum.HardLimitSeconds -eq 11 -and $minimum.CaseBudgetSeconds -eq 1) 'The last one-second case allowance changed.'
$lastSecond.StartedTick = '315999'
Assert-Failure { Get-NativeAcceptanceBudget @arguments -OwnerDeadline $lastSecond } 'Fractional elapsed time rounded up an exhausted allowance.'

# Registration and work before the first CI step remain charged to the owner.
$earlier = $owner.Clone()
$earlier.StartedTick = '350000'
$startup = Get-NativeAcceptanceBudget @arguments -OwnerDeadline $earlier
Assert ($startup.HardLimitSeconds -eq 45 -and $startup.CaseBudgetSeconds -eq 35) 'Owner startup costs were reset at the job boundary.'
$lateJob = $arguments.Clone()
$lateJob.JobStartedUtc = $origin.AddSeconds(-200)
$jobBounded = Get-NativeAcceptanceBudget @lateJob -OwnerDeadline $owner
Assert ($jobBounded.HardLimitSeconds -eq 20 -and $jobBounded.CaseBudgetSeconds -eq 10) 'An owner deadline extended the existing job allowance.'

foreach ($field in @('RunId', 'Revision', 'StartedTick', 'StartedUtc', 'WorkSeconds')) {
    $partial = $owner.Clone()
    $partial.Remove($field)
    Assert-Failure { Get-NativeAcceptanceBudget @arguments -OwnerDeadline $partial } 'A partial inherited owner deadline was accepted.'
}
foreach ($invalid in @(
    @{ RunId='54321' }, @{ Revision=('b' * 40) }, @{ WorkSeconds='211' }, @{ WorkSeconds='0210' },
    @{ StartedTick='500001' }, @{ StartedTick='0' }, @{ StartedTick='-1' }, @{ StartedTick='+450000' },
    @{ StartedTick='not-a-tick' }, @{ StartedTick='9223372036854775808' },
    @{ StartedTick='300000' }, @{ StartedTick=450000L },
    @{ StartedUtc='2026-10-31T00:00:00.0000000+00:00' },
    @{ StartedUtc='2026-10-31T00:00:00.0000000' },
    @{ StartedUtc='2026-10-31T00:00:00.0000000-05:00' },
    @{ StartedUtc='not-utc' }, @{ StartedUtc=$origin.AddMinutes(1).ToString('o') }
)) {
    $rejected = $owner.Clone()
    foreach ($field in $invalid.Keys) { $rejected[$field] = $invalid[$field] }
    Assert-Failure { Get-NativeAcceptanceBudget @arguments -OwnerDeadline $rejected } 'An invalid, future or exhausted owner binding was accepted.'
}
$unknown = $owner.Clone()
$unknown.Extra = '210'
Assert-Failure { Get-NativeAcceptanceBudget @arguments -OwnerDeadline $unknown } 'Unknown owner deadline fields were ignored.'
$expiredJob = $arguments.Clone()
$expiredJob.JobStartedUtc = $origin.AddSeconds(-220)
Assert-Failure { Get-NativeAcceptanceBudget @expiredJob } 'An exhausted independent job allowance was accepted.'
$badFrequency = $arguments.Clone()
$badFrequency.TickFrequency = 0
Assert-Failure { Get-NativeAcceptanceBudget @badFrequency -OwnerDeadline $owner } 'An invalid monotonic clock frequency was accepted.'
$nonUtc = $arguments.Clone()
$nonUtc.NowUtc = [DateTime]::SpecifyKind($arguments.NowUtc, [DateTimeKind]::Unspecified)
Assert-Failure { Get-NativeAcceptanceBudget @nonUtc -OwnerDeadline $owner } 'An ambiguous job clock was accepted.'
'Focused native acceptance budget checks passed without model or process execution.'
