#requires -Version 7.6
<#
.SYNOPSIS
Captures CI-only native TTS timings and synthetic WAVs for independent listening.
.DESCRIPTION
Compares both pinned VoiceDesign quantizations and a bounded CPU thread grid.
Records cold readiness, warm generation, first audio and real-time factor. This
does not qualify human voice quality or infer Snapdragon results from hosted CPUs.
The only text and voice description are the fixed public evaluation fixtures.
#>
[CmdletBinding()]
[OutputType([pscustomobject])]
param([Parameter(Mandatory)][string]$DataDirectory,
      [string]$TtsPath, [string]$ToolPath, [string]$EvidenceDirectory,
      [ValidateRange(1, 3)][int]$Repetitions = 2,
      [ValidateSet('wav', 'pcm')][string]$ResponseFormat = 'pcm')
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if ($env:GITHUB_ACTIONS -cne 'true') { throw 'All TTS performance evaluations run only in CI.' }
if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -ne 'Arm64') {
    throw 'TTS evaluation requires native Windows ARM64.'
}
$repo = Split-Path $PSScriptRoot -Parent
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.NativeSpeech.psm1')
$data = Resolve-ExternalPath -Path $DataDirectory
$evidence = if ($EvidenceDirectory) { Resolve-ExternalPath -Path $EvidenceDirectory } else {
    Join-Path ([IO.Path]::GetTempPath()) ('Bonsai-TtsEvaluation-' + [guid]::NewGuid().ToString('N'))
}
if ([IO.Directory]::Exists($evidence)) { throw 'Use a new external evaluation evidence directory.' }
[void][IO.Directory]::CreateDirectory($evidence)
if ($env:GITHUB_ENV) { "BONSAI_TTS_EVIDENCE=$evidence" | Add-Content -LiteralPath $env:GITHUB_ENV }
$reportPath = Join-Path $evidence 'tts-evaluation.json'
@{ SchemaVersion = 1; Revision = $env:GITHUB_SHA; Status = 'preparing'; QualityVerdict = 'pending-human-listening' } |
    ConvertTo-Json | Set-Content -LiteralPath $reportPath
