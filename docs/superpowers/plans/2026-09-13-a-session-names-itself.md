# A session names itself — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every session attaches to the Runtime under a name it chose, so the board can tell two sessions apart — and the Studio launcher stops pointing every checkout at one hardcoded project.

**Architecture:** Both halves are one defect: a committed file shared by every session, carrying one project's value as a literal. The remedy for each is the same — remove the literal, take the value from the environment, and refuse rather than default when nothing says. `--actor` gains an environment fallback mirroring the one `--token-file` already has twelve lines above it in the same function; `studio-dev.cmd` derives its project from the repository it already locates with `%~dp0..`.

**Tech Stack:** Rust (clap, std::env), Windows batch, JSON config.

**Spec:** https://github.com/stabem/GraphHelm/issues/1058 — including the owner's scope-widening comment that adds the Studio launcher.

## Global Constraints

- **No default identity.** With neither `--actor` nor `GRAPHHELM_ACTOR`, the session is REFUSED with a named diagnostic. Specifically NOT a fallback to `factory-agent`: a default is what produced this issue, and a session whose identity nobody chose must not be able to write.
- **The explicit flag wins over the environment.** A flag is a deliberate act; an environment variable is ambient.
- **`ActorId::parse` still refuses a malformed id whichever door it arrived through** (`apps/cli/src/commands/mcp/mod.rs:62`). A fallback must not create a third path that skips the existing refusals.
- **No literal path to any project** survives in `.claude/studio-dev.cmd`.
- **A preset environment value still wins** in the launcher, so serving another project stays possible deliberately rather than by accident.
- **Comments are claims that get checked.** If a comment says the code does something, it must do exactly that and no more.

---

### Task 1: `--actor` becomes optional, with an environment fallback and a refusal

