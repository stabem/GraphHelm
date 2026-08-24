#!/usr/bin/env python3
"""Resolve - or CHECK - the shared development-contracts conflicts by UNION, for any lane.

    python .factory/bin/union-shared-manifest.py <main-sha>            # resolve: WRITES files
    python .factory/bin/union-shared-manifest.py <main-sha> --check    # compare: writes NOTHING

USE --check AFTER RESOLVING BY HAND. That is the mode this tool was adopted under: the author
resolves the merge as they know how, the tool re-derives the union independently, and the two
are compared. A disagreement means one of them is wrong, and the report says which entries.

Without --check that comparison CANNOT HAPPEN, and it fails silently: the resolve mode
overwrites the hand resolution, so the tool always agrees with a file it just wrote. An
instrument that overwrites its own subject reports agreement it never tested.

--check compares MEANING, not bytes: contribution ids and their digests, enum members and
their order, the Rust arms. Whitespace differs harmlessly between a hand resolution and this
script's output, and a byte comparison would drown a real divergence in formatting noise.

Run it while the merge is conflicted, from the worktree root. It resolves three files and prints a
membership check. It does not commit and it does not stage: read the output first.

WHY THIS EXISTS (B, 2026-08-24, after shipping the defect it prevents)
---------------------------------------------------------------------
Every conflict in these three files is two lanes APPENDING to the same list. That makes the
obvious tools wrong in ways that pass every downstream check:

  * `--ours` / `--theirs` are FILE policies applied to a VALUE problem. Either one deletes the
    other lane's entries wholesale. The result parses, validates against the schema, and the
    set-equality guard stays GREEN -- because both halves lose the same entries together, so the
    Rust type and the schema still agree, on a smaller set. A correct-looking clean merge.

  * Splicing the conflict hunks by hand is worse than it looks. I did it, and it shipped a
    DUPLICATE: `code_rule_waiver_invalid` stood on both sides of one hunk and the concatenation
    kept both. Still valid JSON, still a valid schema, 19 codes silently became 20. Only counting
    the members caught it. A duplicate in a closed vocabulary is not cosmetic -- it makes the
    set-equality guard's subject ambiguous.

So this rebuilds each file from main's blob plus the branch's own additions: a judgement traded
for a derivation. Rebuilding from the source leaves no hunks to resolve wrongly.

WHAT IT DOES NOT DO
-------------------
It does not replace J's formal `## Merge membership check` on the PR. The check printed here is
the author's own pass, and the author's pass is the one that shares the author's blind spots. Two
runs of the same method find different faults in each other.
"""

import collections
import hashlib
import json
import pathlib
import re
import subprocess
import sys

PKG = pathlib.Path("extensions/builtin/graphhelm-development-contracts")
MANIFEST = PKG / "extension.json"
SCHEMA = PKG / "schemas/development-envelope.schema.json"
VOCAB = pathlib.Path("core/protocols/src/development.rs")


def blob(ref, path):
    r = subprocess.run(["git", "show", f"{ref}:{path}"], capture_output=True)
    if r.returncode != 0:
        sys.exit(f"cannot read {path} at {ref} -- is {ref} a sha this worktree has?")
    return r.stdout


def refusal_arms(source):
    """The wire names of the refusal-code vocabulary ALONE.

    Not every `=> "wire"` in the file: that module declares three vocabularies and the loose
    pattern matches 27 arms across all of them, which would pass a schema code that exists only as
    some other vocabulary's name.
    """
    start = source.index("DevelopmentRefusalCode")
    block = source[start : source.index("\n    }\n", start)]
    return re.findall(r'=> "([a-z_]+)"', block)


