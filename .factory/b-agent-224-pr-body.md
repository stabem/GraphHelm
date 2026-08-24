## What and why

The development-contracts package gains three **data-only** entry skills, the three hand-written host views, a README, and the hostile-package guards that were written before any of it.

Closes #224

## The authority is the design

This package's grant is the whole argument:

```
filesystem  package: read          workspaceArtifacts: proposal-write
network     external: false        loopbackRuntimeApi: true
runtime     read: true             mutations: []
secrets     artifactValues: false  tokenFile: false
```

So the three skills declare `artifact.propose` and never `artifact.local.write`, every surface they name is read-only, and none can mutate the Runtime. `memory-curator` in particular emits advisory candidates only — *a curator that could accept its own proposals is not a curator, it is an author with extra steps.*

A skill that enforced its own contract or published its own memory would be a second authority whose rules live in prose, and prose cannot be validated, versioned, or refused. That is why the boundary sits in the manifest: a contribution requesting more than the grant is refused at package validation, before any host sees it.

## Guards first, and each under its OWN code

Fourteen cases in `apps/cli/tests/development_plugin.rs`. Each copies the shipped package into a `TempDir`, changes **one** thing, and asserts the refusal arrives under the specific diagnostic code — not merely that something was refused.

| case | code | why it is one mutation |
|---|---|---|
| positive control: the bundle validates and declares its three skills and three host views | — | runs first by intent: every negative below is satisfied by a validator that refuses everything |
| an effect beyond the package grant | `GHEX018_AUTHORITY_ESCALATION` | `runtime.mutate`, which the grant does not carry |
| an effect without its declared permission | `GHEX018_AUTHORITY_ESCALATION` | `host.discover` is *inside* the grant — this is taking authority without declaring it |
| a skill that writes instead of proposing | `GHEX018_AUTHORITY_ESCALATION` | the enforced half of the self-publication threat |
| **eight cells**, one per cross-matched fact | `GHEX013_HOST` | each moves that file's declared digest, so identity is the only thing wrong |
| moving a declared file's bytes | `GHEX005_DIGEST` | one byte appended |
| activation | — | declared pending on #212, see below |

**Why "refused" is not the bar.** A hostile fixture that reddens on a neighbouring rule reads as coverage of a threat nobody guards. That discipline caught two of my own defects in one sitting:

1. The host-mismatch case first changed the manifest's `name`, which also changed its bytes. The validator answered `GHEX005_DIGEST` and never reached the host cross-match — a refusal, a green test, and no coverage at all. Fixed by moving the declared digest with the content.
2. The three skills were declared with `runtime.read` as a *permission* and not as an *effect*. The package stopped validating, which took the positive control down with it — exactly its job — and the host case then failed loudly under the wrong code rather than passing against a package broken in five other ways.

## L's two items, closed in the union round

**F1 — the cross-match is a property of many facts and the suite exercised one.** It changed `name` in the Claude manifest and called the property covered; a regression that stopped comparing `version` in the Codex manifest would have reddened nothing. It is this suite's own rule — each case red under its OWN code — one floor up: each **fact** gets its own cell.

Counted from `valid_mcp_registration` and the two identity comparisons rather than taken from the review note, which said seven. There are **five identity facts** — `name` and `version` in the Claude manifest, `id`, `name` and `version` in the Codex one — plus a closed MCP shape whose load-bearing conditions are the server name, the pinned command, and the loopback URL. Eight cells.

**The loopback cell is the one that matters most.** That registration hands the Runtime a token by file path. Point it at a host that is not loopback and the token is presented to whatever answers there — and it is the only file in this bundle that could contradict the package's own `network.external: false` without declaring a single extra permission.

Declared, so the coverage claim is honest: the argument vector's arity and order, and the exactly-one-key conditions, are **not** decomposed. They fail as a unit, and a cell per permutation would tabulate the validator's implementation rather than the property.

**F2 — the host views said "derived" and nothing derives them.** The word was copied from the sibling package; these three files are written by hand. Corrected, and the consequence is the part worth stating: each is **another producer of this package's identity**, the same name and version living in three files that no mechanism keeps in step. The only thing holding them together is the cross-match — which makes F1's decomposition more necessary, not less.

## Two of the six shapes are absent, and both say so in the file

**Self-publication at the manifest level has no emitter.** `/spec/contracts/publication` is declared `governor-only` by every package and read by nothing. Measured across `core/` by the JSON keys the validator indexes by, with controls in the same run:

```
artifactFlowFormat 1   entryFamilies 1   formatVersion 22    <- controls
publication 0   activation 0   composition 0   missingCapabilityResult 0   hostViews 0
```

