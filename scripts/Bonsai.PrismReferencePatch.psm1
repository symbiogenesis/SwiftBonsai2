#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')

function Initialize-BonsaiPrismReferencePatch {
    <#
    .SYNOPSIS
    Creates an explicit, byte-verified production source fix outside the original tree.
    #>
    [CmdletBinding(SupportsShouldProcess)]
    [OutputType([hashtable])]
    param([Parameter(Mandatory)][string]$Repository,
          [Parameter(Mandatory)][string]$SourceDirectory,
          [Parameter(Mandatory)][string]$Directory)
    $source = Resolve-ExternalPath $SourceDirectory
    $output = Resolve-ExternalPath $Directory
    if ($output.Equals($source, [StringComparison]::OrdinalIgnoreCase) -or
        $output.StartsWith($source.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Production source substitutions must remain outside the verified source tree.'
    }
    $lock = Get-Content -LiteralPath (Join-Path $Repository 'tools/prism/upstream/reference-patches.lock.json') -Raw | ConvertFrom-Json -AsHashtable
    if ($lock.SchemaVersion -ne 1 -or $lock.SourceCommit -cne 'b87ca7225456b06b0975b8239c66b5aecf07e37c' -or
        $lock.Patch.RelativeSource -cne 'src/llama-model-saver.cpp' -or
        $lock.Patch.PatchPath -cne 'tools/prism/upstream/patches/dspark-saver.patch') {
        throw 'Unexpected reference patch identity.'
    }
    $original = Join-Path $source $lock.Patch.RelativeSource
    $before = (Get-FileHash -LiteralPath $original -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($before -cne $lock.Patch.BeforeSha256) { throw 'Reference patch input differs from its exact upstream pin.' }
    $patch = [IO.File]::ReadAllText((Join-Path $Repository $lock.Patch.PatchPath)).Replace("`r`n", "`n")
    $patchHash = [Convert]::ToHexStringLower([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($patch)))
    if ($patchHash -cne $lock.Patch.CanonicalPatchSha256) { throw 'Reference patch content differs from its explicit pin.' }
    $lines = $patch.Split("`n")
    if ($lines.Count -lt 4 -or $lines[0] -cne '--- a/src/llama-model-saver.cpp' -or
        $lines[1] -cne '+++ b/src/llama-model-saver.cpp' -or $lines[2] -cne '@@ -347,1 +347,22 @@') {
        throw 'Only the reviewed two-hunk reference patch is accepted.'
    }
    $originalLines = [IO.File]::ReadAllText($original).Replace("`r`n", "`n").Split("`n")
    $result = [Collections.Generic.List[string]]::new()
    $oldIndex = 0
    $patchIndex = 2
    foreach ($hunk in @(@{ Header='@@ -347,1 +347,22 @@'; Start=346; Count=22 }, @{ Header='@@ -444,1 +465,19 @@'; Start=443; Count=19 })) {
        if ($lines[$patchIndex] -cne $hunk.Header) { throw 'Reference patch hunk identity differs.' }
        while ($oldIndex -lt $hunk.Start) { $result.Add($originalLines[$oldIndex]); $oldIndex++ }
        $patchIndex++
        $oldCount = 0
        $newCount = 0
        while ($patchIndex -lt $lines.Count -and -not $lines[$patchIndex].StartsWith('@@', [StringComparison]::Ordinal)) {
            $line = $lines[$patchIndex]
            if ($patchIndex -eq $lines.Count - 1 -and $line.Length -eq 0) { break }
            if ($line.Length -lt 1 -or $line[0] -cnotin @(' ', '+', '-')) { throw 'Invalid reference patch hunk line.' }
            $value = $line.Substring(1)
            if ($line[0] -ne '+') {
                if ($oldIndex -ge $originalLines.Count -or $originalLines[$oldIndex] -cne $value) { throw 'Reference patch context differs.' }
                $oldIndex++
                $oldCount++
            }
            if ($line[0] -ne '-') { $result.Add($value); $newCount++ }
            $patchIndex++
        }
        if ($oldCount -ne 1 -or $newCount -ne $hunk.Count) { throw 'Reference patch extent differs.' }
    }
    if ($patchIndex -ne $lines.Count - 1 -or $lines[$patchIndex].Length -ne 0) { throw 'Unexpected reference patch suffix.' }
    for ($i = $oldIndex; $i -lt $originalLines.Count; $i++) { $result.Add($originalLines[$i]) }
    $bytes = [Text.Encoding]::UTF8.GetBytes($result -join "`n")
    $after = [Convert]::ToHexStringLower([Security.Cryptography.SHA256]::HashData($bytes))
    if ($after -cne $lock.Patch.AfterSha256) { throw 'Patched production source differs from its reviewed byte identity.' }
    $destination = Join-Path $output 'llama-model-saver.cpp'
    if (Test-Path -LiteralPath $destination) {
        Test-LockedArtifact -Path $destination -Bytes $bytes.Length -Sha256 $after
    } else {
        if (-not $PSCmdlet.ShouldProcess($destination, 'Write verified production saver fix outside the unchanged source tree')) { return }
        [void][IO.Directory]::CreateDirectory($output)
        [IO.File]::WriteAllBytes($destination, $bytes)
    }
    return @{ Variant=$lock.Variant; Path=$destination; RelativeSource=$lock.Patch.RelativeSource; BeforeSha256=$before; AfterSha256=$after; CanonicalPatchSha256=$patchHash }
}

Export-ModuleMember -Function Initialize-BonsaiPrismReferencePatch
