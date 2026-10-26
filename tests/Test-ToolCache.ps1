#requires -Version 7.6
[CmdletBinding()]
[OutputType([string])]
param([ValidateSet('All', 'Reuse', 'Integrity')][string]$Case = 'All')
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if ($Case -ceq 'All' -and $env:GITHUB_ACTIONS -cne 'true') {
    throw 'Complete suites run only in GitHub Actions. Select a focused case locally.'
}
$repo = Split-Path $PSScriptRoot -Parent
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-ToolCacheTests-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($scratch)

function Initialize-TestToolModule {
    [CmdletBinding()]
    [OutputType([Management.Automation.PSModuleInfo])]
    param([Parameter(Mandatory)][string]$Directory, [Parameter(Mandatory)][string]$Repository)
    $module = Import-Module (Join-Path $Repository 'scripts/Bonsai.Tools.psm1') -Force -PassThru
    & $module {
        param([string]$Directory)
        $script:ToolCacheRoot = Join-Path $Directory 'cache'
        $script:ToolScratch['bonsai-tools'] = Join-Path $Directory 'diagnostics'
        [void][IO.Directory]::CreateDirectory($script:ToolScratch['bonsai-tools'])
        $script:BuildRecords = Join-Path $Directory 'builds.jsonl'
        $script:BuildExitCode = 0
        function script:Get-Command {
            [CmdletBinding()]
            [OutputType([pscustomobject])]
            param([string]$Name, [string]$CommandType)
            if ($Name -cne 'cargo' -or $CommandType -cne 'Application') { throw 'Unexpected compiler lookup.' }
            return [pscustomobject]@{ Source = 'cargo.exe' }
        }
        function script:Invoke-BonsaiNativeProcess {
            [CmdletBinding()]
            [OutputType([pscustomobject])]
            param([string]$Executable, [string[]]$ArgumentList, [string]$WorkingDirectory, [string]$LogPath)
            $target = $ArgumentList[[array]::IndexOf($ArgumentList, '--target-dir') + 1]
            $path = Join-Path $target 'release/bonsai-tools.exe'
            $record = @{ Executable = $Executable; Arguments = $ArgumentList; WorkingDirectory = $WorkingDirectory; LogPath = $LogPath; Existed = [IO.File]::Exists($path) }
            [IO.File]::AppendAllText($script:BuildRecords, (ConvertTo-Json -InputObject $record -Compress) + "`n")
            if (-not $script:BuildExitCode -and -not [IO.File]::Exists($path)) {
                [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($path))
                [IO.File]::WriteAllText($path, 'checked executable fixture')
            }
            return [pscustomobject]@{ ExitCode = $script:BuildExitCode; StandardError = 'fixture diagnostic' }
        }
    } $Directory
    return $module
}

$module = Initialize-TestToolModule -Directory $scratch -Repository $repo
$parameters = @{ Name = 'bonsai-tools'; ManifestRelativePath = '../tools/bonsai/Cargo.toml' }
if ($Case -cin @('All', 'Reuse')) {
    $first = Resolve-BonsaiRustTool @parameters
    $second = Resolve-BonsaiRustTool @parameters
    $records = @(Get-Content -LiteralPath (Join-Path $scratch 'builds.jsonl') | ConvertFrom-Json)
    Assert ($first -ceq $second -and $records.Count -eq 2 -and $records[1].Existed) 'Repeated source resolution discarded the cache or skipped Cargo freshness.'
    $module = Initialize-TestToolModule -Directory $scratch -Repository $repo
    $third = Resolve-BonsaiRustTool @parameters
    $records = @(Get-Content -LiteralPath (Join-Path $scratch 'builds.jsonl') | ConvertFrom-Json)
    Assert ($third -ceq $first -and $records.Count -eq 3 -and $records[2].Existed) 'A new launcher session discarded the cache or skipped Cargo freshness.'
    $toolchain = Get-Content -LiteralPath (Join-Path $repo 'rust-toolchain.toml') -Raw
    $null = $toolchain -match '(?m)^channel\s*=\s*"(nightly-\d{4}-\d{2}-\d{2})"\s*$'
    $channel = '+' + $Matches[1] + '-aarch64-pc-windows-msvc'
    foreach ($record in $records) {
        Assert ($record.Arguments[0] -ceq $channel -and '--locked' -cin $record.Arguments -and '--release' -cin $record.Arguments) 'Maintenance reuse lost its pinned release compiler contract.'
        Assert ($record.WorkingDirectory -ceq (Join-Path $scratch 'diagnostics') -and $record.LogPath.StartsWith($record.WorkingDirectory + '\', [StringComparison]::OrdinalIgnoreCase)) 'Build diagnostics escaped their assigned external lane.'
    }
    $explicit = Join-Path $scratch 'explicit.exe'
    [IO.File]::WriteAllText($explicit, 'explicit tool fixture')
    Assert ((Resolve-BonsaiRustTool @parameters -ToolPath $explicit) -ceq $explicit) 'Explicit maintenance tool selection was replaced.'
    $records = @(Get-Content -LiteralPath (Join-Path $scratch 'builds.jsonl') | ConvertFrom-Json)
    Assert ($records.Count -eq 3) 'An explicit maintenance executable triggered a build.'
}
if ($Case -cin @('All', 'Integrity')) {
    $tool = Resolve-BonsaiRustTool @parameters
    $target = [IO.Path]::GetDirectoryName([IO.Path]::GetDirectoryName($tool))
    $receipt = Join-Path $target 'bonsai-tools.sha256'
    [IO.File]::WriteAllText($tool, 'corrupt cached executable')
    $null = Resolve-BonsaiRustTool @parameters
    Assert ([IO.File]::ReadAllText($tool) -ceq 'checked executable fixture') 'A corrupt executable reached maintenance operations.'
    [IO.File]::Delete($receipt)
    $null = Resolve-BonsaiRustTool @parameters
    $records = @(Get-Content -LiteralPath (Join-Path $scratch 'builds.jsonl') | ConvertFrom-Json)
    Assert (-not $records[-1].Existed -and -not $records[-2].Existed) 'Changed or unreceipted outputs were trusted by Cargo.'
    $lock = [IO.FileStream]::new((Join-Path $target 'build.lock'), [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    try { Assert-Failure { Resolve-BonsaiRustTool @parameters } 'Concurrent cache preparation was accepted.' }
    finally { $lock.Dispose() }
    & $module { $script:BuildExitCode = 7 }
    Assert-Failure { Resolve-BonsaiRustTool @parameters } 'A failed freshness check returned an executable.'
    & $module { $script:ToolCacheRoot = $args[0] } $repo
    Assert-Failure { Resolve-BonsaiRustTool @parameters } 'A maintenance cache was created inside Git.'
    Assert-Failure { Resolve-BonsaiRustTool @parameters -ToolPath 'relative.exe' } 'A relative explicit tool path was accepted.'
}
Write-Output "Maintenance tool cache $Case checks passed. Evidence: $scratch"
