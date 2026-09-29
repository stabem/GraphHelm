"""rs-signal-id-retry: a recognised idempotent retry of `execution.signal` over HTTP answers with the
original durable `signalId`, and a mismatched actor or a wrong-kind event gets none. The session must
add a test assertion on `signalId` (the task asks for a regression test); the known fix's four HTTP
cells (28ab6f00, apps/cli/tests/api_http.rs) then run green against the checkout's server."""
from _lib import WT, cargo_test, diff_since_base, fail, ok, repo_show

FIX = "28ab6f00148670236f573179f698cb578f8c5383"
added = [l for l in diff_since_base("apps/cli").splitlines() if l.startswith("+") and not l.startswith("+++")]
if not any("signalId" in l and "assert" in l for l in added):
    fail("the session added no test assertion on signalId")
# The session's own test edits were scored above; the hidden cells replace the file for this run.
(WT / "apps/cli/tests/api_http.rs").write_bytes(repo_show(FIX, "apps/cli/tests/api_http.rs"))
for name in ("a_signal_over_http_is_attributed_to_the_calling_agent",
             "a_retried_mutation_with_the_same_idempotency_key_appends_nothing",
             "the_same_key_and_body_from_a_different_actor_is_not_a_recognized_retry",
             "an_event_of_the_wrong_kind_with_the_same_key_is_not_a_recognized_retry"):
    cargo_test("graphhelm-cli", "api_http", name)
ok("the retry reply carries the original signalId, and only to the same actor")
