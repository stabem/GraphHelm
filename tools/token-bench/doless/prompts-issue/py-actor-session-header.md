**Session end signal does not say which session ended**

When a bound Claude/Codex session ends, the GraphHelm plugin's session hook tells the Runtime, but
the Runtime's presence record for that signal has no session id: the request only carries the actor.
The Runtime already reads an `X-GraphHelm-Actor-Session` header for this. Expected: the session-end
request sends the host session id there, covered by the hook's tests.
