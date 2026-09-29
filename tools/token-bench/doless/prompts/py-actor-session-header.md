When a GraphHelm-bound session ends, `plugins/graphhelm/hooks/session_hook.py` posts an
`agent_session_ended` signal to the Runtime. The request names the actor
(`X-GraphHelm-Actor`) but not the host session, so the Runtime's structured presence record has no
session id. Send the host session id in the `X-GraphHelm-Actor-Session` header of that end request,
and extend the existing end-request test in `plugins/graphhelm/hooks/test_session_hook.py` to assert
it. Do not add a new header helper or change the start request; the hook's other tests must stay
green.
