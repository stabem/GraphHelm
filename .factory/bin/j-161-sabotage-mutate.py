import io, sys
P = "core/events/src/projection.rs"

INSERT = """            projection.clearance_registry.insert(
                payload.identity.as_str().to_owned(),
                payload.key_fingerprint.clone(),
            );"""
REMOVE = """            projection
                .clearance_registry
                .remove(payload.identity.as_str());"""

MUT = {
    "S1": (REMOVE, "            // SABOTAGE S1: the remove is gone."),
    "S2": (INSERT, """            projection
                .clearance_registry
                .entry(payload.identity.as_str().to_owned())
                .or_insert(payload.key_fingerprint.clone());"""),
    "S3": (INSERT, "            // SABOTAGE S3: the insert is gone."),
}

which = sys.argv[1]
s = io.open(P, encoding="utf-8", newline="").read()
old, new = MUT[which]
assert s.count(old) == 1, "%s anchor count %d" % (which, s.count(old))
io.open(P, "w", encoding="utf-8", newline="").write(s.replace(old, new))
print("APPLIED", which)
