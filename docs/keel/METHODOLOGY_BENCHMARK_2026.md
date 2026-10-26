# Subscription methodology pilot

This instrument compares **adapted protocol prompts**, not complete native framework
installations. It measures no gain until real frozen attempts have independent outcomes.
It does not rank adoption or claim that the named projects are the most used methods.

The operator is the caller of `tools/token-bench/methodology.py`. Its commands are
`qualify`, `freeze`, `run` and `report`. Existing retrieval benchmarks remain separate.

## Frozen design

Six small, standard-library Python tasks cover CSV compatibility, bounded cursor
pagination, recursive redaction, authorization, atomic persistence and concurrent
idempotency. Each has original public checks, a withheld acceptance oracle and a known
correction. Qualification requires public-green/hidden-red on the broken parent and
both green on the correction. This is instrument calibration, not a measured coding gain.

Seven profiles are direct implementation, TDD, Spec Kit, OpenSpec, BMAD, Superpowers and
JPD with Keel. Branded profiles cite immutable primary-source revisions in
`methodology_profiles.json`. All profiles receive the same task, tools and bounds.
Prompts intentionally adapt workflows to a single session: native hooks, specialist
agents, Runtime tools and framework installations are outside this treatment.

The initial cohort has 84 cells: six tasks × seven profiles × two CLIs × one repeat.
Task order is seeded and profile order rotates by task. The two CLI strata remain
separate, with explicit model requests and observed model identities retained where
the CLI exposes them. One repeat supports a descriptive pilot, not a general causal
or statistical superiority claim. Small tasks can produce a ceiling effect.

## Operator flow

Use Python and explicit **native executable** paths. Freeze refuses batch and PowerShell
shims. All paths below are examples; put private evidence outside the source checkout.

```powershell
python tools/token-bench/methodology.py qualify --output D:/gh/my-lane/calibration.json
python tools/token-bench/methodology.py freeze --codex C:/path/codex.exe --claude C:/path/claude.exe --codex-model PINNED_CODEX_MODEL --claude-model PINNED_CLAUDE_MODEL --output D:/gh/my-lane/manifest.json
python tools/token-bench/methodology.py run --manifest D:/gh/my-lane/manifest.json --attempt 000 --output D:/gh/my-lane/results
python tools/token-bench/methodology.py report --manifest D:/gh/my-lane/manifest.json --output D:/gh/my-lane/results
```

Run cells serially in frozen order. Never retry an existing ID: its reservation records
that it was started even if the process crashed before publishing the outcome. Stop
the cohort on subscription exhaustion, authentication failure or uncertain child
cleanup. There is no automatic API, model or provider fallback. Record the remaining
cells as not run through the report. A new trial requires a separately frozen cohort;
never replace a failure or silently tune prompts after inspecting outcomes.

## Evidence and limits

Each attempt gets a fresh directory containing only the broken source and public test.
After the model exits, the evaluator copies only the authored module to another fresh
directory and restores the original public checks before adding hidden checks.
An unpredictable completion witness is written only after each check finishes; an
early zero exit is retained as a zero exit but cannot count as accepted evidence.
Known corrections and hidden checks are never supplied to the subject. Directory
separation and a prompt are not proof of adversarial OS-enforced oracle secrecy.
The corpus is executable, reviewed repository code. Do not qualify untrusted case
files: evaluation is not a sandbox for hostile Python. Native process-tree cleanup
uses bounded taskkill on Windows; absence of all descendants remains unobserved.

Child environments omit provider API keys and endpoint overrides. Both CLIs must
report subscription authentication before freezing and before each attempt. Global
credentials and settings stay untouched. Codex uses workspace-write without a dangerous
approval bypass; Claude uses safe mode and noninteractive permission denial. These
controls may deny a tool: preserve that failure rather than weakening host security.
Managed policy and residual host context can remain a confound; clean-room isolation
and compliance with every requested process step are **UNOBSERVED**.

The report retains all attempted and missing cells. Correctness requires successful
CLI completion plus the restored regression checks and hidden acceptance oracle.
Tokens, cache categories, elapsed agent time and observed model identities are separate
evidence. Missing usage is unknown, never free. Subscription billed USD is `null`:
CLI price estimates do not establish a subscription bill or dollar savings.
Full traces and authored source remain private evidence; inspect them before sharing.

No automatic winner is emitted. Compare acceptance and cost together within each
CLI/model stratum. An incomplete cohort, unknown model identity, missing usage,
unobserved adherence or native-framework fidelity limits the claim explicitly.
Adding harder real-project tasks, repeats and native-framework treatments requires
a new preregistration, not retroactive edits to this pilot.

## Native token accounting

For fresh, non-resumed, single-shot Claude invocations, `counts` comes from the final
result's validated `modelUsage` totals across query-pipeline models. `mainLoopCounts`
retains the separate final `usage` scope. Auxiliary calls may make pipeline counts
larger than main-loop counts. Coding-model identity still comes from assistant records;
an auxiliary model in accounting does not become the coding model.

