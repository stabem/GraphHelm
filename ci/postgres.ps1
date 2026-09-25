<#
.SYNOPSIS
    Runs the GraphHelm ignored PostgreSQL test gates against a throwaway, isolated cluster.

.DESCRIPTION
    Discovers the PostgreSQL binaries already installed on the host (GitHub Actions runners ship
    PostgreSQL on both windows-latest and ubuntu-latest), initialises a brand new cluster in a
    temporary directory on a random loopback-only TCP port, creates a random administrative role
    and database, exports GRAPHHELM_TEST_ADMIN_URL / GRAPHHELM_TEST_PG_DUMP / GRAPHHELM_TEST_PG_RESTORE
    and runs the ignored tests serially. Normal unwinding stops and removes the cluster. The early
    Windows gate additionally contains detached server processes in its parent-owned job; forced
    termination can leave temporary files. No Docker or service container is used.

    Compatible with Windows PowerShell 5.1 and PowerShell 7+ on Windows and Linux.

.PARAMETER TestCommand
    Executable used to run the gated tests. Defaults to "cargo".

.PARAMETER TestArgs
    Exact argument vector handed to TestCommand. Defaults to the full serial ignored-test run.
    Override it to run a bounded subset, for example:
        ./ci/postgres.ps1 -TestArgs @('+1.97.1','test','-p','graphhelm-postgres-event-store',
            '--test','migration','--all-features','--locked','--','--ignored','--test-threads=1')

.PARAMETER PostgresBin
    Directory containing initdb/pg_ctl/postgres/pg_dump/pg_restore/psql. Defaults to
    $env:GRAPHHELM_PG_BIN, then $env:PGBIN, then the platform's standard install locations.
#>
[CmdletBinding()]
param(
    [string] $TestCommand = 'cargo',

    [string[]] $TestArgs = @(
        '+1.97.1',
        'test',
        '--workspace',
        '--all-features',
        '--locked',
        '--',
        '--ignored',
        '--test-threads=1'
    ),

    [string] $PostgresBin,
    [switch] $InitializeJobSupportOnly
)

