#requires -Version 7.6
<# .SYNOPSIS
Inventories checked SDK generator, imports, license and recipe bytes on hosted CI without executing SDK code.
#>
[CmdletBinding()]
[OutputType([Collections.Specialized.OrderedDictionary])]
param([Parameter(Mandatory)][string]$OutputDirectory)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')

function Assert-BonsaiHexagonSdkStream {
    <# .SYNOPSIS
    Checks exact bytes on an already retained guard without changing its cursor.
    #>
    [CmdletBinding()]
    [OutputType([void])]
    param([Parameter(Mandatory)][IO.FileStream]$Stream,
          [Parameter(Mandatory)][long]$Bytes,
          [Parameter(Mandatory)][string]$Sha256)
    if (-not $Stream.CanRead -or -not $Stream.CanSeek -or $Bytes -le 0 -or $Sha256 -cnotmatch '^[a-f0-9]{64}$') {
        throw 'SDK verification requires a readable retained file and an exact byte/digest pin.'
    }
    $position = $Stream.Position
    try {
        if ($Stream.Length -ne $Bytes) { throw 'SDK input byte length differs from its pin.' }
        $Stream.Position = 0
        $digest = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($Stream)).ToLowerInvariant()
        if ($Stream.Length -ne $Bytes -or $digest -cne $Sha256) { throw 'SDK input SHA-256 differs from its pin.' }
    } finally { $Stream.Position = $position }
}

