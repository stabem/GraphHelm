# studio-up: the whole Studio, one command, cold start included.
#
#   powershell -File apps/studio/tools/studio-up.ps1 -Events C:\myproject\.graphhelm\events
#
# What it does, in order:
#   1. starts `graphhelm serve` on -Bind IF nothing is answering there yet (an already-running
#      Runtime is reused, never doubled - two writers on one events dir is the one thing the
#      slot discipline exists to prevent);
#   2. installs the Studio's dependencies IF node_modules is missing (npm ci, lockfile-exact);
#   3. starts the dev server with the environment that makes the page open CONNECTED
#      (GRAPHHELM_EVENTS -> the dev session endpoint hands the token to the page; nobody pastes);
#   4. opens the browser, unless -NoBrowser.
#
# Everything stays on loopback: the Runtime binds 127.0.0.1 and refuses anything else, the token
# never leaves this machine, and this script never prints it.

param(
    # The events directory the Runtime owns. `graphhelm serve` writes its bearer token BESIDE it.
    [Parameter(Mandatory = $true)] [string] $Events,
    [string] $Bind = "127.0.0.1:8791",
    [int] $StudioPort = 5183,
    # A label for the rail; purely cosmetic.
    [string] $Project = "",
    # The graphhelm binary. Defaults to PATH; point it at a `cargo build` output if unreleased.
    [string] $GraphHelm = "graphhelm",
    # Sealing (PR #467 review): without a keyring the Runtime cannot seal a message envelope,
    # so the Studio's message box and graphhelm_send_message are REFUSED on every send. Pass the
    # keyring directory and key id `graphhelm gateway keyring init` created (the passphrase
    # travels in GRAPHHELM_GATEWAY_KEY / GRAPHHELM_EVENTS_KEY, never as a parameter).
    [string] $Keyring = "",
    [string] $KeyId = "",
    [switch] $NoBrowser
)

$ErrorActionPreference = "Stop"
$studio = Split-Path -Parent $PSScriptRoot  # this file lives in apps/studio/tools/

if (-not (Test-Path $Events)) {
    Write-Host "[up] events directory not found, creating: $Events"
    New-Item -ItemType Directory -Force -Path $Events | Out-Null
}

$runtimeUrl = "http://$Bind"

# The token `serve` writes beside the events directory - the proof of WHICH runtime answers.
function Get-EventsToken {
    $trimmed = $Events.TrimEnd("\", "/")
    $tokenFile = Join-Path (Split-Path -Parent $trimmed) ("$(Split-Path -Leaf $trimmed).token")
    if (Test-Path $tokenFile) { return (Get-Content $tokenFile -Raw).Trim() }
    return $null
}

# 1 - the Runtime, only if that address is silent. /health answers unauthenticated by design -
# so an occupied address is verified with THIS project's token before being reused: any healthy
# service would pass the bare probe, and proxying the requested directory's token to an
# unrelated runtime opens the Studio disconnected or, worse, on the wrong project
# (PR #467 review).
$alive = $false
try {
    $null = Invoke-WebRequest -Uri "$runtimeUrl/health" -TimeoutSec 2 -UseBasicParsing
    $alive = $true
} catch {}
if ($alive) {
    $token = Get-EventsToken
    if (-not $token) {
        throw "Something already answers at $runtimeUrl, and no token exists beside $Events to prove it serves this project. Stop that service or pass a different -Bind."
    }
    try {
        $null = Invoke-RestMethod -Uri "$runtimeUrl/v1/executions?limit=1" -Headers @{ Authorization = "Bearer $token" } -TimeoutSec 5
        Write-Host "[up] Runtime already answering at $runtimeUrl and this project's token opens it - reusing it"
    } catch {
        throw "Something answers at $runtimeUrl but refuses this project's token - it is a different service or another project's Runtime. Stop it or pass a different -Bind."
    }
}
if (-not $alive) {
    $serveArgs = @("serve", "--events", $Events, "--bind", $Bind)
    if ($Keyring) { $serveArgs += @("--keyring", $Keyring) }
    if ($KeyId) { $serveArgs += @("--key-id", $KeyId) }
    Write-Host "[up] starting: $GraphHelm $($serveArgs -join ' ')"
    Start-Process -FilePath $GraphHelm -ArgumentList $serveArgs -WindowStyle Hidden
    $deadline = (Get-Date).AddSeconds(20)
    while ((Get-Date) -lt $deadline) {
        try {
            $null = Invoke-WebRequest -Uri "$runtimeUrl/health" -TimeoutSec 2 -UseBasicParsing
            $alive = $true
            break
        } catch { Start-Sleep -Milliseconds 400 }
    }
    if (-not $alive) { throw "The Runtime did not answer at $runtimeUrl within 20s. Is '$GraphHelm' on PATH (or pass -GraphHelm)?" }
    Write-Host "[up] Runtime up at $runtimeUrl"
    if (-not $Keyring) {
        # Said HERE, not discovered at the first refused send: without sealing there is no
        # durable copy of a message a browser can name, so the Runtime refuses SayBox and
        # graphhelm_send_message - the boards and threads still read fine.
        Write-Warning "[up] no -Keyring given: the Studio opens read-and-drive only - SENDING MESSAGES WILL BE REFUSED. Create one with 'graphhelm gateway keyring init' and pass -Keyring/-KeyId."
    }
}

# 2 - dependencies, only on a cold tree.
if (-not (Test-Path (Join-Path $studio "node_modules"))) {
    Write-Host "[up] installing Studio dependencies (npm ci, first run only)"
    Push-Location $studio
    try { npm ci } finally { Pop-Location }
}

# 3 - the dev server, with the environment the auto-connect session endpoint reads. The session
# NONCE is generated here and travels only through this process's environment and the URL below:
# the dev server hands the token exclusively to a caller presenting it, because loopback is not a
# user boundary and the token file is owner-only on disk for a reason (PR #467 review).
$env:GRAPHHELM_EVENTS = $Events
$env:GRAPHHELM_RUNTIME_URL = $runtimeUrl
if ($Project) { $env:GRAPHHELM_PROJECT = $Project }
$rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
$nonceBytes = New-Object byte[] 16
$rng.GetBytes($nonceBytes)
$nonce = (($nonceBytes | ForEach-Object { $_.ToString("x2") }) -join "")
$env:GRAPHHELM_STUDIO_SESSION_NONCE = $nonce

# 4 - the door, with the key in the URL.
if (-not $NoBrowser) {
    Start-Job -ScriptBlock {
        param($port, $nonce)
        $deadline = (Get-Date).AddSeconds(30)
        while ((Get-Date) -lt $deadline) {
            try {
                $null = Invoke-WebRequest -Uri "http://127.0.0.1:$port/" -TimeoutSec 2 -UseBasicParsing
                Start-Process "http://127.0.0.1:$port/?session=$nonce"
                return
            } catch { Start-Sleep -Milliseconds 500 }
        }
    } -ArgumentList $StudioPort, $nonce | Out-Null
}

Write-Host "[up] Studio starting at http://127.0.0.1:$StudioPort - the page opens already connected"
Push-Location $studio
try { npm run dev -- --port $StudioPort } finally { Pop-Location }
