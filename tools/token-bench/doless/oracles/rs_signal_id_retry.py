"""rs-signal-id-retry: a recognised idempotent retry of `execution.signal` over HTTP answers with the
original durable `signalId`, and a mismatched actor or a wrong-kind event gets none. The session must
add a test assertion on `signalId` (the task asks for a regression test), on one line or wrapped
across several; the known fix's four HTTP
cells (28ab6f00, apps/cli/tests/api_http.rs) then run green against the checkout's server."""
from _lib import WT, cargo_test, diff_since_base, fail, ok, repo_show

FIX = "28ab6f00148670236f573179f698cb578f8c5383"
MAX_ASSERT_LINES = 40  # an assertion spanning more added lines than this is not read as one


def added_runs(diff: str) -> list:
    """Each run of consecutive added lines in the diff, as one text (the `+` stripped)."""
    runs, current = [], []
    for line in diff.splitlines():
        if line.startswith("+") and not line.startswith("+++"):
            current.append(line[1:])
        elif current:
            runs.append(current); current = []
    if current:
        runs.append(current)
    return runs


def asserts_on(lines: list, name: str) -> bool:
    """True when an `assert*!(...)` macro written in these added lines mentions `name` anywhere in
    its arguments, however rustfmt wrapped them. The call is read up to its balancing parenthesis,
    bounded at MAX_ASSERT_LINES lines; string contents do not count toward the balance."""
    text = "\n".join(lines)
    start = 0
    while (at := text.find("assert", start)) >= 0:
        start = at + 1
        open_at = text.find("(", at)
        head = text[at:open_at] if open_at >= 0 else ""
        if open_at < 0 or not head.rstrip().endswith("!") or not head[:-1].strip().replace("_", "").isalnum():
            continue
        depth, in_str, i = 0, False, open_at
        while i < len(text) and text.count("\n", open_at, i) <= MAX_ASSERT_LINES:
            ch = text[i]
            if in_str:
                if ch == "\\":
                    i += 1
                elif ch == '"':
                    in_str = False
            elif ch == '"':
                in_str = True
            elif ch == "(":
                depth += 1
            elif ch == ")":
                depth -= 1
                if depth == 0:
                    if name in text[open_at:i]:
                        return True
                    break
            i += 1
    return False


def main() -> None:
    if not any(asserts_on(run, "signalId") for run in added_runs(diff_since_base("apps/cli"))):
        fail("the session added no test assertion on signalId")
    # The session's own test edits were scored above; the hidden cells replace the file for this run.
    (WT / "apps/cli/tests/api_http.rs").write_bytes(repo_show(FIX, "apps/cli/tests/api_http.rs"))
    for name in ("a_signal_over_http_is_attributed_to_the_calling_agent",
                 "a_retried_mutation_with_the_same_idempotency_key_appends_nothing",
                 "the_same_key_and_body_from_a_different_actor_is_not_a_recognized_retry",
                 "an_event_of_the_wrong_kind_with_the_same_key_is_not_a_recognized_retry"):
        cargo_test("graphhelm-cli", "api_http", name)
    ok("the retry reply carries the original signalId, and only to the same actor")


if __name__ == "__main__":
    main()
