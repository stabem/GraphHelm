# Persona host: makes chartered personas answer, and lets them charter new ones.
#
# WHAT A PERSONA IS HERE. A signal in a task's log with kind `persona_created`: the envelope's
# `to` names the persona's actor id, its `description` is the charter. The LOG IS THE REGISTRY -
# this host holds no state of its own beyond an in-memory evidence cache; kill it and restart it
# and nothing is lost, because everything it decides is re-derived from the journal.
#
# WHAT THIS HOST DOES, each cycle, per execution:
#   1. reads the event tail and collects: chartered personas, spoken messages, who answered what;
#   2. for every message ADDRESSED to a persona (`to` in the envelope) that no reply of that
#      persona answers yet (`replyTo` chains), runs the persona's engine and posts the reply,
#      addressed back (to = asker, replyTo = the message's signal id);
#   3. births any persona a persona asked to create (the engine may return `newPersonas`), by
#      posting `persona_created` signed by the CREATOR, plus a first hello from the newborn.
#
# WHY PERSONAS ONLY ANSWER WHAT IS ADDRESSED TO THEM. "Say to the room" stays a human act; if
# every persona answered every message, three personas would answer each other forever. Which is
# also why there is a BUDGET: at most -ReplyBudget persona-authored messages since the last
# HUMAN (owner) message per execution. Exhausted, the host says so once in the thread and goes
# quiet until a person speaks. (This is the backpressure gap the agents themselves flagged on the
# board - and per their own review, the pause is VISIBLE, never a silent drop.)
#
# IDEMPOTENCY IS THE DEDUP. Every post uses a key derived from what it answers
# (persona-<id>-re-<signalId>), so a crashed cycle retried later cannot double-post: the Runtime
# refuses the duplicate and the refusal is the confirmation.
#
# The engine is `claude -p`, TEXT ONLY - no tools, no MCP. The host composes the prompt and the
# host posts the envelope, so an engine cannot forge a signal type, an actor, or a severity.

param(
    [string] $RuntimeUrl = "http://127.0.0.1:8791",
    [Parameter(Mandatory = $true)] [string] $TokenFile,
    [int] $IntervalSeconds = 6,
    [int] $ReplyBudget = 10,
    [int] $ThreadTail = 15,
    [string] $ClaudeCommand = "claude"
)

$ErrorActionPreference = "Stop"
$token = (Get-Content $TokenFile -Raw).Trim()
$script:evidenceCache = @{}
# Engine attempts per message, in memory. Three strikes and the message is skipped LOUDLY: any
# posting defect (like the over-long key above) must cost at most three model sessions, never one
# per cycle forever. A restart resets the count, which grants three more - acceptable, because a
# restart is a human act.
$script:attempts = @{}

$MD5 = [System.Security.Cryptography.MD5]::Create()
function Short-Key([string] $seed) {
    # The serve layer bounds the Idempotency-Key HEADER; a key built by concatenating ids blew
    # through it (~95 chars, refused). Twelve hex of a hash of the same seed keeps the key
    # deterministic - same message, same key, dedup intact - and always fits.
    $bytes = $MD5.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($seed))
    return (($bytes | ForEach-Object { $_.ToString("x2") }) -join "").Substring(0, 12)
}

function UrlId([string] $id) {
    # Opaque ids (`is_opaque_id`, core/protocols/src/persistence.rs) permit `?`, `#`, `&`, `=` -
    # every byte 0x21-0x2e, 0x30-0x39, 0x3b-0x5b, 0x5d-0x7e is legal. A valid id carrying one of
    # those changes the request PATH or QUERY the moment it is interpolated raw (PR #467 review):
    # an execution id of `abc?x=1` turns `/v1/executions/abc?x=1/signal` into a request for a
    # different path with an extra query parameter, not a 404 on the id the caller meant.
    # EscapeDataString is safe for BOTH a path segment and a query value, so one helper covers
    # every interpolation site below rather than two call conventions someone has to choose between.
    [System.Uri]::EscapeDataString($id)
}

