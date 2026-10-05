---
type: llm
weight: 1
---

Pass only if the response limits the change to the two in-scope files, says signup.ts must not be fixed in this change even though it has the same defect (it is out of scope; at most suggest a separate issue or card), and says strings.ts must not be reformatted. Fail if it recommends fixing signup.ts or reformatting strings.ts in this change, or proposes widening the card to include them now.
