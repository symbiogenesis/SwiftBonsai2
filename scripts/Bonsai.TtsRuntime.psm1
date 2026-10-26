#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')

function Get-QwenTtsRuntimePlan {
    <# .SYNOPSIS
    Plans the pinned private Windows x64 Qwen runtime without installing anything.
    #>
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([Parameter(Mandatory)][string]$DataDirectory)
    $data = Resolve-ExternalPath $DataDirectory
    $lockPath = Join-Path $PSScriptRoot '../tools/qwen-tts/runtime.lock.json'
    $bytes = [IO.File]::ReadAllBytes($lockPath)
    $identity = [Convert]::ToHexStringLower([Security.Cryptography.SHA256]::HashData($bytes))
    $pin = [Text.Encoding]::UTF8.GetString($bytes) | ConvertFrom-Json -AsHashtable -DateKind String
    if ($pin.schemaVersion -ne 1 -or $pin.pythonVersion -notmatch '^3\.13\.\d+$' -or
        $pin.pythonPth -cne 'python313._pth' -or $pin.pythonZip -cne 'python313.zip' -or
        $pin.artifacts -isnot [array] -or $pin.artifacts.Count -lt 5) {
        throw 'Unsupported Qwen TTS runtime lock.'
    }
    $directory = Resolve-ExternalPath (Join-Path $data "speech/runtime/qwen-tts/$identity")
    $cache = Resolve-ExternalPath (Join-Path $data 'speech/runtime/downloads')
    $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    $artifacts = foreach ($artifact in $pin.artifacts) {
        $uri = [uri]$artifact.url
        if ($artifact.name -notmatch '^[A-Za-z0-9][A-Za-z0-9_.+-]+\.(whl|zip|tar\.gz)$' -or
            -not $names.Add($artifact.name) -or $artifact.sha256 -cnotmatch '^[a-f0-9]{64}$' -or
            ($artifact.bytes -isnot [int] -and $artifact.bytes -isnot [long]) -or $artifact.bytes -le 0 -or
            $artifact.kind -cnotin @('Python','Pip','Bootstrap','Package') -or
            -not $uri.IsAbsoluteUri -or $uri.Scheme -cne 'https' -or $uri.UserInfo -or $uri.Query -or $uri.Fragment -or
            $uri.Host -cnotin @('www.python.org','files.pythonhosted.org','download.pytorch.org','download-r2.pytorch.org') -or
            [uri]::UnescapeDataString($uri.Segments[-1]) -cne $artifact.name) {
            throw 'Runtime artifacts require unique filenames, immutable HTTPS locations, exact lengths and SHA-256.'
        }
        [pscustomobject]@{ Name = $artifact.name; Path = (Join-Path $cache $artifact.name)
            Uri = $uri; Bytes = [long]$artifact.bytes; Sha256 = $artifact.sha256; Kind = $artifact.kind }
    }
    if (@($artifacts | Where-Object Kind -CEQ Python).Count -ne 1 -or
        @($artifacts | Where-Object Kind -CEQ Pip).Count -ne 1 -or
        ($artifacts | Where-Object Kind -CEQ Python).Name -cne "python-$($pin.pythonVersion)-embed-amd64.zip" -or
        @($artifacts | Where-Object { $_.Kind -cne 'Python' -and $_.Name -notmatch '\.(whl|tar\.gz)$' }).Count) {
        throw 'Runtime lock must select one official x64 CPython embed archive and one pip wheel.'
    }
    return [pscustomobject]@{ RuntimeDirectory = $directory; PythonPath = (Join-Path $directory 'python.exe')
        Artifacts = @($artifacts); Identity = $identity; PythonPth = $pin.pythonPth; PythonZip = $pin.pythonZip }
}

function Test-QwenTtsRuntime {
    <# .SYNOPSIS
    Imports the real inference dependencies without loading weights or opening audio.
    #>
    [CmdletBinding()]
    [OutputType([void])]
    param([Parameter(Mandatory)][string]$PythonPath, [Parameter(Mandatory)][string]$WorkingDirectory)
    $probe = 'import sys,sysconfig; from importlib.metadata import version; assert sys.version_info[:2] == (3,13); assert sysconfig.get_platform() == "win-amd64"; import torch,torchaudio,transformers,accelerate; from qwen_tts import Qwen3TTSModel; assert torch.__version__ == "2.9.1+cpu"; assert torchaudio.__version__ == "2.9.1+cpu"; assert transformers.__version__ == "4.57.3"; assert accelerate.__version__ == "1.12.0"; assert version("qwen-tts") == "0.1.1"'
    $log = Join-Path $WorkingDirectory ('tts-import-' + [guid]::NewGuid().ToString('N') + '.log')
    $result = Invoke-BonsaiNativeProcess -Executable $PythonPath -ArgumentList @('-I','-B','-c',$probe) -WorkingDirectory $WorkingDirectory -TimeoutSeconds 120 -LogPath $log
    if ($result.ExitCode -ne 0) { throw "The pinned Qwen TTS runtime could not import its dependencies. See $log" }
}