def main(main_sha, check_only):
    # --- manifest: main's entries, then whatever this branch added, digests recomputed from disk
    theirs = json.loads(blob(main_sha, MANIFEST.as_posix()).decode("utf-8"))
    ours = json.loads(blob("HEAD", MANIFEST.as_posix()).decode("utf-8"))
    entries = theirs["spec"]["contracts"]["contributions"]
    their_ids = {e["id"] for e in entries}
    added = [e for e in ours["spec"]["contracts"]["contributions"] if e["id"] not in their_ids]
    entries.extend(added)
    # Digests are NOT computed here. They are computed once, at the bottom, after every file this
    # script touches is final -- including the schema below, which is itself a declared file. The
    # first version of this script hashed before rewriting the schema and declared the digest of
    # bytes it was about to replace: the exact stale-digest defect the package guard exists to
    # catch, committed by the tool written to prevent it. Order is the whole content of the fix.
    print(f"manifest : {len(their_ids)} from {main_sha} + {len(added)} from this branch = {len(entries)}")
    for e in added:
        print(f"           + {e['id']}")

    # --- schema enum: main's order, then this branch's additions, de-duplicated
    their_enum = json.loads(blob(main_sha, SCHEMA.as_posix()).decode("utf-8"))["$defs"]["refusalCode"]["enum"]
    our_enum = json.loads(blob("HEAD", SCHEMA.as_posix()).decode("utf-8"))["$defs"]["refusalCode"]["enum"]
    want = their_enum + [c for c in our_enum if c not in their_enum]
    current = SCHEMA.read_text(encoding="utf-8")
    m = re.search(r'("refusalCode"\s*:\s*\{[^}]*?"enum"\s*:\s*\[)(.*?)(\s*\])', current, re.S)
    if not m:
        sys.exit("could not find the refusalCode enum block in the schema")
    # Splice through the closing bracket, not up to it. Replacing only the members leaves the
    # original whitespace before `]` in place and adds this body's own, so each run grows the file
    # by a few blank lines. It stayed invisible because the digest moved with it: the manifest
    # check kept passing while the bytes drifted, which is a check tracking a defect instead of
    # reporting it. Byte-identity across two runs is the property; a passing check is not.
    body = "\n" + ",\n".join('        "%s"' % c for c in want) + "\n      ]"
    rebuilt_schema = current[: m.start(2)] + body + current[m.end(3) :]
    if not check_only:
        SCHEMA.write_bytes(rebuilt_schema.encode("utf-8"))

    # --- vocabulary: main's arms first, then this branch's, by reconstructing the conflict region
    src = VOCAB.read_text(encoding="utf-8")
    region = re.search(r"<<<<<<< [^\n]*\n(.*?)=======\n(.*?)>>>>>>> [^\n]*\n", src, re.S)
    if region and not check_only:
        VOCAB.write_bytes(
            (src[: region.start()] + region.group(2) + region.group(1) + src[region.end() :]).encode("utf-8")
        )

    # --- every touched file is now final, so the digests can be declared
    for e in entries:
        source = (
            rebuilt_schema.encode("utf-8")
            if (check_only and (PKG / e['path']) == SCHEMA)
            else (PKG / e["path"]).read_bytes()
        )
        e["sha256"] = "sha256:" + hashlib.sha256(source).hexdigest()
    if not check_only:
        MANIFEST.write_bytes((json.dumps(theirs, indent=2) + "\n").encode("utf-8"))

    # --- membership, both directions, both halves, against the sha and not against a branch name
    result_text = rebuilt_schema if check_only else SCHEMA.read_text(encoding="utf-8")
    result_enum = json.loads(result_text)["$defs"]["refusalCode"]["enum"]
    arms = refusal_arms(VOCAB.read_text(encoding="utf-8"))
    dupes = [k for k, v in collections.Counter(result_enum).items() if v > 1]
    missing = [c for c in their_enum if c not in result_enum]
    mine = [c for c in result_enum if c not in their_enum]
    stale = [e["id"] for e in entries
             if "sha256:" + hashlib.sha256((PKG / e["path"]).read_bytes()).hexdigest() != e["sha256"]]

    print(f"\nREFUSAL-CODE MEMBERSHIP CHECK against {main_sha}")
    print("  (every row below counts CODES. Manifest ENTRY counts are the block above.)")
    print(f"  codes: rust arms {len(arms)}   schema {len(result_enum)}   main {len(their_enum)}")
    print(f"  codes missing from main   : {missing}          <- MUST be []")
    print(f"  codes added by this branch: {mine}")
    if not mine:
        print("      (empty is expected for a manifest-only branch -- it means no new CODES,")
        print("       not that this branch adds nothing. The entry count is in the block above.)")
    print(f"  duplicate codes           : {dupes}            <- MUST be []")
    print(f"  rust minus schema   : {sorted(set(arms) - set(result_enum))}   <- MUST be []")
    print(f"  schema minus rust   : {sorted(set(result_enum) - set(arms))}   <- MUST be []")
    print(f"  order of main kept  : {[c for c in result_enum if c in their_enum] == their_enum}")
    print(f"  manifest stale      : {stale}            <- MUST be []")
    print("\nThe invariant is the EMPTY difference, never a count. Two lanes can land between your")
    print("merges, and chasing an expected number gets the right total by the wrong composition.")
    ok = not (missing or dupes or stale) and set(arms) == set(result_enum)
    if check_only:
        ok = report_disagreement(entries, result_enum) and ok
        print("\nVERDICT:", "the two derivations AGREE" if ok
              else "THEY DISAGREE -- one of you is wrong; the lines above say which entries")
    else:
        print("\nVERDICT:", "union written -- now have someone who is not you check it" if ok
              else "DO NOT COMMIT")
    return 0 if ok else 1


