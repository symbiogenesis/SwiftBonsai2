#requires -Version 7.6
<#
.SYNOPSIS
Runs one focused synthetic Windows process ownership or resource-limit check.
.DESCRIPTION
Builds a small .NET probe in a unique external temp lane. No model, inference,
benchmark, full test suite, filesystem isolation or egress isolation is involved.
#>
[CmdletBinding()]
[OutputType([string])]
param([Parameter(Mandatory)][ValidateSet('Lifetime', 'Limits', 'LaunchContract')][string]$Case)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if (-not $IsWindows) { throw 'Windows Job lifecycle checks require Windows 10 or later.' }
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$helper = Join-Path $repo 'scripts/Bonsai.Process.cs'
if (-not ('SwiftBonsai.OwnedProcess' -as [type])) {
    Add-Type -Path $helper -CompilerOptions @('/nullable:enable', '/checked+', '/unsafe-', '/warn:9999', '/warnaserror+', '/langversion:14.0')
}
$scratch = [IO.Path]::GetFullPath((Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-ProcessTest-' + [guid]::NewGuid().ToString('N'))))
if (-not [IO.Path]::IsPathFullyQualified($scratch) -or $scratch -match '%[^%]+%') { throw 'The native test lane must be expanded and absolute.' }
[void][IO.Directory]::CreateDirectory($scratch)
[IO.File]::Copy((Join-Path $repo 'global.json'), (Join-Path $scratch 'global.json'))
$probeSource = [Security.SecurityElement]::Escape((Join-Path $PSScriptRoot 'ProcessLifecycleProbe.cs'))
$helperSource = [Security.SecurityElement]::Escape($helper)
$project = Join-Path $scratch 'ProcessProbe.csproj'
@"
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework>
    <LangVersion>14.0</LangVersion><Nullable>enable</Nullable>
    <AllowUnsafeBlocks>false</AllowUnsafeBlocks><CheckForOverflowUnderflow>true</CheckForOverflowUnderflow>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors><EnableDefaultCompileItems>false</EnableDefaultCompileItems>
  </PropertyGroup>
  <ItemGroup><Compile Include="$helperSource" /><Compile Include="$probeSource" /></ItemGroup>
</Project>
"@ | Set-Content -LiteralPath $project
$build = [Diagnostics.ProcessStartInfo]::new((Get-Command -Name dotnet -CommandType Application).Source)
$build.WorkingDirectory = $scratch; $build.UseShellExecute = $false; $build.CreateNoWindow = $true
$build.RedirectStandardOutput = $true; $build.RedirectStandardError = $true
foreach ($argument in @('build', $project, '--configuration', 'Release', '--output', (Join-Path $scratch 'probe'), '--nologo')) { $build.ArgumentList.Add($argument) }
$builder = [Diagnostics.Process]::new()
$builder.StartInfo = $build
try {
    if (-not $builder.Start()) { throw 'Synthetic lifecycle probe build did not start.' }
    $stdout = $builder.StandardOutput.ReadToEndAsync(); $stderr = $builder.StandardError.ReadToEndAsync()
    if (-not $builder.WaitForExit(60000)) { $builder.Kill($true); throw 'Synthetic lifecycle probe build timed out.' }
    $output = $stdout.GetAwaiter().GetResult() + $stderr.GetAwaiter().GetResult()
    [IO.File]::WriteAllText((Join-Path $scratch 'build.log'), $output)
    if ($builder.ExitCode -ne 0) { throw "Synthetic lifecycle probe build failed. Evidence: $scratch`n$output" }
} finally { $builder.Dispose() }
$executable = Join-Path $scratch 'probe/ProcessProbe.exe'

function Assert {
    [CmdletBinding()]
    [OutputType([void])]
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw $Message }
}
function Read-SharedText {
    [CmdletBinding()]
    [OutputType([string])]
    param([string]$Path)
    # Native process teardown can still hold its standard file handle briefly
    # after the Job process count reaches zero. Share that final handle safely.
    $file = [IO.FileStream]::new($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite)
    $reader = [IO.StreamReader]::new($file)
    try { return $reader.ReadToEnd() } finally { $reader.Dispose(); $file.Dispose() }
}
function New-Probe {
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'The focused test must launch only its own disposable synthetic process to exercise native lifecycle enforcement.')]
    [CmdletBinding()]
    [OutputType([SwiftBonsai.OwnedProcess])]
    param([string[]]$Arguments, [string]$Name, [ulong]$Memory = 512MB, [uint]$Processes = 2)
    return [SwiftBonsai.OwnedProcess]::Start($executable, $Arguments, $scratch,
        (Join-Path $scratch "$Name.stdout"), (Join-Path $scratch "$Name.stderr"), $Memory, $Processes)
}
function Wait-Marker {
    [CmdletBinding()]
    [OutputType([string])]
    param([string]$Path)
    $deadline = [Environment]::TickCount64 + 10000
    while ($true) {
        if ([IO.File]::Exists($Path)) {
            try {
                # The marker becomes visible before WriteAllText closes its
                # writer. Wait for that complete publication, then use this read.
                $text = [IO.File]::ReadAllText($Path)
                if ($text.Length -gt 0) { return $text }
            } catch [IO.IOException] {
                # Retry only Windows sharing/lock conflicts while the writer is
                # publishing. Missing, corrupt or otherwise unreadable data fails.
                if (($_.Exception.HResult -band 0xffff) -notin @(32, 33)) { throw }
            }
        }
        if ([Environment]::TickCount64 -ge $deadline) { throw "Synthetic process did not create marker: $Path" }
        Start-Sleep -Milliseconds 20
    }
}
function Get-TreeProcess {
    [CmdletBinding()]
    [OutputType([Diagnostics.Process])]
    param([string]$Marker)
    $processIds = Wait-Marker $Marker
    $null = Wait-Marker ($Marker + '.child')
    foreach ($processId in $processIds.Split(',')) { [Diagnostics.Process]::GetProcessById([int]$processId) }
}
function Assert-TreeExited {
    [CmdletBinding()]
    [OutputType([void])]
    param([Diagnostics.Process[]]$Processes)
    foreach ($member in $Processes) {
        try { Assert ($member.WaitForExit(10000)) "Owned process $($member.Id) survived Job closure." }
        finally { $member.Dispose() }
    }
}

