# Read-only assessment

Assess this evidence against _end_impl: an HTTP success reply contains data.executionId equal to the current execution but data.signalId is a different signal. No task outcome or completion event was observed. Return acknowledgement (matched or mismatched), taskCompletion (proven or unobserved), retryAuthorized (boolean: does this function authorize another write after the mismatch?), and missingEvidence (task_outcome or none). Do not send requests or change files.

Read only the named source files: `plugins/graphhelm/hooks/session_hook.py`. Leave the checkout unchanged. Return only one JSON object, without Markdown fences. Object keys are {"acknowledgement": "string", "taskCompletion": "string", "retryAuthorized": "boolean", "missingEvidence": "string"}. Array order does not matter. Do not include other findings or fields.
