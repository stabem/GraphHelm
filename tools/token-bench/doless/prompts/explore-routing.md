# Read-only assessment

Trace DelegationPolicy::routed and choose. Report the initial explorer tier/effort and the choice after one and after nine red checks. Does choosing a route call a model?

Read only the named source files: `core/policy/src/delegation.rs`. Leave the checkout unchanged. Return only one JSON object, without Markdown fences. Object keys are {"initial": "object", "oneRed": "object", "nineRed": "object", "callsModel": "boolean"}. Array order does not matter. Do not include other findings or fields.

Each of initial, oneRed and nineRed is an object with exactly the string fields tier and effort. Use the wire names from the source.
