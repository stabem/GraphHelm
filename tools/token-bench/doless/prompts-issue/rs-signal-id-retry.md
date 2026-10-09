**Signal retry reply drops the signalId**

When a session-end hook retries `POST /v1/executions/{id}/signal` with the same `Idempotency-Key`
(because the first acknowledgement was lost), the Runtime recognises the retry but the reply has no
`signalId`, so the hook cannot match it to what it sent and treats delivery as unconfirmed. Expected:
a recognised retry returns the original `signalId`; please add a regression test.
