# Claude Code hook relay: an agent's observed activity, recorded as a SIGNAL in the one log.
#
# WHAT THIS IS. Claude Code hooks (PostToolUse, Stop, ...) hand a JSON payload on stdin to any
# command you configure. This script is that command: it reads the payload and posts ONE
# authenticated `signal` into a GraphHelm execution's append-only log. The Studio's thread and
# board then show the agent's activity live ("Edit on src/App.tsx") the same way they show
# everything else - as a recorded, attributed fact.
#
# WHAT THIS DELIBERATELY IS NOT. The reference for this feature (agenttrail's `hook` command)
# relays the same payload to an UNAUTHENTICATED localhost HTTP endpoint, where any process can
# post anything as anyone. That is the trap this repo refuses: here the relay authenticates with
# a bearer token, the write is attributed (actor headers, type `agent`), and the Runtime's own
# rules decide whether it lands. No side channel, no second truth - the log or nothing.
#
# WIRING (in the watched project's .claude/settings.json):
#   "hooks": {
#     "PostToolUse": [{ "hooks": [{ "type": "command", "command":
#       "powershell -File <this file> -TokenFile <token> -ExecutionId <run id>" }] }]
#   }
# The hook payload arrives on stdin; nothing else is read from the environment unless the
# parameters below say so.
#
# IDEMPOTENCY IS THE DEDUP. The key is a short hash of the payload's identifying fields, so a
# hook Claude Code retries cannot double-post: the Runtime refuses the duplicate, and that
# refusal is the confirmation (exit 0 - a landed duplicate is success, not failure).

param(
    [string] $RuntimeUrl = $(if ($env:GRAPHHELM_RUNTIME_URL) { $env:GRAPHHELM_RUNTIME_URL } else { "http://127.0.0.1:8791" }),
    [Parameter(Mandatory = $true)] [string] $TokenFile,
    [Parameter(Mandatory = $true)] [string] $ExecutionId,
    # The actor the signal is recorded AS. An agent, never an owner: a hook firing is the
    # machine's act, and the log's attribution must say so.
    [string] $Actor = "claude-code"
)

$ErrorActionPreference = "Stop"
$token = (Get-Content $TokenFile -Raw).Trim()

# The whole hook payload, from stdin - the one place Claude Code puts it.
$raw = [Console]::In.ReadToEnd()
if (-not $raw -or $raw.Trim().Length -eq 0) { exit 0 }
try { $hook = $raw | ConvertFrom-Json } catch { exit 0 }

$eventName = if ($hook.hook_event_name) { [string]$hook.hook_event_name } else { "hook" }
$toolName = if ($hook.tool_name) { [string]$hook.tool_name } else { $null }

# One readable sentence, not the payload: the log records that the agent ACTED, and where. The
# raw tool input can carry anything (secrets included) and is deliberately not relayed.
$what = if ($toolName) { "$toolName" } else { $eventName }
$file = $null
if ($hook.tool_input -and $hook.tool_input.file_path) { $file = [string]$hook.tool_input.file_path }
if ($file) {
    # The basename only: the log is readable by the whole room, and a full local path is the
    # operator's machine layout, not the work.
    $what = "$what on $([System.IO.Path]::GetFileName($file))"
}
$description = "[$eventName] $what"

$MD5 = [System.Security.Cryptography.MD5]::Create()
$seed = "$($hook.session_id)|$eventName|$what|$($hook.tool_use_id)"
$bytes = $MD5.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($seed))
$key = "hook-" + ((($bytes | ForEach-Object { $_.ToString("x2") }) -join "").Substring(0, 12))

$signal = @{
    id          = $key
    source      = @{ type = "tool"; id = $Actor }
    type        = "agent_activity"
    severity    = "low"
    description = $description
    evidence    = @($ExecutionId)
    emittedAt   = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
}

$headers = @{
    Authorization            = "Bearer $token"
    "X-GraphHelm-Actor"      = $Actor
    "X-GraphHelm-Actor-Type" = "agent"
    "Idempotency-Key"        = $key
}
$body = @{ signal = $signal } | ConvertTo-Json -Depth 6

try {
    $null = Invoke-RestMethod -Uri "$RuntimeUrl/v1/executions/$ExecutionId/signal" -Method POST `
        -Headers $headers -ContentType "application/json" -Body $body -TimeoutSec 15
    exit 0
} catch {
    # Only the exact conflict code is the dedup working; anything else is a real miss. Either
    # way the HOOK must not fail the agent's turn - observability never blocks the work.
    exit 0
}
