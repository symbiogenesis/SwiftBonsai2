#requires -Version 7.6
<# .SYNOPSIS
Requires hosted candidate preparation to reject before writes outside its exact CI context.
#>
[CmdletBinding()]
[OutputType([string])]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
$output = Join-Path ([IO.Path]::GetTempPath()) ('Bonsai-RejectedHeadless-' + [guid]::NewGuid().ToString('N'))
$script = Join-Path $PSScriptRoot '../scripts/Build-HeadlessPublisherCandidate.ps1'
$previousRef = [Environment]::GetEnvironmentVariable('GITHUB_REF')
try {
    $env:GITHUB_REF = 'refs/heads/model-free-guard-fixture'
    $message = Assert-Failure -PassThru -Action { & $script -OutputDirectory $output }
    Assert ($message -ceq 'Headless publisher preparation requires this repository main on hosted native Windows ARM64 CI.') 'The guard did not reject before native preparation.'
    Assert (-not (Test-Path -LiteralPath $output)) 'Rejected preparation created an output directory.'
} finally {
    if ($null -eq $previousRef) { Remove-Item -LiteralPath Env:GITHUB_REF -ErrorAction SilentlyContinue }
    else { $env:GITHUB_REF = $previousRef }
}
# Exercise only the extraction command wiring with a mocked native boundary;
# no compiler archive, extraction tool, build or model is needed locally.
$tokens = $null
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($script, [ref]$tokens, [ref]$errors)
Assert ($errors.Count -eq 0) 'Candidate builder has a PowerShell syntax error.'
$definition = $ast.Find({ param($node)
    $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq 'Expand-CandidateLlvmArchive'
}, $true)
Assert ($null -ne $definition) 'The bounded LLVM extraction boundary is missing.'
& {
    param([scriptblock]$Definition, [string]$FixtureRoot)
    $calls = [Collections.Generic.List[object]]::new()
    $failDecompression = $false
    function Invoke-CandidateTool {
        [CmdletBinding()]
        [OutputType([pscustomobject])]
        param([string]$Executable, [string[]]$Arguments, [string]$LogName, [int]$TimeoutSeconds)
        $calls.Add(@{ Executable=$Executable; Arguments=$Arguments; LogName=$LogName; TimeoutSeconds=$TimeoutSeconds })
        if ($failDecompression -and $Executable -ceq 'fixture-zstd.exe') { throw 'synthetic decoder failure' }
        [pscustomobject]@{ ExitCode=0 }
    }
    . $Definition
    $archive = Join-Path $FixtureRoot 'llvm.tar.zst'
    $plainTar = Join-Path $FixtureRoot 'llvm.tar'
    $destination = Join-Path $FixtureRoot 'llvm'
    Expand-CandidateLlvmArchive -Zstd 'fixture-zstd.exe' -Tar 'fixture-tar.exe' -Archive $archive -Destination $destination
    Assert ($calls.Count -eq 2) 'LLVM extraction did not use separate decoder and tar operations.'
    Assert ($calls[0].Executable -ceq 'fixture-zstd.exe' -and $calls[0].LogName -ceq 'llvm-decompress.txt' -and
        ($calls[0].Arguments -join '|') -ceq (@('--decompress', '--long=31', '-o', $plainTar, $archive) -join '|')) 'LLVM decoder must use the observed zstd 1.5.7 -o output syntax and explicit window bound.'
    Assert ($calls[1].Executable -ceq 'fixture-tar.exe' -and $calls[1].LogName -ceq 'llvm-extract.txt' -and
        ($calls[1].Arguments -join '|') -ceq (@('-xf', $plainTar, '-C', $destination) -join '|')) 'System tar was given compressed LLVM input.'
    Assert ($calls[0].TimeoutSeconds -eq 300 -and $calls[1].TimeoutSeconds -eq 300) 'An extraction operation lost its deadline.'
    $calls.Clear()
    $failDecompression = $true
    $message = Assert-Failure -PassThru -Action {
        Expand-CandidateLlvmArchive -Zstd 'fixture-zstd.exe' -Tar 'fixture-tar.exe' -Archive $archive -Destination $destination
    }
    Assert ($message -ceq 'synthetic decoder failure' -and $calls.Count -eq 1) 'A failed decoder still reached tar extraction.'
} ([scriptblock]::Create($definition.Extent.Text)) $output
Write-Output 'Hosted guard and bounded two-stage LLVM extraction checks passed without writes, builds, probes or model access.'
