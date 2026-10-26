#requires -Version 7.6
[CmdletBinding()]
[OutputType([string])]
param([ValidateSet('All', 'Fingerprint', 'Receipt', 'Invalidation')][string]$Case = 'All')
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if ($Case -ceq 'All' -and $env:GITHUB_ACTIONS -cne 'true') {
    throw 'Complete suites run only in GitHub Actions. Select a focused case locally.'
}
$repo = Split-Path $PSScriptRoot -Parent
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
$module = Import-Module (Join-Path $repo 'scripts/Bonsai.BuildCache.psm1') -Force -PassThru
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-BuildCacheTests-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($scratch)
$fixture = Join-Path $scratch 'repository'
[void][IO.Directory]::CreateDirectory((Join-Path $fixture 'src'))
$inputPath = Join-Path $fixture 'src/main.rs'
[IO.File]::WriteAllText($inputPath, 'first source')
[IO.File]::WriteAllText((Join-Path $fixture 'Cargo.toml'), '[package]')
if ($Case -cin @('All', 'Fingerprint')) {
    $first = Get-BonsaiBuildFingerprint -RepositoryRoot $fixture
    $second = Get-BonsaiBuildFingerprint -RepositoryRoot $fixture
    Assert ($first -cmatch '^[a-f0-9]{64}$' -and $first -ceq $second) 'Unchanged inputs did not produce a deterministic fingerprint.'
    $time = [IO.File]::GetLastWriteTimeUtc($inputPath)
    [IO.File]::WriteAllText($inputPath, 'other source')
    [IO.File]::SetLastWriteTimeUtc($inputPath, $time)
    $changed = Get-BonsaiBuildFingerprint -RepositoryRoot $fixture
    Assert ($first -cne $changed) 'A same-size source edit with preserved timestamp reused old outputs.'
    $added = Join-Path $fixture 'src/new.rs'
    [IO.File]::WriteAllText($added, 'new source')
    Assert ((Get-BonsaiBuildFingerprint -RepositoryRoot $fixture) -cne $changed) 'An added source did not invalidate outputs.'
    [IO.File]::Delete($added)
    Assert ((Get-BonsaiBuildFingerprint -RepositoryRoot $fixture) -ceq $changed) 'Removing an input was not represented consistently.'
    Assert ((Get-BonsaiBuildFingerprint -RepositoryRoot $fixture -Identity 'different package') -cne $changed) 'Package identity was omitted.'
    $oldFlags = $env:RUSTFLAGS
    try {
        $env:RUSTFLAGS = '--cfg bonsai_cache_regression'
        Assert ((Get-BonsaiBuildFingerprint -RepositoryRoot $fixture) -cne $changed) 'Rust build flags did not invalidate outputs.'
    } finally { $env:RUSTFLAGS = $oldFlags }
    $oldSession = $env:WT_SESSION
    try {
        $env:WT_SESSION = [guid]::NewGuid().ToString('N')
        Assert ((Get-BonsaiBuildFingerprint -RepositoryRoot $fixture) -ceq $changed) 'A fresh terminal session invalidated unchanged builds.'
    } finally { $env:WT_SESSION = $oldSession }
    [void][IO.Directory]::CreateDirectory((Join-Path $fixture 'target'))
    [IO.File]::WriteAllText((Join-Path $fixture 'target/output'), 'disposable output')
    Assert ((Get-BonsaiBuildFingerprint -RepositoryRoot $fixture) -ceq $changed) 'Cargo output invalidated its own receipt.'
    $firstWorking = Join-Path -Path $scratch -ChildPath 'work' -AdditionalChildPath 'first'
    $secondWorking = Join-Path -Path $scratch -ChildPath 'work' -AdditionalChildPath 'second'
    [void][IO.Directory]::CreateDirectory($firstWorking)
    [void][IO.Directory]::CreateDirectory($secondWorking)
    $workingFingerprint = Get-BonsaiBuildFingerprint -RepositoryRoot $fixture -WorkingDirectory $firstWorking
    Assert ($workingFingerprint -ceq (Get-BonsaiBuildFingerprint -RepositoryRoot $fixture -WorkingDirectory $secondWorking)) 'Unique external working directories invalidated unchanged builds.'
    $workingConfig = Join-Path -Path $scratch -ChildPath 'work/.cargo' -AdditionalChildPath 'config.toml'
    [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($workingConfig))
    [IO.File]::WriteAllText($workingConfig, '[build]')
    Assert ($workingFingerprint -cne (Get-BonsaiBuildFingerprint -RepositoryRoot $fixture -WorkingDirectory $firstWorking)) 'Cargo configuration above the native working directory was omitted.'
    [IO.File]::Delete($workingConfig)
    [void][IO.Directory]::CreateDirectory((Join-Path $fixture 'tools/validation/obj'))
    [IO.File]::WriteAllText((Join-Path $fixture 'tools/validation/obj/generated.txt'), 'compiler output')
    Assert ((Get-BonsaiBuildFingerprint -RepositoryRoot $fixture) -ceq $changed) 'Nested documented validation outputs invalidated build receipts.'
    $config = Join-Path -Path $fixture -ChildPath '.cargo' -AdditionalChildPath 'config.toml'
    [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($config))
    [IO.File]::WriteAllText($config, '[build]')
    $configured = Get-BonsaiBuildFingerprint -RepositoryRoot $fixture
    Assert ($configured -cne $changed -and $configured -ceq (Get-BonsaiBuildFingerprint -RepositoryRoot $fixture)) 'Cargo config was omitted or stable config disabled reuse.'
    [IO.File]::WriteAllText($config, 'paths = ["C:/external-source"]')
    Assert ((Get-BonsaiBuildFingerprint -RepositoryRoot $fixture) -cne (Get-BonsaiBuildFingerprint -RepositoryRoot $fixture)) 'External Cargo source overrides were cached without their contents.'
    [IO.File]::WriteAllText($config, '[patch.crates-io]' + "`n" + 'example = { path = "C:/external-source" }')
    Assert ((Get-BonsaiBuildFingerprint -RepositoryRoot $fixture) -cne (Get-BonsaiBuildFingerprint -RepositoryRoot $fixture)) 'An inline external Cargo patch was cached without its contents.'
    [IO.File]::Delete($config)
    [IO.File]::WriteAllText((Join-Path $fixture 'Cargo.toml'), 'dependency = { path = "../external-source" }')
    Assert ((Get-BonsaiBuildFingerprint -RepositoryRoot $fixture) -cne (Get-BonsaiBuildFingerprint -RepositoryRoot $fixture)) 'External path dependencies were cached without their contents.'
    [IO.File]::WriteAllText((Join-Path $fixture 'Cargo.toml'), 'dependency = { path = "C:/external-source" }')
    Assert ((Get-BonsaiBuildFingerprint -RepositoryRoot $fixture) -cne (Get-BonsaiBuildFingerprint -RepositoryRoot $fixture)) 'An absolute external dependency was mistaken for an in-tree path.'
    if ($IsWindows) {
        $alias = Join-Path $fixture 'linked-source'
        $null = New-Item -ItemType Junction -Path $alias -Target (Join-Path $fixture 'src')
        Assert-Failure { Get-BonsaiBuildFingerprint -RepositoryRoot $fixture } 'Reparse-point source inputs were accepted.'
    }
}
if ($Case -cin @('All', 'Receipt')) {
    $fingerprint = Get-BonsaiBuildFingerprint -RepositoryRoot $fixture
    $executable = Join-Path $scratch 'app.exe'
    $receipt = Join-Path $scratch 'build.json'
    [IO.File]::WriteAllText($executable, 'verified executable fixture')
    $parameters = @{ ReceiptPath = $receipt; Fingerprint = $fingerprint; Executable = @($executable) }
    Assert (-not (Test-BonsaiBuildReceipt @parameters)) 'A missing receipt was trusted.'
    Write-BonsaiBuildReceipt @parameters
    Assert (Test-BonsaiBuildReceipt @parameters) 'A current build receipt was rejected.'
    $different = '0' * 64
    Assert (-not (Test-BonsaiBuildReceipt -ReceiptPath $receipt -Fingerprint $different -Executable @($executable))) 'A different source fingerprint was accepted.'
    $other = Join-Path $scratch 'other.exe'
    [IO.File]::Copy($executable, $other)
    Assert (-not (Test-BonsaiBuildReceipt -ReceiptPath $receipt -Fingerprint $fingerprint -Executable @($other))) 'Equal bytes at a different executable path were accepted.'
    [IO.File]::WriteAllText($executable, 'modified executable fixture')
    Assert (-not (Test-BonsaiBuildReceipt @parameters)) 'Changed executable bytes reached the launcher.'
    Write-BonsaiBuildReceipt @parameters
    [IO.File]::Delete($executable)
    Assert (-not (Test-BonsaiBuildReceipt @parameters)) 'A missing executable was accepted.'
    foreach ($invalid in @('{', '{}', '{"Version":1,"Fingerprint":"invalid","Outputs":[]}')) {
        [IO.File]::WriteAllText($receipt, $invalid)
        Assert (-not (Test-BonsaiBuildReceipt @parameters)) 'A malformed build receipt was accepted.'
    }
    Assert-Failure { Write-BonsaiBuildReceipt -ReceiptPath (Join-Path $repo 'build.json') -Fingerprint $fingerprint -Executable @($other) } 'A cache receipt was written inside Git.'
}
if ($Case -cin @('All', 'Invalidation')) {
    [void][IO.Directory]::CreateDirectory((Join-Path $fixture 'tools'))
    [void][IO.Directory]::CreateDirectory((Join-Path $fixture 'src/app'))
    [void][IO.Directory]::CreateDirectory((Join-Path $fixture 'src/dependency'))
    [IO.File]::WriteAllText((Join-Path $fixture 'src/app/Cargo.toml'), "[package]`nname = `"app`"`nversion = `"0.1.0`"")
    [IO.File]::WriteAllText((Join-Path $fixture 'src/dependency/Cargo.toml'), "[package]`nname = `"dependency`"`nversion = `"0.1.0`"")
    & $module {
        $script:exitCode = 0
        function script:Invoke-BonsaiNativeProcess {
            [CmdletBinding()]
            [OutputType([pscustomobject])]
            param([string]$Executable, [string[]]$ArgumentList, [string]$WorkingDirectory, [string]$LogPath, [int]$TimeoutSeconds)
            $script:clean = @{ Executable = $Executable; Arguments = $ArgumentList; WorkingDirectory = $WorkingDirectory; LogPath = $LogPath; TimeoutSeconds = $TimeoutSeconds }
            return [pscustomobject]@{ ExitCode = $script:exitCode }
        }
    }
    $parameters = @{ RepositoryRoot = $fixture; CargoPath = 'cargo.exe'; Channel = 'nightly-2026-10-22-aarch64-pc-windows-msvc'; TargetDirectory = (Join-Path $scratch 'target'); WorkingDirectory = $scratch; BuildTarget = 'aarch64-pc-windows-msvc' }
    Clear-BonsaiWorkspaceBuild @parameters
    $record = & $module { $script:clean }
    Assert ($record.Executable -ceq 'cargo.exe' -and $record.Arguments[1] -ceq 'clean' -and
        $record.Arguments -ccontains 'app' -and $record.Arguments -ccontains 'dependency' -and
        $record.Arguments -ccontains '--release' -and $record.Arguments -ccontains 'aarch64-pc-windows-msvc' -and
        $record.WorkingDirectory -ceq $scratch) 'Workspace invalidation missed local dependencies or its pinned/external build contract.'
    & $module { $script:exitCode = 7 }
    Assert-Failure { Clear-BonsaiWorkspaceBuild @parameters } 'Failed workspace invalidation was accepted.'
}
Write-Output "Build cache $Case checks passed. Evidence: $scratch"
