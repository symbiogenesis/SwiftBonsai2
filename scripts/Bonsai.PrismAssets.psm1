#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')

function Save-BonsaiPrismMetadataAsset {
    <#
    .SYNOPSIS
    Materializes immutable, bounded metadata inputs outside the checkout.
    #>
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Called only by explicitly requested Prism test execution, writing fresh external evidence.')]
    [CmdletBinding()]
    [OutputType([hashtable])]
    param([Parameter(Mandatory)][string]$Repository,
          [Parameter(Mandatory)][string]$Directory)
    $root = Resolve-ExternalPath $Directory
    [void][IO.Directory]::CreateDirectory($root)
    $lockPath = Join-Path $Repository 'tools/prism/upstream/metadata-assets.lock.json'
    $lock = Get-Content -LiteralPath $lockPath -Raw | ConvertFrom-Json -AsHashtable
    if ($lock.SchemaVersion -ne 1 -or $lock.Models.Count -ne 11) { throw 'Unexpected metadata fixture lock schema.' }
    $client = [Net.Http.HttpClient]::new()
    $client.Timeout = [TimeSpan]::FromSeconds(120)
    $receipts = [Collections.Generic.List[object]]::new()
    try {
        foreach ($model in $lock.Models) {
            if ($model.Repo -cnotmatch '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$' -or
                $model.Revision -cnotmatch '^[0-9a-f]{40}$' -or $model.Files.Count -lt 1 -or $model.Files.Count -gt 256) {
                throw 'Invalid immutable metadata model identity.'
            }
            $provider = if ($model.ContainsKey('ProviderRepo')) { $model.ProviderRepo } else { $model.Repo }
            if ($provider -cnotmatch '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$') { throw 'Invalid metadata provider repository.' }
            foreach ($asset in @($model.Api) + @($model.Files)) {
                if ($asset.Path -cnotmatch '^metadata/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$' -or
                    ($asset.Bytes -isnot [int] -and $asset.Bytes -isnot [long]) -or $asset.Bytes -lt 1 -or $asset.Bytes -gt 67108864 -or
                    $asset.Sha256 -cnotmatch '^[0-9a-f]{64}$') {
                    throw 'Invalid bounded metadata asset identity.'
                }
                $destination = Resolve-ExternalPath (Join-Path $root $asset.Path)
                [void][IO.Directory]::CreateDirectory((Split-Path $destination -Parent))
                if (Test-Path -LiteralPath $destination) { throw 'Metadata fixture destination must be fresh.' }
                if ($asset.ContainsKey('CorpusPath')) {
                    if ($asset.CorpusPath -cnotmatch '^tools/prism/upstream/metadata-api/[A-Za-z0-9_.-]+\.json$' -or
                        $asset.Url -cne "https://huggingface.co/api/models/$provider/revision/$($model.Revision)") {
                        throw 'Invalid frozen API corpus provenance.'
                    }
                    # API listings include changing activity fields even at a revision URL.
                    # Use the original raw response corpus, never a decoded expected schema.
                    Copy-Item -LiteralPath (Join-Path $Repository $asset.CorpusPath) -Destination $destination
                } else {
                    if ($asset.Filename -cnotmatch '^[A-Za-z0-9_./-]+\.gguf$' -or
                        $asset.Filename.Split('/') -ccontains '..' -or
                        $asset.Status -ne 206 -or $asset.RangeBytes -ne $asset.Bytes -or
                        $asset.Url -cne "https://huggingface.co/$provider/resolve/$($model.Revision)/$($asset.Filename)") {
                        throw 'Invalid immutable GGUF range provenance.'
                    }
                    $request = [Net.Http.HttpRequestMessage]::new([Net.Http.HttpMethod]::Get, $asset.Url)
                    $request.Headers.Range = [Net.Http.Headers.RangeHeaderValue]::new(0, $asset.RangeBytes - 1)
                    $response = $null
                    try {
                        $response = $client.Send($request, [Net.Http.HttpCompletionOption]::ResponseHeadersRead)
                        $null = $response.EnsureSuccessStatusCode()
                        $range = $response.Content.Headers.ContentRange
                        if ([int]$response.StatusCode -ne 206 -or $null -eq $range -or $range.From -ne 0 -or
                            $range.To -ne $asset.RangeBytes - 1 -or $response.Content.Headers.ContentLength -gt $asset.Bytes) {
                            throw 'Server did not honor the exact bounded GGUF prefix request.'
                        }
                        $inputStream = $response.Content.ReadAsStream()
                        $outputStream = [IO.FileStream]::new($destination, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
                        try {
                            $buffer = [byte[]]::new(65536)
                            while (($count = $inputStream.Read($buffer, 0, $buffer.Length)) -gt 0) {
                                if ($outputStream.Length + $count -gt $asset.Bytes) { throw 'GGUF prefix exceeds its pinned size.' }
                                $outputStream.Write($buffer, 0, $count)
                            }
                        } finally { $inputStream.Dispose(); $outputStream.Dispose() }
                    } finally { if ($response) { $response.Dispose() }; $request.Dispose() }
                }
                Test-LockedArtifact -Path $destination -Bytes $asset.Bytes -Sha256 $asset.Sha256
                $receipts.Add(@{ Path=$destination; Bytes=$asset.Bytes; Sha256=$asset.Sha256; Url=$asset.Url })
            }
        }
        # The same verified manifest is consumed by both safe Rust target runs.
        $manifestDestination = Join-Path $root 'metadata/manifest.json'
        Copy-Item -LiteralPath $lockPath -Destination $manifestDestination
        return @{ Assets=$receipts.ToArray(); Manifest=$manifestDestination; ManifestSha256=(Get-FileHash -LiteralPath $manifestDestination -Algorithm SHA256).Hash.ToLowerInvariant() }
    } finally { $client.Dispose() }
}

Export-ModuleMember -Function Save-BonsaiPrismMetadataAsset
