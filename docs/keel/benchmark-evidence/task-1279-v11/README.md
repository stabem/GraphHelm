# Task 1279: three-arm coding pilot on runner v11

This is one sequential instrument pilot, not evidence that a methodology improves delivery
across projects. The interpretation and individual review findings below are the public record
for this imported pilot; the original study discussion remains in the private development archive.
The patches below are the agents' unmodified final diffs against the same historical parent.
They are evidence, not proposed changes to current source.

## Frozen task and controls

- Historical parent: `e85e41a8eb0b8af6ef6eac95bf39a42cc9277f8c`.
- Known fix: `e4072151c8a8960e7a362b11dc1577b926f6beee`.
- Runner source: `061a42ca2d0e31d829ea8b698155b9f302632f03` (bench version 11).
- Task prompt SHA-256: `ffeec8b4bdfb14c5df010e10d6bcf61f4ca164deeef949d980e424c28b254f7b`.
- Hidden oracle SHA-256: `362fa7632c06f81903aff915987aa42f560d675a2b1075fac815036221631630`.
- Claude CLI: `2.1.269`, SHA-256 `97164b206e46eecde26fbe8f3993e2fd015e4f53fa90b26b53a8edcb8bc875a9`.
- Requested model route: `sonnet`; observed coding model: `claude-sonnet-5`. The CLI also
  used Haiku internally on all arms; the reported session estimate includes its usage.
- B/C GraphHelm CLI SHA-256: `27986fe9136e25a2dbbe736d67707743226582cc8daa8c1d9eb374f4987ece4d`.
- Run order: B, C, A. No USD cap; each agent had a 30-minute timeout.

The unchanged historical regression passed at the parent, the hidden oracle failed there
for the intended defect, and the oracle passed against the known fix. Each agent received a
fresh one-commit checkout with no remote or future-fix object. The hidden oracle was installed
only after the agent finished.

## Observed results

| Arm | Submitted / old regression / hidden oracle | Agent seconds | CLI estimated USD | Input / output tokens | Cache-read / cache-write tokens | Preflight / checkout seconds | Patch SHA-256 |
| --- | --- | ---: | ---: | --- | --- | --- | --- |
| A, ordinary | PASS / PASS / PASS | 585.4 | 0.7734448 | 50 / 22,882 | 1,552,674 / 58,188 | 167.0 / 37.0 | [`a.patch`](a.patch): `4ce61ca8a045acb05baa5406ba3914d90bc21b02bb1a40f696cfb919863ca712` |
| B, GraphHelm | PASS / PASS / PASS | 400.9 | 0.8479418 | 50 / 19,515 | 1,535,459 / 86,033 | 234.3 / 31.2 | [`b.patch`](b.patch): `8b0ddf1f24ccba3b96496e94406a364d6ed639deeed1c32cde48b34bd2cb286c` |
| C, GraphHelm + Keel | PASS / PASS / PASS | 414.7 | 0.7341958 | 46 / 20,544 | 1,404,459 / 61,018 | 111.7 / 23.7 | [`c.patch`](c.patch): `09824f6d0a38f101df89a5f3fb210abd430c7f0cb3a71e5a3212eb9d3be6dd31` |

The automatic runner marked all three `PASS`. Blind review found a material portability
regression in A: its agent-authored test launches a Windows `.bat` file unconditionally, so
the focused test suite fails on POSIX. A Linux container reproduced the batch-file execution
error. A therefore does **not** count as cross-platform proven delivery. B and C had no
material production-code finding. Their agent-authored tests only inspect subprocess
configuration; the independent hidden oracle exercises real Unicode delivery through a
child process. B and C both recorded GraphHelm `start`, `briefing`, `compile_context`, and
`signal` with matching private Runtime events. C produced a Keel card, but whether it preceded
the first edit remains unobserved.

On this one task, C's agent session cost 5.1% less and took 29.2% less time than A's. These
figures are diagnostic: A failed the blind portability review, the order is not
counterbalanced across tasks, and disk load changed markedly between runs. The Runtime used
fixture executors (`model=false`, `tools=false`), so the pilot does not measure real model or
tool execution through GraphHelm or index-assisted retrieval. CLI USD is a list-price
estimate, not a bill. Reviewer, machine, setup and retry costs have not been monetized; total
cost per proven delivery is therefore not yet measured.

The next study step is to calibrate and freeze the five additional candidate tasks recorded
on the study issue, counterbalance arm order, observe Keel card timing, and add separate
platform observers where portability is a delivery promise.
