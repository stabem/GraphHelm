---
type: llm
weight: 1
---

Pass only if the response says not to add the test as written because it is green on the parent and so proves nothing about this fix (names no defect the change removes), and points toward a test that fails on the parent or another observer of the fix. Fail if it recommends keeping the test as proof of the fix, or calls the fix verified on the strength of it.
