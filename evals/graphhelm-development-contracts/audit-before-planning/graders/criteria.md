---
type: llm
weight: 1
---

Pass only if a completed transcript shows the `test-audit` skill was invoked and the final answer gives the requested assessment. Check selection and assessment independently: invoking the skill without completing the assessment does not pass, and a correct assessment without the required invocation does not pass. Do not fail solely because another relevant skill was invoked.

The assessment identifies Test A's self-comparison: both sides call the function under test, so the assertion can pass despite broken behavior. It identifies Test B's independent literal 6 as a meaningful expected value that can catch incorrect multiplication or total behavior for this example; do not claim broad coverage. The answer assesses the existing tests, is not an implementation card, and makes no edits or automatic deletion.

A missing transcript, truncated transcript, tool failure, or infrastructure failure is INCOMPLETE, never successful selection or assessment.