if (-not $TtsPath) {
    $built = Invoke-NativeSpeechBuild -DataDirectory $data -WorkingDirectory $evidence -RepositoryRoot $repo -Model QwenTts
    $TtsPath = $built.TtsPath
}
$TtsPath = Resolve-ExternalPath -Path $TtsPath
Assert-Arm64PE -Path $TtsPath
$keys = & (Join-Path $PSScriptRoot 'New-DevelopmentKeys.ps1') -ParentDirectory $evidence -ToolPath $ToolPath
$key = [IO.File]::ReadAllText($keys.FrontendKeyFile)
if ($key -cnotmatch '^[0-9a-f]{64}$') { throw 'Evaluation credential has invalid framing.' }
$handler = [Net.Http.HttpClientHandler]::new()
$handler.UseProxy = $false
$handler.AllowAutoRedirect = $false
$handler.UseCookies = $false
$client = [Net.Http.HttpClient]::new($handler)
$client.Timeout = [TimeSpan]::FromSeconds(190)
$client.DefaultRequestHeaders.Authorization = [Net.Http.Headers.AuthenticationHeaderValue]::new('Bearer', $key)
$key = $null
$available = [Environment]::ProcessorCount
if ($available -lt 1 -or $available -gt 256) { throw 'Evaluation CPU count exceeds the supported bound.' }
$threads = @([Math]::Max(1, [int][Math]::Floor($available / 2)), [Math]::Max(1, $available - 2), $available) | Select-Object -Unique
$cpu = (Get-ItemProperty -LiteralPath 'Registry::HKEY_LOCAL_MACHINE\HARDWARE\DESCRIPTION\System\CentralProcessor\0' -Name ProcessorNameString).ProcessorNameString
$report = [ordered]@{
    SchemaVersion = 1; Revision = $env:GITHUB_SHA; Status = 'running'; Cpu = $cpu; LogicalProcessors = $available
    Architecture = [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture.ToString()
    OS = [Runtime.InteropServices.RuntimeInformation]::OSDescription
    Runner = $env:RUNNER_NAME; ResponseFormat = $ResponseFormat; Repetitions = $Repetitions
    ColdReadyDefinition = 'New process; operating-system file cache is not flushed.'
    Sampling = 'Upstream defaults with random seeds; fixed public text and voice description.'
    Text = 'The quick brown fox crossed the quiet garden.'
    VoiceDescription = 'A calm, clear narrator with a natural speaking pace.'
    QualityVerdict = 'pending-human-listening'; Records = [Collections.Generic.List[object]]::new()
}
$completed = $false

function ConvertTo-EvaluationWave {
    [CmdletBinding()]
    [OutputType([byte[]])]
    param([Parameter(Mandatory)][byte[]]$Pcm)
    if ($Pcm.Length -eq 0 -or $Pcm.Length -gt 5760000 -or $Pcm.Length % 2 -ne 0) {
        throw 'TTS returned empty, oversized or unaligned PCM.'
    }
    $stream = [IO.MemoryStream]::new()
    $writer = [IO.BinaryWriter]::new($stream)
    try {
        $writer.Write([Text.Encoding]::ASCII.GetBytes('RIFF'))
        $writer.Write([uint32](36 + $Pcm.Length))
        $writer.Write([Text.Encoding]::ASCII.GetBytes('WAVEfmt '))
        $writer.Write([uint32]16); $writer.Write([uint16]1); $writer.Write([uint16]1)
        $writer.Write([uint32]24000); $writer.Write([uint32]48000)
        $writer.Write([uint16]2); $writer.Write([uint16]16)
        $writer.Write([Text.Encoding]::ASCII.GetBytes('data'))
        $writer.Write([uint32]$Pcm.Length); $writer.Write($Pcm)
        Write-Output -NoEnumerate -InputObject $stream.ToArray()
    } finally { $writer.Dispose(); $stream.Dispose() }
}

try {
    foreach ($quantization in @('Q8_0', 'Q4_K_M')) {
        $models = Install-NativeSpeechModels -DataDirectory $data -Model QwenTts -TtsQuantization $quantization -ToolPath $ToolPath
        foreach ($threadCount in $threads) {
            $lane = Join-Path $evidence ($quantization + '-' + $threadCount)
            [void][IO.Directory]::CreateDirectory($lane)
            $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
            $listener.Start()
            $port = $listener.LocalEndpoint.Port
            $listener.Stop()
            $address = 'http://127.0.0.1:' + $port
            $startInfo = [Diagnostics.ProcessStartInfo]::new($TtsPath)
            $startInfo.UseShellExecute = $false
            $startInfo.CreateNoWindow = $true
            $startInfo.WorkingDirectory = $lane
            foreach ($argument in @('--model', $models.TtsModelPath, '--codec', $models.TtsCodecPath,
                '--token-file', $keys.FrontendKeyFile, '--port', [string]$port,
                '--quantization', $quantization, '--threads', [string]$threadCount)) {
                $startInfo.ArgumentList.Add($argument)
            }
            $process = [Diagnostics.Process]::new()
            $process.StartInfo = $startInfo
            $cold = [Diagnostics.Stopwatch]::StartNew()
            $started = $false
            $readyDeadline = [Threading.CancellationTokenSource]::new([TimeSpan]::FromSeconds(180))
            try {
                if (-not $process.Start()) { throw 'Could not start the native evaluation companion.' }
                $started = $true
                $ready = $false
                while ($cold.Elapsed.TotalSeconds -lt 180 -and -not $process.HasExited) {
                    try {
                        $healthResponse = $client.GetAsync($address + '/health', $readyDeadline.Token).GetAwaiter().GetResult()
                        try {
                            if ($healthResponse.IsSuccessStatusCode) {
                                $health = $healthResponse.Content.ReadAsStringAsync().GetAwaiter().GetResult() | ConvertFrom-Json
                                $ready = $health.ready -and $health.model -ceq 'Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign'
                            }
                        } finally { $healthResponse.Dispose() }
                    } catch [Net.Http.HttpRequestException] { $ready = $false }
                    if ($ready) { break }
                    Start-Sleep -Milliseconds 100
                }
                $cold.Stop()
                if (-not $ready) { throw 'The verified native evaluation companion did not become ready.' }
                for ($iteration = 1; $iteration -le $Repetitions; $iteration++) {
                    $payload = @{
                        model = 'Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign'; input = $report.Text
                        voice_description = $report.VoiceDescription; language = 'Auto'; response_format = $ResponseFormat
                    } | ConvertTo-Json -Compress
                    $request = [Net.Http.HttpRequestMessage]::new([Net.Http.HttpMethod]::Post, $address + '/v1/audio/speech')
                    $request.Content = [Net.Http.StringContent]::new($payload, [Text.Encoding]::UTF8, 'application/json')
                    $response = $null
                    $buffered = [IO.MemoryStream]::new()
                    $clock = [Diagnostics.Stopwatch]::StartNew()
                    $deadline = [Threading.CancellationTokenSource]::new([TimeSpan]::FromSeconds(190))
                    try {
                        $response = $client.SendAsync($request, [Net.Http.HttpCompletionOption]::ResponseHeadersRead, $deadline.Token).GetAwaiter().GetResult()
                        if (-not $response.IsSuccessStatusCode) { throw 'Native evaluation inference failed.' }
                        $expectedMedia = if ($ResponseFormat -ceq 'pcm') { 'audio/pcm' } else { 'audio/wav' }
                        if ($response.Content.Headers.ContentType.MediaType -cne $expectedMedia) { throw 'Unexpected evaluation audio media type.' }
                        $audio = $response.Content.ReadAsStreamAsync($deadline.Token).GetAwaiter().GetResult()
                        $buffer = [byte[]]::new(16384)
                        $firstAudio = $null
                        try {
                            while (($count = $audio.ReadAsync($buffer, 0, $buffer.Length, $deadline.Token).GetAwaiter().GetResult()) -gt 0) {
                                if ($null -eq $firstAudio) { $firstAudio = $clock.Elapsed.TotalMilliseconds }
                                if ($buffered.Length + $count -gt 5760044) { throw 'Evaluation audio exceeded its duration bound.' }
                                $buffered.Write($buffer, 0, $count)
                            }
                        } finally { $audio.Dispose() }
                        $clock.Stop()
                        $bytes = $buffered.ToArray()
                        if ($ResponseFormat -ceq 'pcm') { $bytes = ConvertTo-EvaluationWave -Pcm $bytes }
                        if ($bytes.Length -lt 46 -or [Text.Encoding]::ASCII.GetString($bytes, 0, 4) -cne 'RIFF' -or
                            [BitConverter]::ToUInt32($bytes, 4) -ne $bytes.Length - 8 -or
                            [Text.Encoding]::ASCII.GetString($bytes, 8, 8) -cne 'WAVEfmt ' -or
                            [BitConverter]::ToUInt32($bytes, 16) -ne 16 -or [BitConverter]::ToUInt16($bytes, 20) -ne 1 -or
                            [BitConverter]::ToUInt16($bytes, 22) -ne 1 -or [BitConverter]::ToUInt32($bytes, 24) -ne 24000 -or
                            [BitConverter]::ToUInt32($bytes, 28) -ne 48000 -or [BitConverter]::ToUInt16($bytes, 32) -ne 2 -or
                            [BitConverter]::ToUInt16($bytes, 34) -ne 16 -or [Text.Encoding]::ASCII.GetString($bytes, 36, 4) -cne 'data' -or
                            [BitConverter]::ToUInt32($bytes, 40) -ne $bytes.Length - 44 -or ($bytes.Length - 44) % 2 -ne 0) {
                            throw 'Evaluation output was not a complete mono 24 kHz PCM16 WAV.'
                        }
                        $seconds = ($bytes.Length - 44) / 48000.0
                        $clip = $quantization + '-' + $threadCount + '-' + $iteration + '.wav'
                        [IO.File]::WriteAllBytes((Join-Path $lane $clip), $bytes)
                        $report.Records.Add([ordered]@{
                            Quantization = $quantization; Threads = $threadCount; Iteration = $iteration
                            ColdReadyMs = $cold.Elapsed.TotalMilliseconds; FirstBodyMs = $firstAudio
                            InferenceMs = $clock.Elapsed.TotalMilliseconds; AudioSeconds = $seconds
                            RealTimeFactor = $clock.Elapsed.TotalSeconds / $seconds; CompletePcm16 = $true
                            Clip = (Split-Path $lane -Leaf) + '/' + $clip
                        })
                        $report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $reportPath
                    } finally {
                        $clock.Stop(); $buffered.Dispose(); $request.Dispose()
                        $deadline.Dispose()
                        if ($response) { $response.Dispose() }
                    }
                }
            } finally {
                if ($started -and -not $process.HasExited) { $process.Kill($true); $process.WaitForExit() }
                $readyDeadline.Dispose()
                $process.Dispose()
            }
        }
    }
    $completed = $true
} finally {
    $client.Dispose()
    $report.Status = if ($completed) { 'complete' } else { 'incomplete' }
    $report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $reportPath
}
[pscustomobject]@{ ReportPath = $reportPath; QualityVerdict = $report.QualityVerdict; Records = $report.Records.Count }