def report_disagreement(derived_entries, derived_enum):
    """Compare this derivation against what is already on disk, by MEANING.

    Reached only in --check mode, where nothing was written, and that is the point: an
    instrument that overwrites its subject reports agreement it never tested. Verified by
    perturbing a real entry and confirming this reports the difference AND leaves the tree
    untouched. A check mode tested only against an agreeing tree proves nothing, because an
    overwrite onto matching content is invisible.
    """
    try:
        on_disk = json.loads(MANIFEST.read_text(encoding="utf-8"))["spec"]["contracts"]["contributions"]
        disk_enum = json.loads(SCHEMA.read_text(encoding="utf-8"))["$defs"]["refusalCode"]["enum"]
    except json.JSONDecodeError as broken:
        # Report it. A file on disk that does not parse IS the disagreement, and the person
        # running this needs the line number, not a stack from inside the checker.
        print("\nAGREEMENT WITH THE RESOLUTION ALREADY ON DISK")
        print("  the resolution on disk is not valid JSON:", broken)
        return False

    agree = True
    disk_map = {e["id"]: e["sha256"] for e in on_disk}
    derived_map = {e["id"]: e["sha256"] for e in derived_entries}
    print("\nAGREEMENT WITH THE RESOLUTION ALREADY ON DISK")
    for label, value in (
        ("entries only on disk", sorted(set(disk_map) - set(derived_map))),
        ("entries only in this derivation", sorted(set(derived_map) - set(disk_map))),
        ("entries whose digest differs",
         sorted(i for i in set(disk_map) & set(derived_map) if disk_map[i] != derived_map[i])),
        ("codes only on disk", [c for c in disk_enum if c not in derived_enum]),
        ("codes only in this derivation", [c for c in derived_enum if c not in disk_enum]),
    ):
        print("  {:34}: {}".format(label, value))
        if value:
            agree = False
    if agree and disk_enum != derived_enum:
        print("  {:34}: disk {} vs derived {}".format("same codes, DIFFERENT ORDER", disk_enum, derived_enum))
        agree = False
    return agree


if __name__ == "__main__":
    args = sys.argv[1:]
    check = "--check" in args
    positional = [a for a in args if not a.startswith("--")]
    if len(positional) != 1:
        sys.exit(__doc__)
    sys.exit(main(positional[0], check))