# The early gate owns this job; the child only opens it long enough to join. Assignment happens
# before any PostgreSQL executable starts. Detached pg_ctl descendants cannot escape the job.
if ($InitializeJobSupportOnly) {
    if (-not ('GraphHelmPostgresJob' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Threading;
public sealed class GraphHelmPostgresJob : IDisposable {
    [StructLayout(LayoutKind.Sequential)] struct BasicLimit {
        public long ProcessTime, JobTime; public uint Flags; public UIntPtr Min, Max;
        public uint ActiveLimit; public UIntPtr Affinity; public uint Priority, Scheduling;
    }
    [StructLayout(LayoutKind.Sequential)] struct IoCounters { public ulong A,B,C,D,E,F; }
    [StructLayout(LayoutKind.Sequential)] struct ExtendedLimit {
        public BasicLimit Basic; public IoCounters Io; public UIntPtr ProcessMemory, JobMemory, PeakProcess, PeakJob;
    }
    [StructLayout(LayoutKind.Sequential)] struct Accounting {
        public long A,B,C,D; public uint Faults, Total, Active, Terminated;
    }
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr CreateJobObject(IntPtr attributes, string name);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr OpenJobObject(uint access, bool inherit, string name);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool SetInformationJobObject(IntPtr job, int info, ref ExtendedLimit value, uint size);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool QueryInformationJobObject(IntPtr job, int info, out Accounting value, uint size, IntPtr returned);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool TerminateJobObject(IntPtr job, uint code);
    [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    IntPtr handle;
    public string Name { get; private set; }
    public GraphHelmPostgresJob() {
        Name = "Local\\GraphHelmPostgres-" + Guid.NewGuid().ToString("N");
        handle = CreateJobObject(IntPtr.Zero, Name);
        if (handle == IntPtr.Zero) throw new Win32Exception();
        var limits = new ExtendedLimit(); limits.Basic.Flags = 0x2000; // KILL_ON_JOB_CLOSE; no breakaway.
        if (!SetInformationJobObject(handle, 9, ref limits, (uint)Marshal.SizeOf(limits))) {
            var error = new Win32Exception(); Dispose(); throw error;
        }
    }
    public static void Join(string name) {
        IntPtr job = OpenJobObject(1, false, name); // ASSIGN_PROCESS only, non-inheritable.
        if (job == IntPtr.Zero) throw new Win32Exception();
        try { if (!AssignProcessToJobObject(job, GetCurrentProcess())) throw new Win32Exception(); }
        finally { CloseHandle(job); }
    }
    public uint ActiveProcesses {
        get {
            if (handle == IntPtr.Zero) throw new ObjectDisposedException("GraphHelmPostgresJob");
            Accounting value;
            if (!QueryInformationJobObject(handle, 1, out value, (uint)Marshal.SizeOf(typeof(Accounting)), IntPtr.Zero)) throw new Win32Exception();
            return value.Active;
        }
    }
    // The deadline bounds polling, not kernel-call latency. The caller first observes wrapper exit,
    // so it cannot join the job after the final empty observation. No child retains an assignment handle.
    public void StopAndDrain(int milliseconds) {
        if (!TerminateJobObject(handle, 1)) throw new Win32Exception();
        var watch = Stopwatch.StartNew();
        while (ActiveProcesses != 0) {
            if (watch.ElapsedMilliseconds >= milliseconds) throw new TimeoutException("PostgreSQL job did not drain; slot must not be released.");
            Thread.Sleep(10);
        }
    }
    public void Dispose() { if (handle != IntPtr.Zero) { CloseHandle(handle); handle = IntPtr.Zero; } GC.SuppressFinalize(this); }
    ~GraphHelmPostgresJob() { Dispose(); }
}
'@
    }
    return
}

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

$onWindows = $true
if (Get-Variable -Name 'IsWindows' -ErrorAction SilentlyContinue) {
    $onWindows = [bool] (Get-Variable -Name 'IsWindows' -ValueOnly)
}
$exeSuffix = ''
if ($onWindows) {
    $exeSuffix = '.exe'
}

function Write-Step {
    param([string] $Message)
    Write-Host "[ci/postgres] $Message"
}

# #909: the executed-test count. Dot-sourced, not reimplemented -- ci/test-count.ps1 defines two pure
# functions and does nothing else, and its own suite is what proves the parse.
. (Join-Path $PSScriptRoot 'test-count.ps1')

function Invoke-Tool {
    param(
        [string] $Path,
        [string[]] $Arguments,
        [string] $What
    )
    & $Path @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$What failed with exit code $LASTEXITCODE"
    }
}

function Get-CandidateBinDirectories {
    $candidates = New-Object System.Collections.Generic.List[string]
    if ($PostgresBin) {
        $candidates.Add($PostgresBin)
    }
    if ($env:GRAPHHELM_PG_BIN) {
        $candidates.Add($env:GRAPHHELM_PG_BIN)
    }
    if ($env:PGBIN) {
        $candidates.Add($env:PGBIN)
    }

    $roots = @()
    if ($onWindows) {
        $roots = @(
            'C:\Program Files\PostgreSQL',
            'C:\Program Files (x86)\PostgreSQL'
        )
    } else {
        $roots = @(
            '/usr/lib/postgresql',
            '/usr/local/pgsql',
            '/opt/homebrew/opt'
        )
    }
    foreach ($root in $roots) {
        if (Test-Path -LiteralPath $root) {
            $versions = Get-ChildItem -LiteralPath $root -Directory -ErrorAction SilentlyContinue |
                Sort-Object -Property Name -Descending
            foreach ($version in $versions) {
                $candidates.Add((Join-Path $version.FullName 'bin'))
            }
        }
    }

    $onPath = Get-Command -Name "initdb$exeSuffix" -ErrorAction SilentlyContinue
    if ($onPath) {
        $candidates.Add((Split-Path -Parent $onPath.Source))
    }

    return $candidates
}

function Resolve-PostgresBin {
    $required = @('initdb', 'pg_ctl', 'postgres', 'pg_dump', 'pg_restore', 'psql')
    $inspected = New-Object System.Collections.Generic.List[string]
    foreach ($directory in (Get-CandidateBinDirectories)) {
        if (-not $directory) {
            continue
        }
        if ($inspected.Contains($directory)) {
            continue
        }
        $inspected.Add($directory)
        if (-not (Test-Path -LiteralPath $directory)) {
            continue
        }
        $complete = $true
        foreach ($tool in $required) {
            if (-not (Test-Path -LiteralPath (Join-Path $directory "$tool$exeSuffix"))) {
                $complete = $false
                break
            }
        }
        if ($complete) {
            return (Resolve-Path -LiteralPath $directory).ProviderPath
        }
    }

    $searched = ''
    if ($inspected.Count -gt 0) {
        $searched = [string]::Join([Environment]::NewLine + '  - ', $inspected.ToArray())
    }
    throw ("Could not find a PostgreSQL installation containing " +
        [string]::Join(', ', $required) +
        ". Set GRAPHHELM_PG_BIN (or pass -PostgresBin) to the bin directory of a PostgreSQL " +
        "distribution. Searched:" + [Environment]::NewLine + '  - ' + $searched)
}

function Get-RandomToken {
    param([int] $Bytes = 18)
    $buffer = New-Object 'byte[]' $Bytes
    $rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
    try {
        $rng.GetBytes($buffer)
    } finally {
        $rng.Dispose()
    }
    $alphabet = 'abcdefghijklmnopqrstuvwxyz0123456789'
    $builder = New-Object System.Text.StringBuilder
    foreach ($byte in $buffer) {
        [void] $builder.Append($alphabet[$byte % $alphabet.Length])
    }
    return $builder.ToString()
}

function Get-RandomFreePort {
    for ($attempt = 0; $attempt -lt 128; $attempt++) {
        $candidate = Get-Random -Minimum 20000 -Maximum 60000
        $listener = $null
        try {
            $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $candidate)
            $listener.Start()
            $listener.Stop()
            return $candidate
        } catch {
            if ($listener) {
                try { $listener.Stop() } catch { }
            }
        }
    }
    throw 'Unable to find a free loopback TCP port for the throwaway PostgreSQL cluster.'
}

