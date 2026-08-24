# Phase 2 mutations for #201. Anchors are one-line markers, never generic delimiters: a `],` or a
# bare `}` matches in dozens of places and next() silently picks the wrong one.
import io, sys
P = "core/events/src/projection.rs"
s = io.open(P, encoding="utf-8", newline="").read()
w = sys.argv[1]
if w == "T1":
    a = "                    match projection.clearance_registry.get(identity.as_str()) {"
    assert s.count(a) == 1
    s = s.replace(a, "                    match projection.sabotage_final_registry.get(identity.as_str()) {")
    f = "    pub clearance_registry: BTreeMap<String, WireHash>,"
    assert s.count(f) == 1
    s = s.replace(f, f + "\n    #[serde(skip)]\n    pub sabotage_final_registry: BTreeMap<String, WireHash>,")
    # This line appears TWICE (the replay entry and a test module), so a one-line anchor
    # asserts count == 1 and dies. That death is the correct outcome; the fix is a two-line
    # context unique to the replay entry, never a looser match that picks one silently.
    # chr(10) rather than an escape: a quoted heredoc still collapses a doubled backslash,
    # and a lone backslash before a newline inside a Python string eats the newline.
    anc = chr(10).join([
        "    let schemas = graphhelm_schema::repository_schema_set().map_err(|_| ReplayError::Corrupt)?;",
        "    let mut projection = ExecutionProjection::default();",
    ])
    assert s.count(anc) == 1
    s = s.replace(anc, anc + """
    for e in events {
        match &e.kind {
            EventKind::ClearanceIdentityRegistered(p) => {
                projection.sabotage_final_registry.insert(p.identity.as_str().to_owned(), p.key_fingerprint.clone());
            }
            EventKind::ClearanceIdentityRevoked(p) => {
                projection.sabotage_final_registry.remove(p.identity.as_str());
            }
            _ => {}
        }
    }""")
elif w == "T2":
    a = "            projection\n                .clearance_registry\n                .remove(payload.identity.as_str());"
    assert s.count(a) == 1
    s = s.replace(a, "            // T2: revocation ignored.")
elif w == "T3":
    a = "                        Some(registered) if registered == key_fingerprint => {"
    assert s.count(a) == 1
    s = s.replace(a, "                        Some(_registered) => {")
elif w == "T8":
    # The mutation the review asked for: a refusal becomes Corrupt instead of being recorded.
    # It lives here and not only in prose because the property that lets F1 call this matrix an
    # INSTRUMENT rather than a report is that a reviewer can re-run it. Until this branch, the one
    # mutation born from a reviewer's request was the one a reviewer could not reproduce.
    a = chr(10).join([
        "            projection",
        "                .clearances",
        "                .insert(payload.claim_seq, outcome.clone());",
    ])
    assert s.count(a) == 1
    guard = chr(10).join([
        "            if matches!(outcome, ClearanceOutcome::Refused { .. }) {",
        "                return Err(ReplayError::Corrupt); // T8",
        "            }",
        a,
    ])
    s = s.replace(a, guard)
else:
    raise SystemExit("unknown " + w)
io.open(P, "w", encoding="utf-8", newline="").write(s)
print("APPLIED " + w)
