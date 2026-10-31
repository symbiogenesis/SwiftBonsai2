#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-NativeAcceptanceBudget {
    <# .SYNOPSIS
    Computes fixed native acceptance limits, optionally shortened by an exact owner binding.
    .DESCRIPTION
    Pure calculation: reads no environment, starts no process and loads no model.
    OwnerDeadline contains RunId, Revision, StartedTick, StartedUtc and WorkSeconds.
    All five absent preserves the existing job allowance; any supplied binding must
    match this exact run/revision and the fixed 210-second monotonic work deadline.
    UTC supplies sanity/attribution only. Injected clocks permit model-free tests.
    #>
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param(
        [Parameter(Mandatory)][DateTime]$JobStartedUtc,
        [Parameter(Mandatory)][DateTime]$NowUtc,
        [Parameter(Mandatory)][long]$NowTick,
        [Parameter(Mandatory)][long]$TickFrequency,
        [Parameter(Mandatory)][ValidatePattern('^[1-9][0-9]{0,19}$')][string]$RunId,
        [Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{40}$')][string]$Revision,
        [Collections.IDictionary]$OwnerDeadline = @{}
    )
    if ($JobStartedUtc.Kind -ne [DateTimeKind]::Utc -or $NowUtc.Kind -ne [DateTimeKind]::Utc -or
        $NowTick -lt 0 -or $TickFrequency -le 0) { throw 'Acceptance clocks must be valid UTC and monotonic values.' }
    $setupElapsed = ($NowUtc - $JobStartedUtc).TotalSeconds
    if ($setupElapsed -lt 0) { throw 'Acceptance setup time cannot be negative.' }
    $hardLimit = [Math]::Floor([Math]::Min(250, 270 - $setupElapsed))
    $fields = @('RunId', 'Revision', 'StartedTick', 'StartedUtc', 'WorkSeconds')
    if ($null -eq $OwnerDeadline -or @($OwnerDeadline.Keys | Where-Object { $_ -cnotin $fields }).Count) {
        throw 'Owner deadline fields are invalid.'
    }
    $supplied = @($fields | Where-Object {
        $OwnerDeadline.Contains($_) -and $null -ne $OwnerDeadline[$_] -and $OwnerDeadline[$_] -cne ''
    })
    $ownerElapsed = $null
    if ($supplied.Count) {
        if ($supplied.Count -ne $fields.Count) { throw 'The complete owner deadline binding is required.' }
        foreach ($field in $fields) {
            if ($OwnerDeadline[$field] -isnot [string]) { throw 'Owner deadline fields must be strings.' }
        }
        if ($OwnerDeadline.RunId -cne $RunId -or $OwnerDeadline.Revision -cne $Revision -or
            $OwnerDeadline.WorkSeconds -cne '210') { throw 'The owner deadline run, revision or fixed allowance differs.' }
        $ownerTick = 0L
        if ($OwnerDeadline.StartedTick -cnotmatch '^[1-9][0-9]{0,18}$' -or
            -not [long]::TryParse($OwnerDeadline.StartedTick, [Globalization.NumberStyles]::None,
                [Globalization.CultureInfo]::InvariantCulture, [ref]$ownerTick) -or $ownerTick -gt $NowTick) {
            throw 'The original owner monotonic start is invalid or in the future.'
        }
        $ownerUtc = [DateTime]::MinValue
        if ($OwnerDeadline.StartedUtc -cnotmatch '^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{7}Z$' -or
            -not [DateTime]::TryParseExact($OwnerDeadline.StartedUtc, 'o', [Globalization.CultureInfo]::InvariantCulture,
                [Globalization.DateTimeStyles]::RoundtripKind, [ref]$ownerUtc) -or
            $ownerUtc.Kind -ne [DateTimeKind]::Utc -or $ownerUtc -gt $NowUtc) {
            throw 'The owner start requires a nonfuture round-trip UTC timestamp.'
        }
        $ownerElapsed = ($NowTick - $ownerTick) / [double]$TickFrequency
        # Keep fifteen seconds before the independent outer work cutoff for
        # cooperative process completion, receipt collection and runner exit.
        $hardLimit = [Math]::Min($hardLimit, [Math]::Floor(210 - $ownerElapsed - 15))
    }
    $caseBudget = [Math]::Min(240, $hardLimit - 10)
    if ($caseBudget -lt 1) { throw 'Preparation exhausted the acceptance allowance; qualification remains pending.' }
    return [pscustomobject]@{
        PreparationElapsedSeconds=$setupElapsed
        HardLimitSeconds=[int]$hardLimit
        CaseBudgetSeconds=[int]$caseBudget
        OwnerDeadlineApplied=($supplied.Count -eq $fields.Count)
        OwnerElapsedSeconds=$ownerElapsed
    }
}

Export-ModuleMember -Function Get-NativeAcceptanceBudget
