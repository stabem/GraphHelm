**Codex kills the SessionEnd hook before it can confirm**

On Codex, the GraphHelm session-end hook sometimes never confirms delivery. Codex enforces a
three-second cap on `SessionEnd` hook commands no matter what the hook declares; ours declares more
and its acknowledgement wait alone can use the whole three seconds. Expected: on Codex the hook's
declared budget and its wait fit inside what Codex actually enforces, and the plugin docs say so.
