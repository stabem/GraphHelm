# Task 1279: test-proof sensitivity replay

This is a retrospective instrument check on the [runner-v11 pilot](../task-1279-v11/README.md).
It asks whether each agent-authored test distinguishes the original defect from its patch, and
whether it observes the promised behavior. It **does not** compare strategies for writing or
removing unit tests, and it is not another agent run.

## Reproduce

At repository revision `83f9e9879fbf8eddc6ad40fe1f3427313532d929` or a descendant retaining
the archived patches, with the historical parent object available locally, Git, Python 3.11,
and pytest 8 installed:

```powershell
python tools/token-bench/proof_sensitivity.py --scratch-root D:\_agent-scratch\graphhelm\keel-proof-pilot
```

The script reads the three archived patches from their committed Git blobs and checks their
SHA-256 digests before applying them to isolated
copies of historical parent `e85e41a8eb0b8af6ef6eac95bf39a42cc9277f8c`. It takes the
agent-authored test from each patch and runs that test against the unchanged parent and the
patched code with `PYTHONUTF8=0`. On Windows, it also runs A's test against the unchanged parent
with `PYTHONUTF8=1`. It does not modify the checkout, start a model, or use GraphHelm Runtime.
The committed [Windows result](results-windows.json) is one observed run, not a golden fixture;
elapsed seconds can vary with host load.

## Observed result on Windows

| Test from arm | Parent, UTF-8 mode off | Patched code, UTF-8 mode off | What the test observes |
| --- | --- | --- | --- |
| A, ordinary | FAIL: `UnicodeEncodeError` | PASS | Unicode received by a real child on Windows. |
| B, GraphHelm | FAIL: missing mocked `encoding` argument | PASS | `subprocess.run` arguments; child delivery is not observed. |
| C, GraphHelm + Keel | FAIL: mocked `encoding` is `None` | PASS | `subprocess.run` arguments; child delivery is not observed. |

A's test also **passed on the defective parent** with Python UTF-8 mode on. Its locale patch
does not force the regression when Python selects UTF-8 before consulting the legacy code page.
The test is sensitive in the targeted Windows configuration but not in every configuration.
The prior blind review found that A's unconditional `.bat` fixture fails on POSIX; this replay
did not rerun a POSIX environment.

Seven focused pytest invocations took 8.35 seconds combined in the committed run. That number is
diagnostic machine time, includes process startup, and is not model spend or total delivery cost.
All three tests go RED then GREEN in the selected environment; only one observes actual child
delivery there. B and C passed the separate hidden real-child oracle in the original pilot, so
this finding concerns their **authored tests**, not the correctness of their production patches.

## What to test next

No arm was assigned a test-pruning treatment here. Therefore this replay cannot say whether
reducing unit tests saves time or preserves quality. Run a separate
paired experiment under the [benchmark protocol](../../BENCHMARK_PROTOCOL.md): current Keel
versus current Keel with a short proof receipt and permission to remove a test only after its
unique obligation is covered by another observer. Freeze the same tasks, model route, hidden
faults, platform matrix, and evaluation before either arm runs. Count authoring, execution,
repair, and review cost; count lost and newly detected defects. A smaller suite is a win only if
quality remains at least as strong and total cost per proven delivery falls.
