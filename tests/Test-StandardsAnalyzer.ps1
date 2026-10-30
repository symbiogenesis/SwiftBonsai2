#requires -Version 7.6
<# .SYNOPSIS
Checks the export-rule scheduling regression with one synthetic module and one real file.
#>
[CmdletBinding()]
[OutputType([string])]
param([Parameter(Mandatory)][string]$AnalyzerModulePath)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
# Match the standards runner's module context that exposes the PowerShell 7.6 race.
Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.Native.psm1')
$analyzer = Import-Module -Name $AnalyzerModulePath -PassThru -Force
if ($analyzer.Name -cne 'PSScriptAnalyzer' -or $analyzer.Version -ne [version]'1.25.0') { throw 'Use the exact pinned analyzer.' }
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Standards.psm1')
$settings = Join-Path $PSScriptRoot '../PSScriptAnalyzerSettings.psd1'
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Analyzer-Export-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($scratch)
$fixture = Join-Path $scratch 'Exported.psm1'
$fixtureText = @(
    'function Get-Thing* {',
    '    [CmdletBinding()]',
    '    [OutputType([string])]',
    '    param()',
    "    Write-Host 'trace'",
    "    return 'café'",
    '}',
    "Export-ModuleMember -Function 'Get-Thing*'"
) -join "`n"
[IO.File]::WriteAllText($fixture, $fixtureText, [Text.UTF8Encoding]::new($false))
$diagnostics = @(Invoke-BonsaiStandardsAnalyzer -Path $fixture -Settings $settings)
foreach ($rule in @('PSReservedCmdletChar', 'PSProvideCommentHelp', 'PSAvoidUsingWriteHost')) {
    if ($rule -cnotin $diagnostics.RuleName) { throw "The scheduled analyzer lost required rule $rule." }
}
if ('PSUseBOMForUnicodeEncodedFile' -cin $diagnostics.RuleName) { throw 'The existing configured exclusion changed.' }
$real = @(Invoke-BonsaiStandardsAnalyzer -Path (Join-Path $PSScriptRoot '../scripts/Bonsai.Capabilities.psm1') -Settings $settings)
if ($real.Count) { throw 'The unchanged capabilities module produced diagnostics.' }
Write-Output 'Focused export scheduling checks passed; both export rules and ordinary rules remain enforced.'
