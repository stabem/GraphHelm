**Consumer finding from #221 (task-005), posted here per this file's own rule — "codes needed by
consuming lanes are allocated HERE, never minted downstream."**

Starting #221's implementation against `core/protocols/src/development.rs` @ `9e93c6d`. Measured,
with a positive control: `DevelopmentRefusalCode` (`:87-101`) has twelve variants — none of them
`OwnerOutputSchemaInvalid` or `OwnerOutputUnsafeCompression`. The two codes the design doc names
for this task (`OWNER_OUTPUT_SCHEMA_INVALID`, `OWNER_OUTPUT_UNSAFE_COMPRESSION`, design §9) exist
only in prose (`git grep` confirms zero code occurrences, positive control: the same command finds
`DevelopmentRefusalCode` itself).

`core/protocols/src/development.rs` is **not** in #221's files-in-scope (strict), so I cannot add
these variants myself without violating the issue's own discipline. Two ways to close this, and I
have not decided between them:

1. `OWNER_OUTPUT_SCHEMA_INVALID` maps cleanly onto the **already-allocated** `SchemaInvalid` — a
   malformed style plan or a cardinality-mismatched result genuinely is a schema violation. No new
   code needed for this half.
2. `OWNER_OUTPUT_UNSAFE_COMPRESSION` has no existing match — nothing in the current twelve names
   "a safe-but-compressed rendering would hide required material." This one needs either a new
   variant in `DevelopmentRefusalCode`, or a decision that #221 reuses `CardinalityViolation` or
   `SchemaInvalid` for it too (losing the distinction the design's own refusal table draws between
   "the input was malformed" and "the input was fine but the requested compression was unsafe").

Not blocking T1 (the no-decision/`Nothing now` case doesn't touch a refusal code), but it blocks
T3, T6, and T9 of the sealed test plan. Flagged to the orchestrator in parallel; proceeding with
T1 while this is decided.
