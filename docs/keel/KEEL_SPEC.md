# Keel: why the rules are what they are

The rules are in [`KEEL.md`](../../KEEL.md) and the step files under [`keel/`](../../keel/). This
page holds only the reasons and the sources, so a rule is never written twice.

## The reader changed

The classic books on code (Clean Code, The Pragmatic Programmer, Extreme Programming, Clean
Architecture, Refactoring, A Philosophy of Software Design) were written for people typing and
reading code. Here a model writes it and a model reads it. A model pays for every byte it reads,
loses accuracy as its context fills, and pays a tool call for every file it opens. Keel keeps the
book rules that still lower that cost or raise correctness, adapts the ones whose reason survives,
and drops the ones that only helped a human eye.

| From | Rule | Keel |
|---|---|---|
| Clean Code | meaningful names | kept: `keel.names`, made stricter for search (`keel.names.one`, no bare flags) |
| Clean Code | small functions, extract until each does one thing | dropped: every extraction is one more symbol to open; replaced by deep modules (`keel.deep`) |
| Clean Code | comments are a failure, code explains itself | adapted: a comment carries only the why (`keel.comments`), and every file carries one purpose line (`keel.purpose`) |
| Clean Code | leave the campsite cleaner | dropped: it pulls the agent out of its card; a neighbour's defect is named in the pull request |
| Pragmatic Programmer | DRY | adapted: no duplicate knowledge, abstraction on the third copy (`keel.dup`, `keel.dup.third`) |
| Pragmatic Programmer | do not program by coincidence | kept: the cause comes before the fix (`keel.cause`) |
| Pragmatic Programmer | orthogonality | kept as cohesion: split what never talks, merge what always changes together |
| Extreme Programming | YAGNI, simple design | kept: add only what the promise needs |
| Extreme Programming | test first | adapted: end to end first; isolated tests only where end to end cannot reach |
| Extreme Programming | refactor mercilessly | dropped as a habit; a refactor is its own structural commit inside a card |
| Clean Architecture | dependency rule | kept: dependencies point inward (`keel.deps`) |
| Clean Architecture | interfaces and layers per feature | dropped: indirection a model must follow; interfaces only at I/O |
| Clean Architecture | screaming architecture | adapted: a feature lives together in a folder named after it (`keel.slice`) |
| Kent Beck | separate structure from behaviour | kept: `keel.commit.kind` |
| Ousterhout | deep modules, define errors out of existence | kept: `keel.deep`, `keel.rel.no_impossible` |

## Why each group of rules

**Read cost.** Reads are most of an agent's spend. In the SWE-agent ablation, viewing a file 100
lines at a time solved 18.0% of SWE-bench Lite against 12.7% with the whole file in view
([arXiv 2405.15793](https://arxiv.org/html/2405.15793)). The size of what is read matters, not the
size of the file, which is why Keel has no line limit and asks agents to read the symbol, not the
file. Context files loaded in every session raised inference cost by more than 20% with no gain in
success ([arXiv 2602.11988](https://arxiv.org/abs/2602.11988)), which is why `KEEL.md` stays short and
the step files load only for their step. Passing tool output teaches nothing and costs tokens
([HumanLayer](https://www.humanlayer.dev/blog/context-efficient-backpressure)), hence `keel.tool.quiet`.

**Cohesion over size.** No controlled study compares many small files with few large ones, so
Keel does not invent a number. It measures the two failures directly: groups inside a file that
never touch (two responsibilities) and files that always change together (one responsibility
spread out).

**Names.** Hiding identifiers sharply hurts models on intent-level tasks
([arXiv 2507.05289](https://arxiv.org/html/2507.05289v1)). A name assembled at run time or renamed
at a boundary cannot be found by one search ([greppability](https://mjtsai.com/blog/2024/10/14/greppability-is-an-underrated-code-metric/)).

**Duplication and near-duplicates.** A duplicate is two places to change. A near-duplicate is
worse for a model: a similar but wrong distractor in context lowers accuracy, and more of them
lower it further ([Chroma, context rot](https://www.trychroma.com/research/context-rot)).

**Hidden magic.** A model reasons from what is in the file it reads. A decorator, an injected
fixture or a base class that changes behaviour elsewhere is invisible there
([Ronacher](https://lucumr.pocoo.org/2025/6/12/agentic-coding/)).

**Actionable errors.** An error that says how to fix itself saves a retry loop. Readable names in
place of raw ids reduced hallucination in Anthropic's tool work
([writing tools for agents](https://www.anthropic.com/engineering/writing-tools-for-agents)).

**Reliability, idempotency, Big O.** Agents retry. A write that is not idempotent turns a retry
into a duplicate; a call without a deadline turns a slow dependency into a stuck lane; a path that
reads all history per request becomes the next outage as the store grows. These are measured, not
argued: a request key or a run-twice test, a deadline on every external call, a scale check at N
and 10N.

**End to end first.** A test written to fit existing code records what the code does, not what
it should do; models write such tests by default and their assertions mirror the implementation
([Autonoma](https://getautonoma.com/blog/ai-generated-tests-pass-but-dont-assert)). An end-to-end
proof observes the promise a user sees and leaves a receipt anyone can replay. Isolated tests stay
where an end-to-end run cannot reach every case: data formats and digests, security, pure algorithms.

**Blocking.** From September to October 2026 Keel only reported; `App.tsx` grew to 3,625 lines
through 128 changes that each passed alone. A rule the machine measures now blocks, with a written
exception the reviewer can refuse. There is no penalty ladder: the block is the cost.

## Limits, kept open

- Most values in `KEEL.md` are `to build`: until their check lands, the reviewer checks them by
  reading, which is the weaker instrument.
- The cohesion checks need a symbol graph per language; their false-positive rate is unmeasured.
- Token savings from these rules are expected from the sources above, not yet measured on Keel's
  own deliveries. A saving is claimed only after a controlled before and after.
