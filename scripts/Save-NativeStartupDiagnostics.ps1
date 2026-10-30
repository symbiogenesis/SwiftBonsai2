#requires -Version 7.6
<# .SYNOPSIS
Retains bounded, credential-checked native worker logs before CI temp cleanup.
#>
[CmdletBinding()]
[OutputType([void])]
param(
    [Parameter(Mandatory)][string]$EvidenceDirectory,
    [Nullable[int]]$AcceptanceExitCode,
    [switch]$BudgetExpired
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$maximumBytes = 65536
$maximumSessions = 8
$encoding = [Text.UTF8Encoding]::new($false, $true)
$receipt = [ordered]@{
    SchemaVersion=1; Scope='bounded-credential-checked-native-startup-diagnostics'
    AcceptanceProcessExitCode=$AcceptanceExitCode; AggregateBudgetExpired=$BudgetExpired.IsPresent
    MaximumLogBytes=$maximumBytes; MaximumSessions=$maximumSessions
    Classification='not-collected'; Sources=@(); PublishedFiles=@()
}
$rootValidated = $false

function Read-BoundedByte {
    [CmdletBinding()]
    [OutputType([byte[]])]
    param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][int]$Maximum)
    $item = Get-Item -LiteralPath $Path -Force
    if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw [IO.InvalidDataException]::new('unsafe-file')
    }
    if ($item.Length -gt $Maximum) { throw [IO.InvalidDataException]::new('read-bound-exceeded') }
    $stream = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    try {
        $buffer = [byte[]]::new($Maximum + 1)
        $count = 0
        do {
            $read = $stream.Read($buffer, $count, $buffer.Length - $count)
            $count += $read
        } while ($read -gt 0 -and $count -lt $buffer.Length)
        if ($count -gt $Maximum) { throw [IO.InvalidDataException]::new('read-bound-exceeded') }
        $result = [byte[]]::new($count)
        [Array]::Copy($buffer, $result, $count)
        return $result
    } finally { $stream.Dispose() }
}

$outputs = @('startup-backend.stdout.txt', 'startup-backend.stderr.txt')
try {
    $root = Get-Item -LiteralPath $EvidenceDirectory -Force
    if (-not $root.PSIsContainer -or ($root.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw [IO.InvalidDataException]::new('unsafe-evidence-directory')
    }
    $rootValidated = $true
    # Delete only our two fixed public outputs, so a failed collection cannot
    # leave stale logs behind. Never copy or upload the protected directories.
    foreach ($name in @($outputs) + 'startup-diagnostics.json') {
        $path = Join-Path $root.FullName $name
        if (Test-Path -LiteralPath $path) {
            if ((Get-Item -LiteralPath $path -Force).PSIsContainer) { throw [IO.InvalidDataException]::new('unsafe-output') }
            Remove-Item -LiteralPath $path -Force
        }
    }
    $directories = [Collections.Generic.List[string]]::new()
    foreach ($directory in [IO.Directory]::EnumerateDirectories($root.FullName)) {
        if ([IO.Path]::GetFileName($directory) -cnotmatch '^SwiftBonsai-Keys-[a-f0-9]{32}$') { continue }
        $directories.Add($directory)
        if ($directories.Count -gt $maximumSessions) {
            $receipt.Classification = 'session-bound-exceeded'
            return
        }
    }
    if ($directories.Count -eq 0) { $receipt.Classification = 'no-protected-worker-directory'; return }
    $keys = [Collections.Generic.List[string]]::new()
    # Validate every actual key first, so a log cannot disclose another
    # session's credential. Failure closes publication for every raw log.
    try {
        foreach ($directory in $directories) {
            if ((Get-Item -LiteralPath $directory -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {
                throw [IO.InvalidDataException]::new('unsafe-session-directory')
            }
            foreach ($name in @('frontend.key', 'backend.key')) {
                $bytes = [byte[]]@(Read-BoundedByte -Path (Join-Path $directory $name) -Maximum 64)
                $key = $encoding.GetString([byte[]]$bytes)
                if ($key -cnotmatch '^[a-f0-9]{64}$') { throw [IO.InvalidDataException]::new('invalid-key') }
                $keys.Add($key)
            }
        }
    } catch { $receipt.Classification = 'key-validation-failed'; return }
    $sources = [Collections.Generic.List[object]]::new()
    $published = [Collections.Generic.List[object]]::new()
    foreach ($name in @('backend.stdout', 'backend.stderr')) {
        $combined = [Text.StringBuilder]::new()
        for ($index = 0; $index -lt $directories.Count; $index++) {
            $source = [ordered]@{ Session=($index + 1); Name=$name; Classification='not-read'; Published=$false }
            $sources.Add($source)
            $path = Join-Path $directories[$index] $name
            if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { $source.Classification='missing'; continue }
            try {
                $bytes = [byte[]]@(Read-BoundedByte -Path $path -Maximum $maximumBytes)
            } catch [IO.InvalidDataException] {
                $source.Classification = if ($_.Exception.Message -ceq 'read-bound-exceeded') { 'read-bound-exceeded' } else { 'unsafe-file' }
                continue
            } catch { $source.Classification='read-failed'; continue }
            try { $text = $encoding.GetString([byte[]]$bytes) }
            catch { $source.Classification='invalid-utf8'; continue }
            foreach ($key in $keys) { $text = $text.Replace($key, '[REDACTED]', [StringComparison]::OrdinalIgnoreCase) }
            # Also cover unrecognized hex credentials and remove terminal controls.
            $text = [regex]::Replace($text, '(?i)[a-f0-9]{64,}', '[REDACTED]')
            $text = [regex]::Replace($text, '[\x00-\x08\x0B\x0C\x0E-\x1F\x7F]', '?')
            if (@($keys | Where-Object { $text.Contains($_, [StringComparison]::OrdinalIgnoreCase) }).Count) {
                $source.Classification='credential-check-failed'; continue
            }
            [void]$combined.Append("--- worker session $($index + 1) ---`n")
            [void]$combined.Append($text)
            [void]$combined.Append("`n")
            $source.Classification='sanitized'; $source.Published=$true
        }
        if ($combined.Length -eq 0) { continue }
        $text = $combined.ToString()
        if (@($keys | Where-Object { $text.Contains($_, [StringComparison]::OrdinalIgnoreCase) }).Count) {
            $receipt.Classification='credential-check-failed'; return
        }
        $outputName = 'startup-' + $name + '.txt'
        $output = Join-Path $root.FullName $outputName
        [IO.File]::WriteAllText($output, $text, $encoding)
        $published.Add([ordered]@{
            Name=$outputName; Bytes=(Get-Item -LiteralPath $output).Length
            Sha256=(Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash.ToLowerInvariant()
        })
    }
    $receipt.Sources = @($sources)
    $receipt.PublishedFiles = @($published)
    $receipt.Classification = if ($published.Count) { 'bounded-sanitized-logs-retained' } else { 'no-log-published' }
} catch {
    # Never serialize exception messages: they can include private paths/data.
    $receipt.Classification='collection-failed'
} finally {
    if ($rootValidated) {
        $receipt | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $EvidenceDirectory 'startup-diagnostics.json') -Encoding utf8NoBOM
    }
}