function Remove-TreeWithRetry {
    param([string] $Path)
    for ($attempt = 0; $attempt -lt 10; $attempt++) {
        if (-not (Test-Path -LiteralPath $Path)) {
            return
        }
        try {
            Remove-Item -LiteralPath $Path -Recurse -Force -ErrorAction Stop
            return
        } catch {
            Start-Sleep -Milliseconds 300
        }
    }
    if (Test-Path -LiteralPath $Path) {
        Write-Warning "[ci/postgres] Could not remove temporary directory '$Path'."
    }
}

$binDirectory = Resolve-PostgresBin
$initdb = Join-Path $binDirectory "initdb$exeSuffix"
$pgCtl = Join-Path $binDirectory "pg_ctl$exeSuffix"
$psql = Join-Path $binDirectory "psql$exeSuffix"
$pgIsReady = Join-Path $binDirectory "pg_isready$exeSuffix"
$pgDump = Join-Path $binDirectory "pg_dump$exeSuffix"
$pgRestore = Join-Path $binDirectory "pg_restore$exeSuffix"
Write-Step "Using PostgreSQL binaries from '$binDirectory'."

$token = Get-RandomToken
$bootstrapRole = "pgroot_$token"
# The administrative role must be named 'postgres'. The hostile-superuser recovery test asserts the
# conventional superuser owns a database created through the admin connection, which is what every
# stock installation provides. Isolation comes from the private data directory and the random port,
# not from randomising the superuser name.
$adminRole = 'postgres'
$databaseName = "graphhelm_ci_$token"
$bootstrapPassword = Get-RandomToken -Bytes 24
$adminPassword = Get-RandomToken -Bytes 24
$port = Get-RandomFreePort