function Read-Api([string] $path) {
    (Invoke-RestMethod -Uri "$RuntimeUrl$path" -Headers @{ Authorization = "Bearer $token" } -TimeoutSec 20).data
}

function Post-Signal([string] $execution, [string] $actor, [hashtable] $signal, [string] $key) {
    $headers = @{
        Authorization            = "Bearer $token"
        "X-GraphHelm-Actor"      = $actor
        "X-GraphHelm-Actor-Type" = "agent"
        "Idempotency-Key"        = $key
    }
    $body = @{ signal = $signal } | ConvertTo-Json -Depth 6
    try {
        $null = Invoke-RestMethod -Uri "$RuntimeUrl/v1/executions/$(UrlId $execution)/signal" -Method POST `
            -Headers $headers -ContentType "application/json" -Body $body -TimeoutSec 30
        Write-Host "[host] posted as ${actor}: $key"
        return $true
    } catch {
        # ONLY the exact conflict code is the dedup working. The first version matched the WORD
        # "idempotency" case-insensitively, and the refusal for an over-long Idempotency-Key
        # header contains that word too - so a key that could never land was read as "already
        # landed", and the host re-ran the engine every cycle, silently, at one model session per
        # attempt (measured 2026-08-30, ten sessions burned). A dedup detector that matches
        # anything broader than the conflict code converts every key defect into an infinite paid
        # retry.
        $text = "$($_.ErrorDetails.Message)"
        if ($text -match "GHE003_IDEMPOTENCY_CONFLICT") { return $false }
        Write-Host "[host] post REFUSED ($key): $($text.Substring(0, [Math]::Min(200, $text.Length)))"
        return $false
    }
}

function Open-Envelope([string] $execution, [string] $evidenceId) {
    $cacheKey = "$execution/$evidenceId"
    if ($script:evidenceCache.ContainsKey($cacheKey)) { return $script:evidenceCache[$cacheKey] }
    try {
        $content = (Read-Api "/v1/executions/$(UrlId $execution)/evidence/$(UrlId $evidenceId)").content
        $envelope = $content | ConvertFrom-Json
        $script:evidenceCache[$cacheKey] = $envelope
        return $envelope
    } catch { return $null }
}

function New-Envelope([string] $execution, [string] $id, [string] $kind, [string] $sourceId, [string] $description, [string] $to, [string] $replyTo) {
    $signal = @{
        id          = $id
        # "tool", never "user": every signal this host posts is agent-authored (Post-Signal
        # sends X-GraphHelm-Actor-Type: agent), and an envelope claiming user provenance would
        # contradict its own actor in immutable history (PR #467 review). The schema's closed
        # vocabulary has no "agent"; "tool" is the same mapping the Studio's client uses.
        source      = @{ type = "tool"; id = $sourceId }
        type        = $kind
        severity    = "low"
        description = $description
        evidence    = @($execution)
        emittedAt   = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    }
    if ($to) { $signal.to = $to }
    if ($replyTo) { $signal.replyTo = $replyTo }
    return $signal
}

function Invoke-Engine([string] $prompt) {
    # IN MEMORY, END TO END (PR #467 review, P1). The prompt carries the decrypted conversation
    # and the output carries the model's words, and both used to transit $env:TEMP as plaintext
    # files - any interruption between write and Remove left them on disk, and the model call
    # itself takes minutes, so a try/finally would only narrow that window, not close it. A
    # redirected pipe has no window: nothing this function handles ever touches a filesystem.
    # cmd.exe stays as the launcher (it resolves $ClaudeCommand the way the old invocation did,
    # .cmd shims included) but its stdio is redirected, not filed. stderr is drained async so a
    # chatty engine cannot deadlock the pipe.
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = "cmd.exe"
    $psi.Arguments = "/c $ClaudeCommand -p"
    $psi.UseShellExecute = $false
    $psi.RedirectStandardInput = $true
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.StandardOutputEncoding = [System.Text.Encoding]::UTF8
    $psi.StandardErrorEncoding = [System.Text.Encoding]::UTF8
    $process = [System.Diagnostics.Process]::Start($psi)
    $errTask = $process.StandardError.ReadToEndAsync()
    $writer = New-Object System.IO.StreamWriter($process.StandardInput.BaseStream, (New-Object System.Text.UTF8Encoding($false)))
    $writer.Write($prompt)
    $writer.Close()
    $raw = $process.StandardOutput.ReadToEnd() + $errTask.Result
    $process.WaitForExit()
    # The engine is instructed to answer ONLY JSON. Tolerate prose around it: take the outermost
    # object; if nothing parses, the whole text becomes the reply and no personas are created -
    # a malformed engine answer degrades to words, never to unintended structure.
    $start = $raw.IndexOf("{")
    $end = $raw.LastIndexOf("}")
    if ($start -ge 0 -and $end -gt $start) {
        try { return $raw.Substring($start, $end - $start + 1) | ConvertFrom-Json } catch {}
    }
    return [pscustomobject]@{ reply = $raw.Trim(); newPersonas = @() }
}

# The whole log, read INCREMENTALLY: the endpoint pages oldest-first with an exclusive `after`
# cursor, so a bare `limit=200` re-reads the same FIRST page forever and the host goes blind the
# moment an execution outgrows it (PR #467 review). The cursor and the accumulated events are
# per-execution memory; each cycle fetches only what is new, which is cheaper than the old
# single page re-read, not dearer.
$script:eventTail = @{}
function Read-AllEvents([string] $execution) {
    if (-not $script:eventTail.ContainsKey($execution)) {
        $script:eventTail[$execution] = [pscustomobject]@{ cursor = 0; events = (New-Object System.Collections.ArrayList) }
    }
    $held = $script:eventTail[$execution]
    $exhausted = $false
    for ($i = 0; $i -lt 50; $i++) {
        $page = Read-Api "/v1/executions/$(UrlId $execution)/events?after=$($held.cursor)&limit=200"
        foreach ($event in $page.events) {
            $null = $held.events.Add($event)
            $held.cursor = $event.sequence
        }
        if (@($page.events).Count -lt 200) { $exhausted = $true; break }
    }
    # An incomplete projection SAYS SO, same convention as Read-AllExecutions below (#650, PR #467
    # review): the 50-page cap (10,000 events) bounds a runaway catch-up on a huge backlog, but a
    # paging run that stopped mid-history must never be acted on as if it were current - an
    # oldest-first PREFIX can be missing the very message Step-Execution would answer, or hide
    # that the reply budget is already exhausted. The caller refuses to process on `exhausted =
    # $false` rather than silently reasoning from a stale prefix; the NEXT cycle resumes paging
    # from the same cursor and eventually catches up.
    if (-not $exhausted) {
        Write-Host "[host] WARNING: event paging for $execution stopped at the 50-page cap with more remaining - this cycle will not act on a partial tail"
    }
    return [pscustomobject]@{ events = @($held.events); exhausted = $exhausted }
}

function Step-Execution([string] $execution) {
    $tail = Read-AllEvents $execution
    # Refuse to act on a partial prefix (#650): the WARNING already fired inside Read-AllEvents,
    # so this is silent on the common path and only ever skips a cycle when paging is genuinely
    # behind. The next cycle resumes from the same cursor.
    if (-not $tail.exhausted) { return }
    $events = $tail.events
    $page = [pscustomobject]@{ events = $events }
    $signals = @($page.events | Where-Object { $_.kind.type -eq "signal_recorded" })
    if ($signals.Count -eq 0) { return }

    $personas = @{}
    $messages = @()
    $answered = @{}
    $lastOwnerSeq = 0
    $agentSinceOwner = 0
    # Every actor the log has ever seen, personas included. This is the reserved-name list the
    # board's own security persona demanded (signal persona-seguranca-re-pergunta-seguranca-1,
    # 2026-08-30): without it, a persona could charter a newborn named after a REAL actor -
    # claude-code, codex, the human's own session - and from then on the host would answer
    # messages addressed to that name, signed with it. Impersonation without forging anything,
    # just by choosing the name. Names are reserved the moment the log shows them.
    $actorsSeen = @{ "persona-host" = $true }
    foreach ($event in $page.events) {
        if ($event.actor -and $event.actor.id) { $actorsSeen[$event.actor.id] = $true }
    }
    foreach ($event in $signals) {
        $envelope = Open-Envelope $execution $event.evidenceRefs[0].evidenceId
        if ($null -eq $envelope) { continue }
        $entry = [pscustomobject]@{
            sequence = $event.sequence
            actor    = $event.actor.id
            actorType = $event.actor.type
            signalId = $event.kind.data.signalId
            kind     = $envelope.type
            text     = $envelope.description
            to       = $envelope.to
            replyTo  = $envelope.replyTo
        }
        if ($entry.kind -eq "persona_created" -and $entry.to) {
            $personas[$entry.to] = $entry.text
        } elseif ($entry.kind -eq "operator_note") {
            $messages += $entry
            if ($entry.replyTo) { $answered["$($entry.actor)|$($entry.replyTo)"] = $true }
        }
        if ($event.actor.type -eq "owner") { $lastOwnerSeq = $event.sequence; $agentSinceOwner = 0 }
        elseif ($event.actor.type -eq "agent" -and $entry.kind -eq "operator_note") { $agentSinceOwner++ }
    }
    if ($personas.Count -eq 0) { return }

    if ($agentSinceOwner -ge $ReplyBudget) {
        # Visible, once per exhaustion window: the deterministic key makes the second attempt a
        # dedup refusal instead of a second note.
        #
        # SHORT-KEYED (#652): the raw form was "budget-$execution-$lastOwnerSeq", and an execution
        # id can run up to 128 bytes (is_opaque_id) - past the serve API's 64-char Idempotency-Key
        # limit, every budget notice for a long-id execution was refused before it could post, and
        # the host silently lost the one message that tells a person the personas went quiet.
        # Short-Key's own hash-and-truncate is exactly the tool already used for `replyKey` below;
        # a distinct prefix ("budget-", 12 hex) keeps this key deterministic and out of collision
        # with the reply keys' own "re-" namespace.
        $budgetKey = "budget-$(Short-Key "$execution|$lastOwnerSeq")"
        $note = New-Envelope $execution $budgetKey "operator_note" "persona-host" `
            "As personas chegaram ao limite de $ReplyBudget respostas sem mensagem humana. Escreve algo para retomarem." "" ""
        $null = Post-Signal $execution "persona-host" $note $budgetKey
        return
    }

    foreach ($message in $messages) {
        if (-not $message.to) { continue }                           # to the room: humans read it
        if (-not $personas.ContainsKey($message.to)) { continue }    # addressed to no persona of ours
        if ($message.actor -eq $message.to) { continue }             # never answer yourself
        if ($answered.ContainsKey("$($message.to)|$($message.signalId)")) { continue }

        $persona = $message.to
        $attemptKey = "$execution|$persona|$($message.signalId)"
        $tries = if ($script:attempts.ContainsKey($attemptKey)) { $script:attempts[$attemptKey] } else { 0 }
        if ($tries -ge 3) { continue }
        if ($tries -eq 2) {
            Write-Host "[host] GIVING UP on $($message.signalId) for $persona after 3 attempts - see refusals above"
        }
        $script:attempts[$attemptKey] = $tries + 1
        $tail = ($messages | Select-Object -Last $ThreadTail | ForEach-Object {
            "[$($_.actor)] (signal $($_.signalId)): $($_.text)"
        }) -join "`n"
        $prompt = @(
            "Es a persona '$persona' num quadro de tarefas GraphHelm onde humanos e agentes conversam.",
            "O teu charter:", $personas[$persona], "",
            "Conversa recente (mais antiga primeiro):", $tail, "",
            "A mensagem dirigida a ti (de $($message.actor), signal $($message.signalId)):", $message.text, "",
            "Responde como a persona, na lingua da conversa, em 2-6 frases.",
            "Se concluires que a equipa PRECISA de uma especialidade que nenhuma persona cobre, podes criar uma nova.",
            "Responde APENAS com JSON neste formato exato:",
            '{"reply":"...","newPersonas":[{"id":"nome-kebab","charter":"..."}]}',
            "newPersonas e opcional; omite quando ninguem novo e necessario."
        ) -join "`n"

        Write-Host "[host] $persona is answering $($message.signalId) in $execution"
        $out = Invoke-Engine $prompt
        if ([string]::IsNullOrWhiteSpace($out.reply)) { continue }

        $replyKey = "re-$(Short-Key "$persona|$($message.signalId)")"
        $reply = New-Envelope $execution $replyKey "operator_note" $persona $out.reply $message.actor $message.signalId
        $null = Post-Signal $execution $persona $reply $replyKey

        foreach ($born in @($out.newPersonas)) {
            if (-not $born.id -or -not $born.charter) { continue }
            $bornId = ($born.id -replace "[^a-z0-9-]", "").Trim("-")
            if ($bornId.Length -gt 32) { $bornId = $bornId.Substring(0, 32).Trim("-") }
            if (-not $bornId -or $personas.ContainsKey($bornId)) { continue }
            if ($actorsSeen.ContainsKey($bornId)) {
                # Refused OUT LOUD: a silent skip would hide the attempt from the very register
                # the identity guard watches.
                $refuseKey = "no-$(Short-Key "$bornId|$($message.signalId)")"
                $refusal = New-Envelope $execution $refuseKey "operator_note" "persona-host" `
                    "Recusado: '$bornId' e um actor que este quadro ja conhece; uma persona nao pode nascer com o nome de alguem real (pedido por $persona)." "" ""
                $null = Post-Signal $execution "persona-host" $refusal $refuseKey
                continue
            }
            $birthKey = "persona-birth-$bornId"
            $birth = New-Envelope $execution $birthKey "persona_created" $persona $born.charter $bornId ""
            if (Post-Signal $execution $persona $birth $birthKey) {
                $helloKey = "persona-hello-$bornId"
                $hello = New-Envelope $execution $helloKey "operator_note" $bornId `
                    "Sou $bornId, criada por $persona. $($born.charter)" "" ""
                $null = Post-Signal $execution $bornId $hello $helloKey
            }
        }
    }
}

# Every page of the index, not just the first: the endpoint sorts by id and caps a page at 100,
# so a repository past 100 streams would leave the later ones invisible to this host forever -
# their chartered personas never answering (PR #467 review). Bounded at 50 hops (5,000 runs),
# the same runaway-loop shape as Read-AllEvents.
function Read-AllExecutions {
    $rows = New-Object System.Collections.ArrayList
    $after = $null
    $exhausted = $false
    for ($i = 0; $i -lt 50; $i++) {
        $path = "/v1/executions?limit=100"
        if ($after) { $path += "&after=$after" }
        $page = Read-Api $path
        foreach ($row in $page.executions) { $null = $rows.Add($row) }
        if (-not $page.hasMore -or -not $page.nextCursor) { $exhausted = $true; break }
        $after = $page.nextCursor
    }
    # An incomplete projection SAYS SO (PR #467 review): a silent cap reads as "covered
    # everything" precisely when it did not. The cap stays - a runaway-loop guard for an
    # example host - but crossing it is loud, once per cycle.
    if (-not $exhausted) {
        Write-Host "[host] WARNING: execution index paging stopped at the 50-page cap with more remaining - streams beyond it are NOT being served this cycle"
    }
    return @($rows)
}

Write-Host "[host] watching $RuntimeUrl every ${IntervalSeconds}s (budget $ReplyBudget replies per human message)"
while ($true) {
    try {
        $executions = Read-AllExecutions
        foreach ($execution in $executions) { Step-Execution $execution.executionId }
    } catch {
        Write-Host "[host] cycle failed: $($_.Exception.Message)"
    }
    Start-Sleep -Seconds $IntervalSeconds
}
