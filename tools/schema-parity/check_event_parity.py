#!/usr/bin/env python3
"""Cross-check every #162 `EventKind` payload struct against `event-envelope.schema.json`.

Text-only: it reads the two files and compares them. It never builds, so it can run while
another agent holds the shared target directory, and it answers a question `cargo check` does
not -- `check` proves the Rust compiles, never that the wire contract agrees with it.

Three claims per payload:
  * property NAMES match, with `camelCase` applied the way `rename_all` applies it;
  * `required` is exactly the fields WITHOUT `skip_serializing_if`;
  * no property is simultaneously optional and nullable.

The third exists because `Option<T>` has two schema spellings that make DIFFERENT claims.
With `skip_serializing_if`, the key is absent from `required` and is not nullable: an absent
key says nobody declared the field. Without it, the key is required AND nullable: a present
`null` says the field WAS declared and its value is nothing. Measured across the existing
kinds, 8 take the first spelling and 2 (`previousState`, `previousMode`) take the second.

Sabotages run against this checker, each falling at a NAMED claim rather than "somewhere":
  A  make an optional budget required        -> required
  B  required AND nullable                   -> required
  C  snake_case key instead of camelCase     -> props
  D  not required BUT nullable               -> the nullable claim, and ONLY that one

D is not redundant with B: A, B and C all fall at another claim first, which would leave the
nullable check green forever while looking like it was doing work.

A FOURTH claim guards the checker's own PRECONDITION, and it exists because N asked what would
invalidate the comparison rather than what would fail it. Everything above reads serde ATTRIBUTES
and assumes derive semantics. A hand-written `impl Serialize for T` makes those attributes
decorative: the wire form is then whatever the impl says, `skip_serializing_if` means nothing, and
this checker would keep reporting parity while measuring a model the code no longer follows -- a
green that is not merely wrong but wrong about its own subject.

So each payload must derive `Serialize` and `Deserialize`, and no manual impl may exist for it.
  E  replace a derive with a hand-written impl -> the precondition claim, before any comparison
"""

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCHEMA = ROOT / "schemas" / "event-envelope.schema.json"
EVENTS = ROOT / "core" / "protocols" / "src" / "event.rs"

FIELD = re.compile(r"(?:#\[serde\((?P<attr>[^)]*)\)\]\s*)?pub (?P<name>\w+): (?P<ty>[^,]+),")


def camel(name):
    return re.sub(r"_(.)", lambda m: m.group(1).upper(), name)


MANUAL_IMPL = re.compile(r"impl\s+Serialize\s+for\s+(\w+)")


def attribute_block(source, struct):
    """The contiguous `#[...]` lines immediately above the struct.

    Walked BACKWARDS from the struct rather than matched forwards. A forward pattern like
    `((?:#\[...\]\s*)*)pub struct X` matches the EMPTY string at the position just before
    `pub struct` and still succeeds, so it reports "no attributes" for every struct -- a
    false positive that the baseline run caught, which is the only reason this docstring
    exists instead of a silent bug.
    """
    marker = "pub struct " + struct + " {"
    at = source.find(marker)
    if at < 0:
        return None
    lines = source[:at].splitlines()
    block = []
    for line in reversed(lines):
        stripped = line.strip()
        if stripped.startswith("#["):
            block.append(stripped)
        elif stripped.startswith("///") or not stripped:
            break
        else:
            break
    return chr(10).join(block)


def precondition(source, struct):
    """Derive-based serialisation is what makes reading the attributes meaningful.

    Everything else here reads serde ATTRIBUTES. A hand-written `impl Serialize` makes them
    decorative -- the wire form becomes whatever the impl says, `skip_serializing_if` means
    nothing, and the comparison would keep reporting parity while measuring a model the code
    no longer follows.
    """
    attrs = attribute_block(source, struct)
    if attrs is None:
        return ["struct not found in event.rs"]
    problems = []
    # The whole comparison applies camelCase to the Rust field names, which is only what serde
    # does if the type SAYS so. Without this check the checker is a false green: strip
    # `rename_all` and the emitted JSON becomes snake_case -- a real, silent wire break -- while
    # the comparison still converts and still reports parity. Verified by removing the attribute
    # and watching it report OK.
    if not re.search(r'rename_all\s*=\s*"camelCase"', attrs):
        problems.append('does not carry serde(rename_all = "camelCase")')
    for trait in ("Serialize", "Deserialize"):
        if not re.search(r"#\[derive\([^)]*" + trait + r"[^)]*\)\]", attrs):
            problems.append("does not derive " + trait)
    if struct in MANUAL_IMPL.findall(source):
        problems.append("has a hand-written impl Serialize, so the attribute model does not apply")
    return problems


def payload_fields(source, struct):
    match = re.search(r"pub struct " + struct + r" \{(.*?)\n\}", source, re.S)
    if match is None:
        raise SystemExit("struct not found in event.rs: " + struct)
    return [
        (m.group("name"), m.group("ty").strip(), m.group("attr") or "")
        for m in FIELD.finditer(match.group(1))
    ]


def compare(schema, source, struct, key):
    fields = payload_fields(source, struct)
    definition = schema["$defs"][key]
    properties = set(definition["properties"])
    required = set(definition["required"])
    named = {camel(n) for n, _, _ in fields}
    skipped = {camel(n) for n, _, attr in fields if "skip_serializing_if" in attr}

    problems = []
    if named != properties:
        problems.append("props: rust %s vs schema %s" % (sorted(named), sorted(properties)))
    if named - skipped != required:
        problems.append(
            "required: expected %s vs schema %s" % (sorted(named - skipped), sorted(required))
        )
    for name in sorted(properties - required):
        if "null" in json.dumps(definition["properties"][name]):
            problems.append(name + " is optional AND nullable")
    return problems


PAYLOADS = {
    "DlqRouted": "dlqRouted",
    "DlqRedrive": "dlqRedrive",
    "DlqReturned": "dlqReturned",
    "SweepPerformed": "sweepPerformed",
    "OverdueException": "overdueException",
}

TAGS = {
    "dlq_routed": "dlqRouted",
    "dlq_redrive": "dlqRedrive",
    "dlq_returned": "dlqReturned",
    "sweep_performed": "sweepPerformed",
    "overdue_exception": "overdueException",
}


def main():
    schema = json.loads(SCHEMA.read_text(encoding="utf-8"))
    source = EVENTS.read_text(encoding="utf-8")
    failures = 0

    for struct, key in PAYLOADS.items():
        problems = precondition(source, struct)
        if not problems:
            problems = compare(schema, source, struct, key)
        print("%-18s %-18s %s" % (struct, key, "OK" if not problems else "; ".join(problems)))
        failures += len(problems)

    union = {b["properties"]["type"]["const"] for b in schema["$defs"]["eventKind"]["oneOf"]}
    scoped = {b["properties"]["kind"]["properties"]["type"]["const"] for b in schema["oneOf"]}
    for tag, key in TAGS.items():
        missing = [
            site
            for site, present in (
                ("union", tag in union),
                ("scope", tag in scoped),
                ("$defs", key in schema["$defs"]),
            )
            if not present
        ]
        print("%-20s %s" % (tag, "OK" if not missing else "MISSING FROM " + ", ".join(missing)))
        failures += len(missing)

    print("PARITY OK" if failures == 0 else "PARITY FAILURES: %d" % failures)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
