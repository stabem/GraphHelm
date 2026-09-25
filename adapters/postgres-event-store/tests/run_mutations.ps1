param(
    [int]$StartMutation = 1,
    [int]$EndMutation = 8
)

$ErrorActionPreference = 'Stop'
$script:MutationIndex = 0

if (-not $env:GRAPHHELM_TEST_ADMIN_URL) {
    throw 'GRAPHHELM_TEST_ADMIN_URL is required'
}

$RepositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path

function Invoke-KilledMutation {
    param(
        [string]$RelativePath,
        [string]$Needle,
        [string]$Replacement,
        [string[]]$CargoArguments
    )

    $script:MutationIndex++
    if ($script:MutationIndex -lt $StartMutation -or $script:MutationIndex -gt $EndMutation) {
        return
    }

    $Path = Join-Path $RepositoryRoot $RelativePath
    $OriginalBytes = [IO.File]::ReadAllBytes($Path)
    $Original = [Text.Encoding]::UTF8.GetString($OriginalBytes)
    if (-not $Original.Contains($Needle)) {
        throw "Mutation needle not found in $RelativePath"
    }
    try {
        [IO.File]::WriteAllText(
            $Path,
            $Original.Replace($Needle, $Replacement),
            [Text.UTF8Encoding]::new($false)
        )
        & cargo +1.97.1 test -p graphhelm-postgres-event-store --features test-support --locked @CargoArguments
        if ($LASTEXITCODE -eq 0) {
            throw "Mutation survived in $RelativePath"
        }
    }
    finally {
        [IO.File]::WriteAllBytes($Path, $OriginalBytes)
    }
}

Push-Location $RepositoryRoot
try {
    Invoke-KilledMutation `
        'adapters/postgres-event-store/migrations/0001_event_evidence.sql' `
        'ALTER TABLE public.%I FORCE ROW LEVEL SECURITY' `
        'ALTER TABLE public.%I NO FORCE ROW LEVEL SECURITY' `
        @('--test', 'migration', 'migration_enforces_rls_and_runtime_role_separation', '--', '--ignored', '--exact')

    Invoke-KilledMutation `
        'adapters/postgres-event-store/src/scope.rs' `
        ', true)' `
        ', false)' `
        @('--test', 'isolation', 'pool_reuse_clears_commit_rollback_error_and_cancel_scope', '--', '--ignored', '--exact')

    Invoke-KilledMutation `
        'adapters/postgres-event-store/src/journal.rs' `
        "    head_hash: &str,`n) -> Result<Option<Vec<EventEnvelope>>, EventRepositoryError> {`n    let scoped = scope::parts(request.scope());" `
        "    head_hash: &str,`n) -> Result<Option<Vec<EventEnvelope>>, EventRepositoryError> {`n    return Ok(None);`n    #[allow(unreachable_code)] let scoped = scope::parts(request.scope());" `
        @('--test', 'repository_conformance', 'exact_retry_precedes_sequence_and_divergent_retry_fails', '--', '--ignored', '--exact')

    Invoke-KilledMutation `
        'adapters/postgres-event-store/src/journal.rs' `
        'stream_id=$4 FOR UPDATE' `
        'stream_id=$4' `
        @('--test', 'concurrency', 'same_stream_concurrent_append_has_exactly_one_winner', '--', '--ignored', '--exact')

    Invoke-KilledMutation `
        'adapters/postgres-event-store/src/journal.rs' `
        "if store.failpoint_after_evidence() {`n        return Err(EventRepositoryError::Storage);`n    }" `
        "if store.failpoint_after_evidence() {`n        transaction.commit().await.map_err(crate::error::storage)?;`n        return Err(EventRepositoryError::Storage);`n    }" `
        @('--test', 'repository_conformance', 'append_is_atomic_across_event_evidence_and_refs', '--', '--ignored', '--exact')

    Invoke-KilledMutation `
        'core/events/src/integrity.rs' `
        'event_hash(event, previous_hash)' `
        'Ok(event.event_hash.to_string())' `
        @('--test', 'repository_conformance', 'chain_corruption_fails_before_read_return', '--', '--ignored', '--exact')

    Invoke-KilledMutation `
        'adapters/postgres-event-store/src/integrity.rs' `
        'bind_cursor(&payload, &request_scope, &request_stream, head.as_ref())?;' `
        'let _ = (&payload, &request_scope, &request_stream, head.as_ref());' `
        @('--test', 'repository_conformance', 'cursor_tamper_and_scope_or_stream_mismatch_fail_closed', '--', '--ignored', '--exact')

    Invoke-KilledMutation `
        'adapters/postgres-event-store/src/integrity.rs' `
        'authentication_result.map_err(|_| EventRepositoryError::Integrity)?;' `
        'let _ = authentication_result;' `
        @('--test', 'repository_conformance', 'checkpoints_are_authenticated_and_exact_scope', '--', '--ignored', '--exact')
}
finally {
    Pop-Location
}
