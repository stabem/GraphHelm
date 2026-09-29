"""py-actor-session-header: the SessionEnd signal request carries the host session id in
`X-GraphHelm-Actor-Session`, and the rest of the hook's contract still holds: the known fix's test
file (4cc5ab0e) runs green against the checkout's hook."""
from _lib import hidden_unittest, ok

FIX = "4cc5ab0ec5f1c35846efbd3440cba5d3cca742f1"
hidden_unittest(FIX, "plugins/graphhelm/hooks/test_session_hook.py")
ok("the end request names the session and the hook's tests pass")