if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -cne 'Arm64' -or
    $env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted') {
    throw 'SDK inventory requires hosted Windows ARM64; owner-device fallback is unsupported.'
}
if ($env:GITHUB_SHA -cnotmatch '^[a-f0-9]{40}$' -or $env:GITHUB_RUN_ID -cnotmatch '^[0-9]{1,32}$' -or $env:GITHUB_RUN_ATTEMPT -cnotmatch '^[0-9]{1,8}$') {
    throw 'SDK inventory requires exact source and hosted run identities.'
}
$output = Resolve-ExternalPath -Path $OutputDirectory
if (Test-Path -LiteralPath $output) { throw 'SDK inventory output must be new external storage.' }
[void][IO.Directory]::CreateDirectory($output)
$review = Join-Path $output 'review'
[void][IO.Directory]::CreateDirectory($review)
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$pinPath = Join-Path $repo 'tools/hexagon/sdk-inventory.lock.json'
$source = Join-Path $repo 'tools/hexagon/sdk_inventory.py'
$pin = [IO.File]::ReadAllText($pinPath) | ConvertFrom-Json -AsHashtable -NoEnumerate -DateKind String
if ($pin.schemaVersion -ne 1 -or $pin.generatorExecutionAllowed -isnot [bool] -or $pin.generatorExecutionAllowed -or
    $pin.deviceExecutionAllowed -isnot [bool] -or $pin.deviceExecutionAllowed -or $pin.profileEligible -isnot [bool] -or $pin.profileEligible -or
    $pin.asset.filename -cne 'hexagon-sdk-v6.7.0.0-arm64-wos.tar.xz' -or $pin.asset.bytes -ne 900461020 -or
    $pin.asset.sha256 -cne '9eb256cc7b102f721365151a470a33980069a18b9209e1d56a6d2fa430369759' -or
    $pin.asset.url -cne 'https://github.com/snapdragon-toolchain/hexagon-sdk/releases/download/v6.7.0.0/hexagon-sdk-v6.7.0.0-arm64-wos.tar.xz') {
    throw 'The reviewed SDK archive selection or execution boundary changed.'
}
$holds = [Collections.Generic.List[IO.FileStream]]::new()
$clock = [Diagnostics.Stopwatch]::StartNew()
$report = [ordered]@{
    SchemaVersion = 1; Kind = 'hexagon-hosted-sdk-inventory'; RunnerRevision = $env:GITHUB_SHA
    RunId = $env:GITHUB_RUN_ID; RunAttempt = $env:GITHUB_RUN_ATTEMPT; HostedOnly = $true
    Outcome = 'pending'; Stage = 'reviewed-source-inputs'; ElapsedMilliseconds = 0
    SourceSha256 = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()
    SupervisorSha256 = (Get-FileHash -LiteralPath $PSCommandPath -Algorithm SHA256).Hash.ToLowerInvariant()
    PinSha256 = (Get-FileHash -LiteralPath $pinPath -Algorithm SHA256).Hash.ToLowerInvariant()
    ReviewedInputs = @(); Archive = $pin.asset; InventoryReceiptSha256 = $null
    PythonExecutableSha256 = $null; SdkCodeExecuted = $false; GeneratorExecuted = $false
    GeneratedSourceUploaded = $false; ModelLoaded = $false; DeviceCodeLoaded = $false; ProfileEligible = $false
    LicenseReview = 'pending'; GeneratorInvocationReviewed = $false
}
try {
    foreach ($entry in $pin.reviewedInputs) {
        if ($entry.name -isnot [string] -or $entry.name -cnotmatch '^[A-Za-z0-9][A-Za-z0-9_.-]{0,127}$' -or
            $entry.bytes -le 0 -or $entry.bytes -gt 65536 -or $entry.sha256 -cnotmatch '^[a-f0-9]{64}$') { throw 'Invalid bounded reviewed SDK source input.' }
        $path = Receive-LockedArtifact -Uri $entry.url -Path (Join-Path $output $entry.name) -Bytes $entry.bytes -Sha256 $entry.sha256 -TimeoutSeconds 30
        $stream = [IO.FileStream]::new($path, 'Open', 'Read', 'Read')
        $holds.Add($stream)
        Assert-BonsaiHexagonSdkStream -Stream $stream -Bytes $entry.bytes -Sha256 $entry.sha256
        $report.ReviewedInputs += [ordered]@{ Name = $entry.name; Url = $entry.url; Bytes = $entry.bytes; Sha256 = $entry.sha256 }
    }
    $report.Stage = 'sdk-archive-download'
    $archive = Receive-LockedArtifact -Uri $pin.asset.url -Path (Join-Path $output $pin.asset.filename) -Bytes $pin.asset.bytes -Sha256 $pin.asset.sha256 -TimeoutSeconds $pin.limits.downloadSeconds
    $guard = [IO.FileStream]::new($archive, 'Open', 'Read', 'Read')
    $holds.Add($guard)
    Assert-BonsaiHexagonSdkStream -Stream $guard -Bytes $pin.asset.bytes -Sha256 $pin.asset.sha256
    $python = (Get-Command -Name python.exe -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
    $pythonGuard = [IO.FileStream]::new($python, 'Open', 'Read', 'Read')
    $holds.Add($pythonGuard)
    $report.PythonExecutableSha256 = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($pythonGuard)).ToLowerInvariant()
    $report.Stage = 'sdk-data-only-inventory'
    $inventoryPath = Join-Path $review 'sdk-inventory.json'
    $result = Invoke-BonsaiNativeProcess -Executable $python -ArgumentList @('-B',$source,'--archive',$archive,'--pin',$pinPath,'--output',$inventoryPath) -WorkingDirectory $output -TimeoutSeconds $pin.limits.inventorySeconds -OwnDescendants
    if ($result.ExitCode -ne 0 -or $result.StandardOutput.Length -gt 4096 -or $result.StandardError.Length -gt 65536) {
        throw 'SDK data-only inventory failed its fixed process/output contract; no SDK tool was executed.'
    }
    if (-not [IO.File]::Exists($inventoryPath) -or ([IO.FileInfo]$inventoryPath).Length -gt $pin.limits.receiptBytes) { throw 'SDK inventory omitted its bounded receipt.' }
    $report.InventoryReceiptSha256 = (Get-FileHash -LiteralPath $inventoryPath -Algorithm SHA256).Hash.ToLowerInvariant()
    $report.Stage = 'sdk-data-only-inventory-complete'
    $report.Outcome = 'inventoried-generation-pending'
} finally {
    foreach ($stream in $holds) { $stream.Dispose() }
    $report.ElapsedMilliseconds = $clock.ElapsedMilliseconds
    $report | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $review 'host.json') -Encoding utf8NoBOM
}
return $report