switch ($Case) {
    'Lifetime' {
        foreach ($method in @('Stop', 'Dispose')) {
            $marker = Join-Path $scratch "tree-$method"
            $owned = New-Probe @('tree', $marker) "tree-$method"
            try {
                $members = @(Get-TreeProcess $marker)
                if ($method -eq 'Stop') {
                    $owned.Stop(10000)
                    Assert ($owned.Process.WaitForExit(0)) 'Stop returned before the root process became signaled.'
                } else { $owned.Dispose() }
                Assert-TreeExited $members
            } finally { $owned.Dispose() }
        }
        $owned = New-Probe @('callback-lifetime', (Join-Path $scratch 'callback-lifetime')) 'callback-lifetime'
        try {
            Assert ($owned.Process.WaitForExit(15000)) 'The concurrent exit callback and owner disposal deadlocked.'
            Assert ($owned.Process.ExitCode -eq 0) 'The exit callback could not reenter a concurrently disposing owner.'
            $owned.Stop(10000)
        } finally { $owned.Dispose() }
        # Kill just the owner, without invoking its finally/Dispose methods.
        # The kernel must close the owner's noninheritable Job handle itself.
        $marker = Join-Path $scratch 'abrupt-owner'
        $start = [Diagnostics.ProcessStartInfo]::new($executable)
        $start.WorkingDirectory = $scratch; $start.UseShellExecute = $false; $start.CreateNoWindow = $true
        $start.ArgumentList.Add('owner'); $start.ArgumentList.Add($marker)
        $owner = [Diagnostics.Process]::Start($start)
        try {
            $members = @(Get-TreeProcess $marker)
            $owner.Kill(); Assert ($owner.WaitForExit(10000)) 'Synthetic owner did not exit.'
            Assert-TreeExited $members
        } finally {
            if (-not $owner.HasExited) { $owner.Kill($true); $owner.WaitForExit() }
            $owner.Dispose()
        }
    }
    'Limits' {
        $owned = New-Probe @('memory') 'memory' -Memory 256MB -Processes 1
        try {
            Assert ($owned.Process.WaitForExit(10000)) 'Synthetic commit request timed out.'
            Assert ($owned.Process.ExitCode -eq 0) 'The job failed to deny an over-limit native commit, or rejected a small commit.'
            Assert ((Read-SharedText (Join-Path $scratch 'memory.stdout')) -ceq 'commit-denied') 'Commit-denial evidence is missing.'
            # Windows may include the rejected request in this diagnostic peak.
            # The proof of enforcement is that VirtualAlloc denied that request.
            Assert ($owned.GetPeakJobMemoryBytes() -ge 16MB) 'Windows did not report the successful native commit.'
            $owned.Stop(10000)
        } finally { $owned.Dispose() }
        # Each 300 MiB request fits the 512 MiB Job cap by itself, while the
        # combined parent/child committed charge must be denied.
        $owned = New-Probe @('aggregate', (Join-Path $scratch 'aggregate-child')) 'aggregate-memory'
        try {
            Assert ($owned.Process.WaitForExit(15000)) 'Synthetic aggregate commit request timed out.'
            Assert ($owned.Process.ExitCode -eq 0) 'The job failed to enforce committed memory across descendants.'
            $owned.Stop(10000)
            Assert ((Read-SharedText (Join-Path $scratch 'aggregate-memory.stdout')) -ceq 'aggregate-commit-denied') 'Aggregate commit-denial evidence is missing.'
        } finally { $owned.Dispose() }
        $marker = Join-Path $scratch 'forbidden-child'
        $owned = New-Probe @('spawn', $marker) 'process-limit' -Processes 1
        try {
            Assert ($owned.Process.WaitForExit(10000)) 'Synthetic child creation attempt timed out.'
            Assert ($owned.Process.ExitCode -eq 0) 'The process-count probe failed.'
            Assert (-not [IO.File]::Exists($marker)) 'A child executed despite the one-process Job cap.'
            $owned.Stop(10000)
        } finally { $owned.Dispose() }
    }
    'LaunchContract' {
        $marker = Join-Path $scratch 'stream-sharing'
        $owned = New-Probe @('wait', $marker) 'stream-sharing' -Processes 1
        try {
            $null = Wait-Marker $marker
            foreach ($stream in @('stdout', 'stderr')) {
                $path = Join-Path $scratch "stream-sharing.$stream"
                $null = Read-SharedText $path
                $writeDenied = $false
                try {
                    $writer = [IO.FileStream]::new($path, [IO.FileMode]::Open, [IO.FileAccess]::Write, [IO.FileShare]::ReadWrite)
                    $writer.Dispose()
                } catch [IO.IOException] {
                    if (($_.Exception.HResult -band 0xffff) -ne 32) { throw }
                    $writeDenied = $true
                }
                Assert $writeDenied "Another writer could overwrite active $stream evidence."
            }
            $owned.Stop(10000)
        } finally { $owned.Dispose() }
        $marker = Join-Path $scratch 'error-policy'
        $start = [Diagnostics.ProcessStartInfo]::new($executable)
        $start.WorkingDirectory = $scratch; $start.UseShellExecute = $false; $start.CreateNoWindow = $true
        $start.ArgumentList.Add('error-policy-owner'); $start.ArgumentList.Add($marker)
        $owner = [Diagnostics.Process]::Start($start)
        try {
            Assert ($owner.WaitForExit(30000)) 'Synthetic error-mode owner did not exit.'
            Assert ($owner.ExitCode -eq 0) 'Synthetic error-mode owner failed.'
            Assert ((Wait-Marker ($marker + '.before')) -ceq '0') 'Synthetic owner error mode did not start at the default.'
            Assert ((Wait-Marker ($marker + '.after')) -ceq '0') 'Creating owned processes changed the owner error mode.'
            Assert (([int](Wait-Marker ($marker + '.root'))) -ne 0) 'The root native exception lost its failure status.'
            Assert (([int](Wait-Marker ($marker + '.child'))) -ne 0) 'The descendant native exception lost its failure status.'
        } finally {
            if (-not $owner.HasExited) { $owner.Kill($true); $owner.WaitForExit() }
            $owner.Dispose()
        }
        $values = @('', 'space and & shell | symbols', 'a"b', 'C:\trailing slash\', 'slash\\"quote', "unicode-$([char]0x03bb)")
        $owned = New-Probe (@('echo') + $values) 'arguments' -Processes 1
        try {
            Assert ($owned.Process.WaitForExit(10000)) 'Synthetic argument probe timed out.'
            Assert ($owned.Process.ExitCode -eq 0) 'Synthetic argument probe failed.'
            $actual = Read-SharedText (Join-Path $scratch 'arguments.stdout') | ConvertFrom-Json -DateKind String
            Assert ($actual.Arguments.Count -eq $values.Count) 'The native argument count changed.'
            for ($index = 0; $index -lt $values.Count; $index++) { Assert ($actual.Arguments[$index] -ceq $values[$index]) "Native argument $index changed." }
            Assert ($actual.WorkingDirectory -ceq $scratch) 'The native current directory was not the explicit external temp path.'
            Assert ($actual.SystemRoot -ceq $env:SystemRoot -and $actual.UserProfile -ceq $env:USERPROFILE) 'An inherited OS/profile variable changed.'
            Assert ($actual.InputClosed) 'Synthetic standard input was not closed.'
            Assert ((Read-SharedText (Join-Path $scratch 'arguments.stderr')) -ceq 'stderr-evidence') 'Standard error evidence was not preserved.'
            $owned.Stop(10000)
        } finally { $owned.Dispose() }
    }
}
Write-Output "Focused Windows Job $Case checks passed. Evidence: $scratch"
