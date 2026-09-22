# Reversible host adoption rehearsal

This is a separate, observer-enabled rehearsal recipe, not an offline gate result or a completed
real-host transcript. The shipped CLI cannot establish trusted host-observation custody. Its
production verification result remains `installed_unverified` with `observer_missing` even when
a supplied ActivationReceipt has every field. A host exit code, an agent statement, a screenshot
alone, or a manually authored receipt cannot change that result. Successful custody fixtures exist
only inside adapter unit tests compiled with `cfg(test)` and are explicitly marked `fixtureOnly`.
Production journal readers also refuse a persisted `verified` claim, even with a recomputed
checksum. A checksum protects against accidental corruption; it cannot supply trusted custody.
Copying a fixture journal into a production build therefore cannot authorize apply, recover,
restore, or verification as a verified transaction.

## Disposable scope

Use a disposable project and a disposable host profile. Record the GraphHelm commit, OS, host
name/version, profile identity, project/home bindings, accepted plan digest, transaction ID, package
IDs/versions/digests, and the actual observation capabilities before changing anything. Do not
migrate a real home, bypass managed policy, or call a paid model without separate scope and budget
authorization. Unsupported policy discovery or host containment is a refusal, not permission to
use another mutation mechanism. Current automatic host-process containment is Windows-only.

Seed a factory instruction, one personal preference, one protected deny rule, two compatible
skills, and one conflicting skill. Record their initial bytes and access restrictions. Stop every
affected host session. Keep the recovery directory private and outside project/home roots.

## Reviewed file-state journey

The examples use operator-selected paths; substitute only disposable directories. PowerShell:

```powershell
$project = 'D:/_agent-scratch/graphhelm/adoption-rehearsal/project'
$profile = 'D:/_agent-scratch/graphhelm/adoption-rehearsal/profile'
$state = 'D:/_agent-scratch/graphhelm/adoption-rehearsal/state'
$plan = 'D:/_agent-scratch/graphhelm/adoption-rehearsal/reviewed-plan.json'
graphhelm setup --project $project --home $profile --dry-run --json
graphhelm setup --project $project --home $profile --plan $plan
graphhelm setup --project $project --home $profile --state-root $state --apply $plan --accept 'sha256:<exact-reviewed-digest>'
```

The discovery preview is conservative: unknown text is `unresolved`, and fixed-file inventory is
not exhaustive skill-tree discovery or proof of effective managed policy. Resolve the supported
operations in a private AdoptionPlan, with explicit user scope for user configuration. Review the
actual keep/disable/replace/unresolved decisions, protected rules, exact operation bytes, package
pins and digest. `--plan` previews an already reviewed plan; it does not invent approval or convert
an unresolved discovery report into an executable plan. Acceptance is the single explicit
`--accept` digest; pipes never confirm automatically. The CLI repeats all mutation checks.

Check the verified backup and durable journal before treating file installation as complete.
Repeat the identical accepted apply and confirm the original backup ID is unchanged. Record
`installed_unverified`; this is evidence about files and activation pointers only.

## Fresh-session observation

Open a new host session with the exact supported loading arguments returned in the receipt.
Record independently observed session identity and start time after installation, host version,
effective configuration, loaded package digests, loaded skills, effective instructions and the
absence of the old methodology. Confirm the personal preference and deny rule still apply.

Execute the GraphHelm MCP server's read-only `list` tool (`GET /v1/executions`) from that session.
Correlate its request ID, session, environment, timestamp and response digest with an independent
Runtime observation. Capture a reversible sample task under the new method using the promised
outcome observer. Preserve evidence digests and private custody; never include tokens or raw
sensitive captures in the receipt. Missing observation of any required fact is `observer_missing`.

For the sample, change one disposable greeting fixture from `hello` to `hello fixture`. State its
observable output and the files/settings that must remain unchanged before editing. Run the
fixture's local test, inspect the diff and unchanged deny/preference/skill state, then reverse the
sample edit. Record the observed output and any retry linked to its first attempt. If the loaded
method cannot be independently observed guiding this task, do not infer adoption from the passing
greeting test.

ActivationReceipt validates transaction/plan, host/version, exact configuration and package
digests, environment, fresh session, observer/custody identity, MCP correlation, methodology
evidence and timestamp ordering. Receipt integrity is not observer authentication. This build has
no real observer adapter and cannot mint custody, so the following is a refusal/missing-observer
check, not a route to a production `verified` result:

```powershell
graphhelm setup --project $project --home $profile --state-root $state --plan $plan --verify 'D:/_agent-scratch/graphhelm/adoption-rehearsal/activation-receipt.json'
```

Tamper with the receipt digest, omit MCP evidence, reuse the old session, change a package or
configuration byte, and re-enable the old methodology in separate trials. Each must block
verification. A structurally valid self-assertion must still report `observer_missing`. Verification
re-reads the active journal and current names/bytes under retained project/home/state roots and
locks before persisting its result. It is point-in-time evidence, not a promise against future edits.

## Restore and reopen

Add an unrelated user key, preview `graphhelm restore --state-root $state --backup original --json`,
save only `data.plan` privately, then apply that plan with `--apply <restore-plan.json> --accept
<its-exact-digest>`. Restore runs without a host process, model or network. Confirm originals return
and the added key survives; same-key or inseparable prose changes must conflict rather than vanish.
Reopen a fresh host session after restore and independently check the old configuration again.
Without that observer, report file restoration only and keep actual host behavior unproven.

## Retained guards and portability limits

Each modified file can leave an owner-only sibling directory named `.graphhelm-adoption-<uuid>`.
These project-local guards retain candidate/displaced source bytes and access metadata for safe
recovery. They are sensitive local recovery material, even though normal receipts contain only
digests. Do not add them to Git or share them in archives; review `git status` and archive inputs
explicitly. If accidentally staged, unstage them before committing. If already shared, restrict
the archive/access and handle any exposed secrets through the owner's incident procedure. Do not
delete a guard based on its name alone: preserve it until the owning transaction is reconciled.

Linux publication uses atomic exchange. Windows uses anchored, journaled, no-replace renames;
the destination can be absent between steps while the host is quiescent. The number of protocol
steps is bounded, but crashes, scheduling and filesystem calls mean there is no wall-clock bound
on that absence. Recovery preserves an unrelated file created during the interval.

The journal schema permits at most 16 guard records **per entry**. This does not bound total disk
usage, the number of transactions, or guards created before their intent became durable. There
is no automatic guard sweeper. Keep the disposable project, profile and private state together
until recovery and restore evidence have been checked, then remove only that owned rehearsal.