A package declaring `"publication": "self"` validates clean today. Filed as **#285**. The effect-level threat *is* enforced and is covered here — and **that does not cover this**: what a contribution may DO is checked, what the package SAYS IT IS is not.

**Private import needs a graph contribution this package does not have.** `extension://` references are read from `/spec/nodes/*/agent/ref` and `/spec/policies`; this package declares only fixtures, schemas, policies, skills and host adapters. `apps/cli/tests/extension_cli.rs` already drives `extension://another-package/defect-hunter` against a graph-bearing package. Repeating it here would duplicate an **oracle** rather than a mechanism, and a duplicated oracle diverges in silence and quietly changes what "passed" means.

## The defect this change introduced, and the amendment that fixed it

The three `SKILL.md` files are digest-declared and nothing protected them from end-of-line conversion. Measured with `git check-attr` rather than by reading the rules — the package's `.json` resolved to `eol: lf`, its markdown to `eol: unspecified` — and the consequence measured rather than reasoned:

```
declared in the manifest : sha256:54ff82f4...
file as LF, today        : sha256:54ff82f4...   matches
file as CRLF             : sha256:e5f32b84...   does not
```

**A Windows clone rewrites all three and the package fails `GHEX005_DIGEST` for every one** — the class #275 fixed for another lane. `.gitattributes` was outside this issue's nine-file scope, so it was **not** touched: the work stopped and asked, and **scope amendment 1** added the file. The green I would otherwise have reported was true about my working tree and false about the artifact.

Verified after applying, with the same instrument: all five files resolve to `eol: lf`, the stored blobs contain zero CRLF pairs, and each blob's digest equals the manifest's.

## Activation, declared pending

Activation depends on **#212**, which is open. Nothing here activates the bundle. The case that would prove activation behaviour is present and named, stating what it will assert and what unblocks it — deliberately **not** `#[ignore]`, because a skipped test and a passing test are the same green row in a summary, and the thing most likely to be forgotten is the case nobody sees fail.

## Validation evidence

Base is `origin/main` at `9135d4f` (#284 merged first, by publication order in the shared-manifest queue), merged in and unioned. ED-18 was re-run on that result rather than on the older base.

```
cargo check --workspace --all-targets --locked            exit 0
cargo clippy -p graphhelm-cli    --all-targets -D warnings  0 errors
cargo clippy -p graphhelm-schema --all-targets -D warnings  0 errors

development_plugin             14 passed      development_package_inventory   1 passed
development_contract_schemas   27 passed      extension_cli                  31 passed
graphhelm-schema --lib         51 passed                       total 124 passed, 0 failed

extension validate -> ok:true, 43 contributions, 0 diagnostics
```

The sweep fingerprints the tree before and after and reports contamination itself rather than relying on me to remember not to edit mid-run; this run reported the tree unmoved.

Clippy found one defect of mine in the test file (`iter().any()` where `contains()` reads better) — fixed. The clippy red currently on `origin/main` at `core/policy/tests/determinism.rs` is a different crate and does not appear here.

## Manifest

**37 → 43** after the union with #284, which published first and therefore keeps the prefix. Six new entries — three skills and three host adapters — each declaring its own file with a digest computed from disk after every file was final. The inventory guard from #253 is green, which is what proves the two halves moved together.

The resolution was rebuilt from main's blob rather than spliced from conflict hunks, and then checked by a command that is **not** the tool that wrote it — the tool's own `--check` would compare a derivation against its own output:

```
main is an UNBROKEN PREFIX of the result : true
lost from main                          : []
appended after the prefix               : the six from this branch
digests of main's entries unchanged     : true
```

Prefix rather than subset is the stronger claim and the one the queue rule actually makes: a subset check passes for a resolution that reordered main's entries, and a reorder is invisible to a set comparison while changing what every position-dependent reader sees.

The union tool also gained a fix in this round, under scope amendment 2. Its membership block counted refusal **codes** while the block two lines above counted manifest **entries**, in nearly the same words — so on a manifest-only branch it printed `added by this branch: []`, which is true and reads as "this branch adds nothing". The evidence that this misleads is that it misled its author a minute after he wrote it. Every row in that block now names its subject.

## Security review

No new authority and no code that runs. Three markdown documents, three host manifests, a README, six manifest entries, and a test. The host views carry `host.discover` and `package.read` only, cross-match this package's identity and version, and reference the Runtime through the public CLI with the token supplied by **file path rather than value** — the package never sees the secret.

## Rollback

Revert the commits. The package returns to 35 contributions and the host views disappear; nothing else depends on them. Per #224, rollback removes only the host views -- which this package writes by hand rather than derives, as F2 established -- and never user-authored artifacts.
