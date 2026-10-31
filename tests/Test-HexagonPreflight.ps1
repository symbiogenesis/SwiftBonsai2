#requires -Version 7.6
<# .SYNOPSIS
Checks Hexagon inventory bounds and fail-closed evidence without probing a device.
#>
[CmdletBinding()]
[OutputType([string])]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.Hexagon.psm1')
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('Bonsai-Hexagon-Preflight-Test-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($scratch)
try {
    $receipt = New-BonsaiHexagonReceipt -Inventory $null -CollectionStatus 'deadline-expired' -MaximumSeconds 5 -ElapsedMilliseconds 3000
    Assert ($receipt.Qualification -ceq 'pending' -and -not $receipt.ProfileEligible -and $receipt.Execution -ceq 'not-attempted') 'Timeout admitted an accelerator.'
    Assert ($receipt.Inventory -eq $null -and $receipt.Blockers.Count -ge 4) 'Unknown evidence or blockers were dropped.'
    $facts = @{ OsArchitecture = 'Arm64'; ProcessArchitecture = 'Arm64'; Drivers = @(@{ IsSigned = $true }); Package = @{ Status = 'inventoried'; Files = @(@{ Name = 'libggml-htp-v81.so'; Signature = @{ Status = 'Valid' } }) } }
    $receipt = New-BonsaiHexagonReceipt -Inventory $facts -CollectionStatus 'collected' -MaximumSeconds 5 -ElapsedMilliseconds 1
    Assert (-not $receipt.ProfileEligible -and $receipt.Execution -ceq 'not-attempted' -and $receipt.Blockers.Count -eq 3) 'Signed-looking inventory was treated as deployment/execution proof.'
    Assert (-not $receipt.TrustChanged -and -not $receipt.DriverInstalled -and -not $receipt.ModelLoaded) 'Inventory side-effect contract changed.'
    $package = Get-BonsaiHexagonPackage
    Assert ($package.Status -ceq 'not-selected' -and $package.Files.Count -eq 0) 'Missing package was treated as inspected.'
    [IO.File]::WriteAllBytes((Join-Path $scratch 'libggml-htp-v81.so'), [byte[]]@(1, 2, 3))
    [IO.File]::WriteAllText((Join-Path $scratch 'model.gguf'), 'must-not-hash')
    $nested = Join-Path $scratch 'nested'
    [void][IO.Directory]::CreateDirectory($nested)
    [IO.File]::WriteAllBytes((Join-Path $nested 'hidden.so'), [byte[]]@(4))
    $package = Get-BonsaiHexagonPackage -PackagePath $scratch
    Assert ($package.Files.Count -eq 1 -and $package.TotalBytes -eq 3) 'Package inventory traversed nested entries or read model bytes.'
    Assert ($package.Files[0].Signature -eq $null -and $package.Files[0].Sha256 -ceq '039058c6f2c0cb492c533b0a4d14ef77cc0f78abccced5287d84a1a2011cfb81') 'DSP hash or unknown catalog binding changed.'
    Assert-Failure { Get-BonsaiHexagonLocalPath -Path 'relative' } 'Relative package path admitted.'
    Assert-Failure { Get-BonsaiHexagonLocalPath -Path ($scratch + ':stream') } 'Alternate stream path admitted.'
    $large = [IO.File]::Open((Join-Path $scratch 'oversized.so'), 'CreateNew', 'Write', 'None')
    try { $large.SetLength(128MB + 1) } finally { $large.Dispose() }
    Assert-Failure { Get-BonsaiHexagonPackage -PackagePath $scratch } 'Oversized package file admitted.'
    Remove-Item -LiteralPath (Join-Path $scratch 'oversized.so')
    foreach ($number in 1..129) { [IO.File]::WriteAllBytes((Join-Path $scratch ('ignored-' + $number)), [byte[]]::new(0)) }
    Assert-Failure { Get-BonsaiHexagonPackage -PackagePath $scratch } 'Top-level enumeration exceeded its bound.'
    $newReceipt = Join-Path $scratch 'receipt.json'
    Assert ((Get-BonsaiHexagonLocalPath -Path $newReceipt -NewFile) -ceq $newReceipt) 'Safe new external receipt path rejected.'
    [IO.File]::WriteAllText($newReceipt, 'sentinel')
    Assert-Failure { Get-BonsaiHexagonLocalPath -Path $newReceipt -NewFile } 'Existing receipt admitted for overwrite.'
    Assert-Failure { Get-BonsaiHexagonLocalPath -Path (Join-Path $PSScriptRoot 'receipt.json') -NewFile } 'Receipt output inside Git admitted.'
    # Run the real worker supervisor against a copied, synthetic inventory module.
    # Neither child queries CIM, signatures or hardware; the delay exercises teardown.
    foreach ($scenario in @('success', 'timeout')) {
        $fixture = Join-Path $scratch $scenario
        [void][IO.Directory]::CreateDirectory($fixture)
        $entry = Join-Path $fixture 'Invoke-HexagonPreflight.ps1'
        [IO.File]::Copy((Join-Path $PSScriptRoot '../scripts/Invoke-HexagonPreflight.ps1'), $entry)
        $source = [IO.File]::ReadAllText((Join-Path $PSScriptRoot '../scripts/Bonsai.Hexagon.psm1'))
        $delay = if ($scenario -ceq 'timeout') { 'Start-Sleep -Seconds 30' } else { '' }
        $source += @"

function Get-BonsaiHexagonInventory {
    [CmdletBinding()]
    [OutputType([Collections.Specialized.OrderedDictionary])]
    param([string]`$PackagePath)
    $delay
    return [ordered]@{ OsArchitecture = 'Arm64'; ProcessArchitecture = 'Arm64'; Drivers = @(); Package = @{ Status = 'not-selected'; Files = @() } }
}
Export-ModuleMember -Function Get-BonsaiHexagonLocalPath, Get-BonsaiHexagonPackage, Get-BonsaiHexagonInventory, New-BonsaiHexagonReceipt
"@
        [IO.File]::WriteAllText((Join-Path $fixture 'Bonsai.Hexagon.psm1'), $source)
        $reportPath = Join-Path $fixture 'receipt.json'
        $receipt = & $entry -OutputPath $reportPath -MaximumSeconds 5 -PassThru
        $expected = if ($scenario -ceq 'timeout') { 'deadline-expired' } else { 'collected' }
        Assert ($receipt.CollectionStatus -ceq $expected -and $receipt.ElapsedMilliseconds -lt 5000) 'Bounded worker collection or cleanup failed.'
        Assert (-not $receipt.ProfileEligible -and [IO.File]::Exists($reportPath)) 'Worker collection admitted execution or lost its receipt.'
    }
    Write-Output 'Hexagon preflight checks passed: bounded package hashes, no recursive/model reads, unknown deployment and fail-closed qualification.'
} finally {
    $resolved = [IO.Path]::GetFullPath($scratch)
    $parent = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
    if (-not $resolved.StartsWith($parent, [StringComparison]::OrdinalIgnoreCase) -or
        [IO.Path]::GetFileName($resolved) -cnotmatch '^Bonsai-Hexagon-Preflight-Test-[a-f0-9]{32}$') { throw 'Unsafe preflight fixture cleanup path.' }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
