# The S1'/S2' control: hold the FIXTURE constant and remove ONLY the two assertions added by
# ac4fba2. Checking out the pre-strengthening file instead would vary two things at once (the
# identities in the batch AND the assertions) and would drag back types this branch removed.
#
# The anchors are RAW strings. A quoted heredoc still collapses \ to \ on the way in, and a lone
# backslash before a newline inside a normal Python string is a line continuation: the newline
# disappears and the anchor can never match a file that has one. Rust string continuations end in
# exactly that character, so every anchor cut from this codebase needs r"""...""".
import io

P = "core/events/tests/execution_projection.rs"
MARKERS = ['!first.clearance_registry.contains_key("auditor-c")', '.get("auditor-d")']

L = io.open(P, encoding="utf-8", newline="").read().split("\n")
drop = set()
for m in MARKERS:
    hits = [i for i, l in enumerate(L) if m in l]
    assert len(hits) == 1, "%s -> %d hits" % (m, len(hits))
    i = hits[0]
    b = i
    while not L[b].startswith("    assert"):
        b -= 1
    e = i
    while L[e] != "    );":
        e += 1
    drop.update(range(b, e + 1))

io.open(P, "w", encoding="utf-8", newline="").write(
    "\n".join(l for i, l in enumerate(L) if i not in drop)
)
print("STRIPPED %d lines (the two assertions added by ac4fba2)" % len(drop))