**Files:**
- Modify: `apps/cli/src/args.rs:158-160` (the `actor` field)
- Modify: `apps/cli/src/commands/mcp/mod.rs:46-70` (the fallback, beside the token's)
- Test: `apps/cli/tests/mcp_cli.rs`

**Interfaces:**
- Produces: `McpArgs.actor: Option<String>`; a resolved actor string inside `build_client`, or a refusal at path `/actor`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn neither_flag_nor_environment_is_refused_and_names_both_doors() {
    let out = mcp_cmd_env(&["--url", "http://127.0.0.1:1"], &[("GRAPHHELM_ACTOR", None)]);
    assert!(!out.ok, "a session with no chosen identity must not attach");
    assert_eq!(out.diagnostics[0].path, "/actor");
    assert!(out.diagnostics[0].message.contains("GRAPHHELM_ACTOR"),
        "the refusal must name the environment door, or the reader only learns about the flag");
}

#[test]
fn the_environment_supplies_the_actor_when_the_flag_is_absent() {
    let out = mcp_cmd_env(&["--url", "http://127.0.0.1:1"], &[("GRAPHHELM_ACTOR", Some("lane-a"))]);
    assert!(out.diagnostics.iter().all(|d| d.path != "/actor"),
        "a name from the environment must satisfy the same requirement the flag satisfies");
}

#[test]
fn the_explicit_flag_wins_over_the_environment() {
    let out = mcp_cmd_env(&["--url", "http://127.0.0.1:1", "--actor", "from-flag"],
                          &[("GRAPHHELM_ACTOR", Some("from-env"))]);
    assert!(out.chosen_actor_was("from-flag"),
        "a flag is a deliberate act; the environment is ambient");
}

#[test]
fn a_malformed_environment_actor_is_refused_exactly_as_a_malformed_flag_is() {
    let bad = "not a wire safe id";
    let by_flag = mcp_cmd_env(&["--url", "http://127.0.0.1:1", "--actor", bad], &[]);
    let by_env  = mcp_cmd_env(&["--url", "http://127.0.0.1:1"], &[("GRAPHHELM_ACTOR", Some(bad))]);
    assert_eq!(by_flag.diagnostics[0].path, by_env.diagnostics[0].path,
        "one door must not be laxer than the other");
    assert!(!by_flag.ok && !by_env.ok);
}
```

`mcp_cmd_env` does not exist — create it beside the existing `mcp_cmd`, taking the same arguments plus a list of environment overrides (`None` meaning "ensure unset"). `chosen_actor_was` is a placeholder for whatever this harness can honestly observe about which name was used; if the process cannot report it without a live server, assert instead that the flag value appears where the harness can see it and say in your report what you asserted and why.

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test -p graphhelm-cli --test mcp_cli -- actor`
Expected: FAIL — `--actor` is currently required by clap, so the no-flag cases cannot even parse.

- [ ] **Step 3: Make the field optional**

```rust
/// The actor every mutation is attributed to (the serve layer's actor id rules).
/// OPTIONAL because one `.mcp.json` is shared by every session in a repository, so a
/// literal here makes every session the same actor (#1058). Falls back to
/// `GRAPHHELM_ACTOR`; absent from both doors is a refusal, never a default.
#[arg(long)]
pub actor: Option<String>,
```

- [ ] **Step 4: Resolve it beside the token's fallback, in the same shape**

```rust
let actor = match &args.actor {
    Some(name) => name.clone(),
    None => std::env::var("GRAPHHELM_ACTOR").map_err(|_| {
        refuse(
            "no actor: supply --actor <id> or the GRAPHHELM_ACTOR environment variable \
             (one .mcp.json is shared by every session, so a literal there makes them all \
             one actor)",
            "/actor",
        )
    })?,
};
```

Then feed `actor` into the existing `ActorId::parse` check and the `ApiClient::new` call, so both doors pass through the refusal that already exists rather than around it.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p graphhelm-cli --test mcp_cli -- actor` then `cargo fmt --all` then `cargo test -p graphhelm-cli`
Expected: PASS. Run `cargo fmt` BEFORE the suite — rustfmt on this repository has joined a continued string literal and baked its indentation in, which the whitespace guard then caught.

- [ ] **Step 6: Commit**

```bash
git add apps/cli/src/args.rs apps/cli/src/commands/mcp/mod.rs apps/cli/tests/mcp_cli.rs
git commit -m "feat(1058): a session names itself, and an unnamed one is refused"
```

---

### Task 2: The two shared files stop carrying one project's value

**Files:**
- Modify: `.mcp.json` (remove the hardcoded actor)
- Modify: `.claude/studio-dev.cmd` (derive the project from the repository the script lives in)
- Test: `apps/cli/tests/mcp_cli.rs` for the config assertion; the launcher is verified by running it

**Interfaces:**
- Consumes: Task 1's `GRAPHHELM_ACTOR` fallback. Without Task 1, removing the flag from `.mcp.json` would refuse every session — so this task must land after it, never before.
- **Scope correction (Codex on PR #1060):** the root `.mcp.json` is a machine-local, untracked file
  (`git ls-tree` shows only the copies under `examples/` and `extensions/`), so the test below is an
  operator's local check, not a repository test; the shipped implementation carries no such test and
  the instruction to each operator is: remove `--actor` from your own `.mcp.json` and set
  `GRAPHHELM_ACTOR` per session.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn the_shared_mcp_config_names_no_actor_because_it_is_shared_by_every_session() {
    let raw = std::fs::read_to_string(repo_root().join(".mcp.json")).expect("read .mcp.json");
    let config: serde_json::Value = serde_json::from_str(&raw).expect(".mcp.json is JSON");
    let args = config["mcpServers"]["graphhelm"]["args"]
        .as_array().expect("graphhelm args");
    assert!(!args.iter().any(|a| a == "--actor"),
        "a shared config that names an actor makes every session that actor (#1058)");
    assert!(args.iter().any(|a| a == "--token-file"),
        "CONTROL: this test can see the args it is asserting about");
}
```

The CONTROL line is not optional. Without it, a typo in the JSON path yields an empty array, `!any` is trivially true, and the test passes while asserting nothing.

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p graphhelm-cli --test mcp_cli -- shared_mcp_config`
Expected: FAIL — `.mcp.json` currently carries `"--actor", "factory-agent"`.

- [ ] **Step 3: Remove the literal from `.mcp.json`**

Delete `"--actor", "factory-agent"` from the `graphhelm` server's args, leaving the rest untouched.

- [ ] **Step 4: Make the launcher serve its own repository**

In `.claude/studio-dev.cmd`, replace the three hardcoded lines:

```bat
set GRAPHHELM_EVENTS=F:\github\Dale\dale-api-base\.graphhelm\events
set GRAPHHELM_RUNTIME_URL=http://127.0.0.1:8791
set GRAPHHELM_PROJECT=dale-api-base
```

with a derivation from the repository the script already locates. The script ends with `cd /d "%~dp0..\apps\studio"`, so `%~dp0..` is the repository root. Requirements, in this order:

- If `GRAPHHELM_EVENTS` is ALREADY SET, leave it and everything derived from it alone — serving another project stays possible deliberately.
- Otherwise set it to `%~dp0..\.graphhelm\events`.
- Derive `GRAPHHELM_PROJECT` from that directory's repository folder name rather than spelling it. A name that can disagree with the directory it labels is a second source of truth, and this file already proved that by disagreeing.
- Leave `GRAPHHELM_RUNTIME_URL` as it is — it is a local port, not a project value.
- Keep the per-launch session nonce exactly as it is. It is minted per launch precisely because this file is committed.

- [ ] **Step 5: Verify the launcher by running it, not by reading it**

Run it from a checkout of this repository with `GRAPHHELM_EVENTS` unset, and confirm the echoed auto-connect line appears and the Studio serves THIS repository's events. Then run it again with `GRAPHHELM_EVENTS` preset to another path and confirm the preset survives. Put both observations in your report. A batch script has no test suite here, so the run IS the evidence — and a launcher verified only by reading is how the wrong path survived this long.

- [ ] **Step 6: Run the Rust test and commit**

Run: `cargo test -p graphhelm-cli --test mcp_cli -- shared_mcp_config`
Expected: PASS.

```bash
git add .mcp.json .claude/studio-dev.cmd apps/cli/tests/mcp_cli.rs
git commit -m "feat(1058): the shared config names no actor, and the launcher serves its own repository"
```

---

## Not in this plan

**Who sets `GRAPHHELM_ACTOR` for a Claude session, and to what.** That is a harness question, not a repository one: this plan makes a unique name possible and refuses an unnamed session, which is the part that lives in code. Choosing the value — a session id, a lane letter, a human name — is the operator's, and a repository that picked for them would be the same defect one level up.

**What a session is running (#1054), where it works (#1051), and how many agents may work one node (#1049).** This issue answers WHO, and only who.