Assistant-message output usage is a message-start placeholder, not the final generated
output count. The deduplicated transcript remains in `transcriptCounts` for diagnosis;
it is never used as a fallback for missing native totals. See the official
[Claude usage documentation](https://code.claude.com/docs/en/agent-sdk/cost-tracking)
and [result-message scopes](https://code.claude.com/docs/en/agent-sdk/typescript).

Missing or duplicate final results, malformed JSON or native accounting records, missing categories, negative,
noninteger or boolean counters, and pipeline totals below main-loop totals leave
`counts` unknown with an explicit `accountingStatus`. Codex retains validated sums
from completed turns; cached input is a subset of its input count. Observed accounting
does not establish billed USD or an otherwise missing model identity.
Malformed assistant-message diagnostics are retained separately; a diagnostic subtotal
does not override otherwise valid native accounting records.

Historical receipts and manifests stay immutable. To correct a historical analysis,
read its saved native stdout through the current `usage` parser and write a separate
derived artifact containing the original manifest/receipt hashes and the new parser's
revision and digest. Do not rerun models, overwrite old receipts, relabel their first
outcomes, or present the new parser as the parser used during the original collection.

## Real-project benchmark

`tools/token-bench/real_methodology.py` is a separate, offline instrument for a small
exploratory cohort of real project defects. It does not replace this subscription pilot
or reuse its toy cases. A private registry names exactly three frozen task snapshots,
their production scope and import closure, the original public test, and a private hidden
oracle. Snapshot roots and oracle roots are external to the checkout; the registry is
hashed into the frozen manifest.

Qualification must show the parent snapshot's public command green and hidden oracle red,
then the known-fix snapshot green on both. A weakened or missing oracle fails qualification.
Each planned cell is one fresh task directory, one of seven profiles, and one native CLI;
the seeded order is 3 tasks × 7 profiles × 2 CLIs × 1 repeat = 42 cells. The public command
is always `[sys.executable, "test_public.py"]`. The evaluator restores the original public
test and copies only declared production scope from the candidate into a clean parent
template, so changing tests cannot make a result pass. Hidden oracle files and known fixes
never enter the agent directory.

The runner reserves an attempt ID before invoking a child and never retries or overwrites
that ID. Provider authentication, quota, cleanup, malformed output, missing usage and
identity drift leave the cell failed or incomplete; they do not become a passing result.
The report preserves `passed`, `failed`, `incomplete` and `not-run` separately and emits
no winner. The cohort is neutral and exploratory: it cannot establish statistical or
comparative superiority.

The real-project manifest freezes every snapshot, support, public-test and oracle file,
the parser revision, native Python and CLI identities, exact argv, and relevant environment.
Each attempt rechecks those hashes, first-party authentication and version, frozen order,
cohort lock and provider-stop marker before launching. A reserved cell always receives a
receipt, including setup, cleanup, output-capture or evaluation failure; raw candidate,
stdout and stderr evidence remain in the private results directory.

The agent may edit only declared production scope and original public-test files. The
immutable root `test_public.py` wrapper is copied from support files and runs the real
nested public test. Evaluation restores original public tests and support files into a
clean parent template, overlays only declared production files, and excludes caches and
unrelated candidate files. Trusted wrappers must provide completion evidence (for example,
JUnit or Vitest JSON completion); exit code zero alone is not acceptance. Qualification
requires the broken parent to fail for the intended assertion, not an import, tool,
timeout or cleanup error.

Native Claude accounting remains sourced from final `result.modelUsage` totals. The
separate final `usage` scope and transcript diagnostics are retained. Missing native usage
is `unknown`, never free. An empty observed-model list is `unobserved`; a non-empty list
that differs from the frozen requested model stops the cohort.

Pinned external native binaries and dependency directories are fingerprinted before
freeze and before each cell. Directory fingerprints are bounded, deterministic hashes of
sorted relative paths, lengths and content hashes; declared cache paths are the only
ignored entries, and links/reparse points are rejected unless they resolve to the exact
declared candidate target. This validation costs O(external dependency bytes) and is
control/setup time, not agent time; dependencies are not copied into each candidate.

## Native framework workflows

The native workflow instrument is `tools/token-bench/native_methodology.py`. A versioned
registry names each framework's real phases and native stage names. The common records are
only evidence boundaries: a Spec Kit workflow may use specify/plan/tasks/converge, an
OpenSpec workflow may use explore/propose/apply/verify/archive, and another framework may
have a different sequence. The instrument does not turn these stages into one invented
uniform process.

Each phase receives a fresh directory, a new stream, an immutable plan, and a phase attempt
record. Retries link to the earlier attempt; they never replace it. Raw native results and
their SHA-256 hashes are retained, and a later phase must carry the predecessor hashes.
The accepted endpoint is an independently reviewed patch in local Git. It is not a public
GitHub merge or a deployment.

Native CLI configuration keeps project instructions, installed skills, plugins, and native
session logs enabled. Provider keys and endpoint overrides are still removed from child
environments. Authentication and quota are checked before every phase. Cleanup, native
usage, phase costs (setup, planner, author, independent review, fix, verify, archive), and
the declared maximum fix budget are separate evidence. Missing or uncertain evidence leaves
the phase incomplete and preserves the prior phase result.

Fidelity requires an observed native stream, CLI invocation, skill initialization/rollout,
or native artifact with a hash. Model prose claiming that a framework ran is not a witness.
Owned child sessions must be enumerated and reconciled with unique usage records; missing,
duplicate, overlapping, or unowned records keep totals unknown. Task context is limited to
public schema fields and never carries provider credentials, hidden oracles, or private
source paths.
