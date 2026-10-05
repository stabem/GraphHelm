---
max_turns: 4
allowed_tools: [Skill]
---

Use the keel skill. I fixed a rounding bug in `price()`. I wrote a new test `price_rounds_half_up`. I ran it on the parent commit (without my fix) and it also passes there. It passes on my branch too. Should I add this test to the PR? Answer only.
