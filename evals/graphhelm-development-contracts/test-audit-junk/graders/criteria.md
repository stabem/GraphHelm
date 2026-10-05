---
type: llm
weight: 1
---

Pass only if the response says test 1 proves nothing because it compares the subject's output with itself (self-comparison: the expected value is computed by the code under test), and says test 2 proves little or nothing about UserService because the mock computes the answer and the service only forwards it (mock implements the behavior). Noting the few forwarding regressions test 2 can still catch is fine. Fail if it treats either test as a solid proof of the subject's logic, or misses either pattern.