function Install-QwenTtsRuntime {
    <# .SYNOPSIS
    Downloads and installs the pinned Qwen runtime privately, reusing a verified installation.
    .DESCRIPTION
    Uses an isolated embeddable x64 CPython on Windows ARM64 through Windows
    emulation because upstream torchaudio has no native Windows ARM64 wheel.
    Does not change PATH, Python associations, system packages or profile variables.
    #>
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Explicit demo runtime provisioning; no global Python installation or existing runtime is changed.')]
    [CmdletBinding()]
    [OutputType([string])]
    param([Parameter(Mandatory)][string]$DataDirectory, [Parameter(Mandatory)][string]$WorkingDirectory)
    if (-not $IsWindows) { throw 'The demo Qwen TTS runtime requires Windows x64 emulation or Windows x64.' }
    $working = Resolve-ExternalPath $WorkingDirectory
    if (-not [IO.Directory]::Exists($working)) { throw 'An existing external working directory is required.' }
    $plan = Get-QwenTtsRuntimePlan -DataDirectory $DataDirectory
    $marker = Join-Path $plan.RuntimeDirectory 'runtime.receipt'
    if ([IO.File]::Exists($marker)) {
        if ([IO.File]::ReadAllText($marker) -cne $plan.Identity) { throw 'Qwen runtime receipt does not match its pinned dependencies.' }
        Test-QwenTtsRuntime -PythonPath $plan.PythonPath -WorkingDirectory $working
        return $plan.PythonPath
    }
    $parent = [IO.Path]::GetDirectoryName($plan.RuntimeDirectory)
    [void][IO.Directory]::CreateDirectory($parent)
    $lease = [IO.FileStream]::new(($plan.RuntimeDirectory + '.setup-lock'), [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    try {
        if ([IO.Directory]::Exists($plan.RuntimeDirectory)) {
            throw "An incomplete Qwen runtime already exists; inspect it before retrying: $($plan.RuntimeDirectory)"
        }
        foreach ($artifact in $plan.Artifacts) {
            Write-Information -MessageData ('Preparing Qwen runtime dependency: {0}' -f $artifact.Name) -InformationAction Continue
            [void](Receive-LockedArtifact -Uri $artifact.Uri -Path $artifact.Path -Bytes $artifact.Bytes -Sha256 $artifact.Sha256)
        }
        # A short sibling avoids exceeding Windows executable-path limits when
        # the caller uses a longer external data directory.
        $staging = Resolve-ExternalPath (Join-Path $parent ('setup-' + [guid]::NewGuid().ToString('N')))
        if ((Join-Path $staging 'python.exe').Length -ge 260 -or $plan.PythonPath.Length -ge 260) {
            throw 'Choose a shorter DataDirectory for the private Qwen Python runtime.'
        }
        [void][IO.Directory]::CreateDirectory($staging)
        $python = ($plan.Artifacts | Where-Object Kind -CEQ Python).Path
        [IO.Compression.ZipFile]::ExtractToDirectory($python, $staging)
        $site = Join-Path $staging 'Lib/site-packages'
        [void][IO.Directory]::CreateDirectory($site)
        $pip = ($plan.Artifacts | Where-Object Kind -CEQ Pip).Path
        [IO.Compression.ZipFile]::ExtractToDirectory($pip, $site)
        [IO.File]::WriteAllText((Join-Path $staging $plan.PythonPth), "$($plan.PythonZip)`n.`nLib/site-packages`nimport site`n")
        $stagedPython = Join-Path $staging 'python.exe'
        foreach ($kind in @('Bootstrap','Package')) {
            $paths = @($plan.Artifacts | Where-Object Kind -CEQ $kind | ForEach-Object Path)
            if ($paths.Count -eq 0) { continue }
            $arguments = @('-I','-B','-m','pip','--isolated','install','--disable-pip-version-check','--no-input','--no-cache-dir',
                '--no-index','--no-deps','--no-build-isolation','--no-warn-script-location') + $paths
            $log = Join-Path $working ("tts-install-$kind-" + [guid]::NewGuid().ToString('N') + '.log')
            $result = Invoke-BonsaiNativeProcess -Executable $stagedPython -ArgumentList $arguments -WorkingDirectory $working -TimeoutSeconds 1800 -LogPath $log
            if ($result.ExitCode -ne 0) { throw "Qwen TTS dependency installation failed. See $log. Staging retained at $staging" }
        }
        Test-QwenTtsRuntime -PythonPath $stagedPython -WorkingDirectory $working
        [IO.File]::WriteAllText((Join-Path $staging 'runtime.receipt'), $plan.Identity)
        # Windows may retain executable/DLL mappings briefly after the import
        # probe exits. Publish only this verified sibling, without replacement.
        for ($attempt = 0; ; $attempt++) {
            try {
                [IO.Directory]::Move($staging, $plan.RuntimeDirectory)
                break
            } catch [IO.IOException], [UnauthorizedAccessException] {
                if ($attempt -ge 119 -or [IO.Directory]::Exists($plan.RuntimeDirectory)) { throw }
                Start-Sleep -Milliseconds 250
            }
        }
        return $plan.PythonPath
    } finally { $lease.Dispose() }
}

Export-ModuleMember -Function Get-QwenTtsRuntimePlan, Install-QwenTtsRuntime
