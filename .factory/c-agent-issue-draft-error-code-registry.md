> **PROVENANCE: this draft became #131 — *GHCLI error codes have no registry*.**
> Established by title match against `gh issue list --state all`, not from memory. Written here
> because the derived issue carries the content and never the source's name: provenance is the
> strong relation and the one grep cannot see, so only the author can record it.

GHCLI error codes have no registry, so a number can be allocated twice and nothing notices — 009 already is

**The defect is the registry's absence. The collision is its sighting.** Filed after allocating a
new code for #96 required grepping the entire tree to establish, with any confidence, which integers
were free.

## The sighting

Two different codes share 009, both reaching the wire:

| code | defined at |
|---|---|
| `GHCLI009_GATEWAY_INVALID` | `apps/cli/src/commands/gateway/mod.rs:40` |
| `GHCLI009_SERVE_AUDIT_FAILED` | `apps/cli/src/commands/serve/mod.rs:70` |

**Not a live operator defect today**, and that is stated deliberately rather than left to be
discovered: the full code strings differ, so any consumer matching the whole string is unaffected.
The number is decorative for matching — but it is not decorative for *humans*, who read it as an
identifier, sort by it, and allocate from it.

## The defect

There is no list. Nothing enumerates the allocated codes, nothing rejects a duplicate, and nothing
told the second author of 009 that the number was taken. **The collision was silent by
construction**, and the next one will be too.

Allocating `GHCLI019` for #96 meant:

```
grep -rhoE "GHCLI[0-9]{3}_[A-Z_]+" --include=*.rs apps/ core/ | sort -u
```

— a whole-tree grep whose completeness rests on every code being a literal in a `.rs` file under
those two directories. That happens to hold today. It is not a property anything enforces, and a
code assembled at runtime, defined in a doc, or added under a third directory would be invisible to
it. **An allocation procedure whose correctness depends on a convention nobody checks is the same
shape as a coverage list nobody derives (#98): the intent and the reality are two facts, and nothing
reconciles them.**

## Suggested fix

A single enumeration the codes are defined in or checked against — the cheap version being one
module that declares them all, with the existing constants referring to it, so a duplicate becomes a
compile error rather than a discovery. A test that asserts uniqueness over that list is the same
idea at test-time if the compile-time version is too invasive.

**Whether 009 itself gets renamed is a separate and smaller question**, and it is not free: both
codes are wire-visible, so renaming either is a contract change with consumers to consider. The
registry is worth having regardless of what happens to 009 — and with a registry, the decision about
009 becomes visible instead of hypothetical.

## Not fixed in #96 because

#96 is an operator-contract change on the start/resume path. Renaming a wire-visible code or adding
a cross-cutting registry is a different blast radius, and bundling them would make the contract
change unrevertable on its own.

## Related

- #98 — the gate's coverage allowlist. Same family: a hand-maintained list whose omissions are
  silent.
- #96 — the allocation that surfaced this.