$temporaryRoot = Join-Path ([System.IO.Path]::GetTempPath()) "graphhelm-pg-$token"
$dataDirectory = Join-Path $temporaryRoot 'data'
$socketDirectory = Join-Path $temporaryRoot 'sock'
$logFile = Join-Path $temporaryRoot 'postgres.log'
$passwordFile = Join-Path $temporaryRoot 'initdb.pwfile'
$bootstrapSql = Join-Path $temporaryRoot 'bootstrap.sql'

$repositoryRoot = Split-Path -Parent $PSScriptRoot

$clusterStarted = $false
$exitCode = 1
$previousAdminUrl = $env:GRAPHHELM_TEST_ADMIN_URL
$previousPgDump = $env:GRAPHHELM_TEST_PG_DUMP
$previousPgRestore = $env:GRAPHHELM_TEST_PG_RESTORE
$previousPgPassword = $env:PGPASSWORD

try {
    New-Item -ItemType Directory -Path $temporaryRoot -Force | Out-Null
    New-Item -ItemType Directory -Path $socketDirectory -Force | Out-Null
    Set-Content -LiteralPath $passwordFile -Value $bootstrapPassword -Encoding ascii -NoNewline

    Write-Step "Initialising throwaway cluster (port $port)."
    Invoke-Tool -Path $initdb -What 'initdb' -Arguments @(
        '--pgdata', $dataDirectory,
        '--username', $bootstrapRole,
        '--pwfile', $passwordFile,
        '--auth-local=trust',
        '--auth-host=scram-sha-256',
        '--encoding=UTF8',
        # The C locale makes text ordering identical to COLLATE "C", which is exactly the condition
        # under which collation-dependent ordering defects stay invisible. GRAPHHELM_PG_LOCALE lets
        # CI run the same suite under a real collation so that class has an actual regression guard.
        "--locale=$(if ($env:GRAPHHELM_PG_LOCALE) { $env:GRAPHHELM_PG_LOCALE } else { 'C' })",
        '--no-sync'
    )
    Remove-Item -LiteralPath $passwordFile -Force

    $serverOptions = "-p $port -c listen_addresses=127.0.0.1 -c fsync=off -c full_page_writes=off -c synchronous_commit=off -c max_connections=200"
    if (-not $onWindows) {
        $serverOptions = "$serverOptions -c unix_socket_directories=$socketDirectory"
    }

    # `pg_ctl start` must not be invoked through PowerShell's native-command pipeline. The server it
    # spawns is detached but inherits the caller's stdout and stderr handles, so PowerShell keeps
    # reading a stream that never reaches EOF and the script hangs after the server is already up.
    # Start-Process with explicit file redirection gives the child its own handles instead.
    $startOut = Join-Path $temporaryRoot 'pg_ctl-start.out'
    $startErr = Join-Path $temporaryRoot 'pg_ctl-start.err'
    # -Wait is also unusable here: it blocks on descendants, and the server is a descendant that by
    # design never exits. Launch without waiting and poll for readiness instead.
    Start-Process -FilePath $pgCtl -ArgumentList @(
        '-D', "`"$dataDirectory`"",
        '-l', "`"$logFile`"",
        '-o', "`"$serverOptions`"",
        'start'
    ) -NoNewWindow -RedirectStandardOutput $startOut -RedirectStandardError $startErr | Out-Null
    $clusterStarted = $true

    $ready = $false
    for ($attempt = 0; $attempt -lt 90; $attempt++) {
        Start-Sleep -Seconds 1
        & $pgIsReady '--host' '127.0.0.1' '--port' "$port" '--timeout' '2' *> $null
        if ($LASTEXITCODE -eq 0) { $ready = $true; break }
    }
    if (-not $ready) {
        if (Test-Path $logFile) { Get-Content $logFile -Tail 20 | ForEach-Object { Write-Warning "[ci/postgres] $_" } }
        throw 'the throwaway cluster did not become ready within 90 seconds'
    }
    Write-Step "Cluster is accepting connections on 127.0.0.1:$port."

    $statements = @(
        "CREATE ROLE $adminRole SUPERUSER CREATEDB CREATEROLE LOGIN PASSWORD '$adminPassword';",
        "CREATE DATABASE $databaseName OWNER $adminRole;"
    )
    Set-Content -LiteralPath $bootstrapSql -Value ([string]::Join([Environment]::NewLine, $statements)) -Encoding ascii

    $env:PGPASSWORD = $bootstrapPassword
    Invoke-Tool -Path $psql -What 'psql bootstrap' -Arguments @(
        '--host', '127.0.0.1',
        '--port', "$port",
        '--username', $bootstrapRole,
        '--dbname', 'postgres',
        '--no-password',
        '--quiet',
        '--set', 'ON_ERROR_STOP=1',
        '--file', $bootstrapSql
    )
    Remove-Item -LiteralPath $bootstrapSql -Force
    $env:PGPASSWORD = $adminPassword
    Write-Step "Created database '$databaseName' owned by role '$adminRole'."

    $dsnQuery = '?sslmode=disable'
    $env:GRAPHHELM_TEST_ADMIN_URL = "postgres://${adminRole}:${adminPassword}@127.0.0.1:$port/$databaseName$dsnQuery"
    $env:GRAPHHELM_TEST_PG_DUMP = $pgDump
    $env:GRAPHHELM_TEST_PG_RESTORE = $pgRestore
    Write-Step "GRAPHHELM_TEST_ADMIN_URL = postgres://${adminRole}:***@127.0.0.1:$port/$databaseName"
    Write-Step "GRAPHHELM_TEST_PG_DUMP = $pgDump"
    Write-Step "GRAPHHELM_TEST_PG_RESTORE = $pgRestore"

    Push-Location -LiteralPath $repositoryRoot
    try {
        Write-Step "Running: $TestCommand $([string]::Join(' ', $TestArgs))"
        # cargo writes all of its progress to stderr. Under Windows PowerShell 5.1 a redirected
        # native stderr line is wrapped in a NativeCommandError ErrorRecord, which the script-wide
        # $ErrorActionPreference='Stop' promotes to a terminating error - so merely piping this
        # script's output to a file would abort the run before the first test executed. A native
        # process is judged by its exit code, which stays authoritative here.
        $previousPreference = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        # #909, found by the unnumbered ISSUES lane at f99b269c: DECLARE IT BEFORE THE PIPE.
        # `Tee-Object -Variable` only assigns when the pipeline produces at least one object, and this
        # file runs under `Set-StrictMode -Version 2.0` (:46), where reading an unset variable THROWS.
        # cargo writes diagnostics to stderr, so a compile error produces a non-zero exit and EMPTY
        # stdout -- the throw then happens at the read below, before `exit $exitCode` on the last line
        # of the file, and 101 is reported as 1 with no count file written. A PR whose thesis is that
        # a run which measured nothing must not look like one that measured cannot afford to turn a
        # failing run into `unknown`. Tee overwrites this whenever there IS output, so nothing else
        # changes; the empty case now reads as groups = 0, which is exactly `unknown`.
        $capturedStdout = @()
        try {
            # #909: TEE, not redirect. The lines still reach the console exactly as before (Tee-Object
            # passes them down the success stream), and a copy is kept so the run can say how many
            # tests it EXECUTED. Only stdout is piped: libtest's summaries are on stdout, cargo's
            # progress is on stderr, and merging the two with 2>&1 would wrap every stderr line in a
            # NativeCommandError -- the exact hazard the comment above this block is about.
            #
            # $LASTEXITCODE is set by the native command and a downstream cmdlet does not overwrite it
            # ($? would be the cmdlet's; that is a different variable and is not read here).
            & $TestCommand @TestArgs | Tee-Object -Variable capturedStdout
            $exitCode = $LASTEXITCODE
        } finally {
            $ErrorActionPreference = $previousPreference
        }
    } finally {
        Pop-Location
    }
    Write-Step "Test command exited with code $exitCode."

    # #909: HOW MANY TESTS THIS STAGE ACTUALLY RAN.
    #
    # Invoke-Postgres publishes Get-Content -Tail 80 of each stream and then deletes the temp files,
    # so the head of this output does not survive to any reader. A stage that ran every ignored test
    # and one that ran none produced the same artefact, and on 2026-09-05 a lane read one as the other
    # and published it (retracted on #752/#909). A COUNT survives a tail; a transcript does not.
    #
    # The line below is printed LAST on purpose, so it is inside the 80 that survive. The file is the
    # authoritative copy -- a caller that sets GRAPHHELM_PG_COUNT_FILE never has to parse a tail.
    $capturedLines = @($capturedStdout | ForEach-Object { [string] $_ })
    $executionTotals = Get-ExecutedTestCount -Lines $capturedLines
    $executionVerdict = Get-TestExecutionVerdict -Totals $executionTotals
    Write-Step ("executed {0} test(s) across {1} binary summary(ies); verdict {2}." -f `
        $executionTotals.executed, $executionTotals.groups, $executionVerdict)
    # #909, and the reason this line is not decoration: Invoke-Postgres publishes the last 80 lines of
    # this stream. Printing how many there WERE makes the cut visible at the place a reader looks.
    # An unmarked truncation reads as a complete record -- that is how a 160-line tail was reported as
    # a whole stage on 2026-09-05. A reader who sees "1847 stdout line(s)" beside 80 published ones
    # cannot make that mistake.
    Write-Step ("stdout produced {0} line(s); the stage publishes the last 80 of each stream." -f $capturedLines.Count)
    if ($env:GRAPHHELM_PG_COUNT_FILE) {
        # Flat literal, ASCII, LF: the same shape the slot ledger uses, so a bash reader and a
        # PowerShell reader see identical bytes.
        $payload = @(
            "verdict=$executionVerdict",
            "executed=$($executionTotals.executed)",
            "groups=$($executionTotals.groups)",
            "passed=$($executionTotals.passed)",
            "failed=$($executionTotals.failed)",
            "ignored=$($executionTotals.ignored)",
            "filteredOut=$($executionTotals.filteredOut)",
            "stdoutLines=$($capturedLines.Count)"
        ) -join "`n"
        [System.IO.File]::WriteAllText($env:GRAPHHELM_PG_COUNT_FILE, $payload + "`n", [System.Text.UTF8Encoding]::new($false))
    }
} finally {
    $env:GRAPHHELM_TEST_ADMIN_URL = $previousAdminUrl
    $env:GRAPHHELM_TEST_PG_DUMP = $previousPgDump
    $env:GRAPHHELM_TEST_PG_RESTORE = $previousPgRestore
    $env:PGPASSWORD = $previousPgPassword

    if ($clusterStarted) {
        Write-Step 'Stopping throwaway cluster.'
        & $pgCtl '-D' $dataDirectory '-m' 'immediate' '-w' '-t' '60' 'stop'
        if ($LASTEXITCODE -ne 0) {
            Write-Warning "[ci/postgres] pg_ctl stop exited with code $LASTEXITCODE."
        }
    }
    Remove-TreeWithRetry -Path $temporaryRoot
    Write-Step 'Throwaway cluster removed.'
}

exit $exitCode
