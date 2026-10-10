# Read-only assessment

Trace the execution status delegation field to its source and the NodePanel label. Report source as the projection field name, statusFields as an array, defaultsAbsentNode as a boolean, and labelForAbsentChoice as a boolean (whether a label renders without a choice).

Read only the named source files: `apps/cli/src/commands/execution/mod.rs`, `apps/studio/src/components/panel.tsx`. Leave the checkout unchanged. Return only one JSON object, without Markdown fences. Object keys are {"source": "string", "statusFields": "array", "defaultsAbsentNode": "boolean", "labelForAbsentChoice": "boolean"}. Array order does not matter. Do not include other findings or fields.
