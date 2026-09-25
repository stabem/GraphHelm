# pathogens: the staleness population includes crates the binary does not depend on, so the canary's nonce makes every reused binary read as stale

## The instrument compares the binary against sources it does not contain

`tools/pathogens/src/subject.rs::measurable_binary` refuses to measure when any workspace source is newer than `target/debug/graphhelm`. The population it compares against is **every `src/*.rs` in the entire workspace**:

```rust
fn workspace_sources(root: &Path, into: &mut Vec<PathBuf>) {
    ...
    } else if path.extension().is_some_and(|extension| extension == "rs")
        && path.components().any(|component| component.as_os_str() == "src")
```

That includes crates the binary does not depend on. Concretely, it includes `tools/ci-canary/src/nonce.rs`, and `ci-canary` is **not** a dependency of `apps/cli` — its only dependencies are `sha2` and `hex`.

**The gate's canary rewrites `nonce.rs` at the start of every run.** That is how it proves the build is real. So on every run there is a source, outside the binary's dependency closure, whose mtime is later than any binary not rebuilt after that moment.

## Why it has been invisible until now

In a cold run every artifact is rebuilt *after* the canary writes the nonce, so the binary is newer and the comparison passes. The population has always been wrong; the build order hid it.

A warm run with content-addressed artifact reuse (#904) does not rebuild `graphhelm.exe` — its inputs are unchanged — so the reused binary keeps the previous run's mtime and the instrument refuses:

```
with a binary newer than every source, the instrument must measure, not refuse:
Err(Stale { path: "E:/opus-warmpair/debug/graphhelm.exe",
            newer_source: ".../tools/ci-canary/src/nonce.rs" })
```

`pathogens/subject_refusals` is the only target that failed in that run (`workspace tests`, exit 101).

## This is the same class the code already fixed once

The comment at that line records an earlier narrowing:

> Only `src/` is embodied by the binary. The first draft collected every `.rs` in the workspace, which meant writing a TEST aged the instrument against itself and it refused forever — a guard that always refuses.

`src/` of a crate the binary does not link is the same mistake one level further in. **The predicate — *binary newer than every source* — is a proxy for the property we actually want: *the binary embodies today's sources*.** The proxy breaks when a file outside the closure moves; the property is untouched.

## The fix

Narrow the population to the binary's **workspace dependency closure**: start at `apps/cli`, follow `path = "..."` dependencies transitively, and collect `src/` from those crates only. `ci-canary` is unreachable and drops out. `pathogens` itself IS reachable (`apps/cli/Cargo.toml` depends on it), so the existing staleness phase keeps its witness and its power.

Do **not** loosen or except the check. It exists because sixty green stages once measured a stale binary against the wrong tree; a guard that stops refusing buys that incident back.

## Acceptance

- The population excludes `tools/ci-canary/src/nonce.rs` and includes a crate the binary does depend on — asserted directly, not inferred.
- The existing staleness phase still reddens when a source **inside** the closure is aged: a guard that only passes because the defect is absent proves nothing.
- The change is proven by the unmodified gate; an instrument cannot certify its own amendment.

## Consequence if fixed

This is the single remaining red on the warm-build path. With it closed, warm gating becomes usable — `buildPassSecs` measured 172.234 cold against 1.359 warm, 233 proven reuses, zero unproven and zero contaminated. Twenty-five of the twenty-nine open PRs currently have no gate receipt for the head they present; at cold cost that is roughly eleven hours of gate time.



## Acceptance interface (bench addendum)

Expose the population the instrument compares against as `pub fn embodied_sources() -> Vec<PathBuf>` in `tools/pathogens/src/subject.rs`, so a test can assert what it contains.
