# M09 seeds — what is still standing when M08 closed

> **Status:** open work, not history. The milestone record for M08 is
> [`ask-once-and-sleep.md`](ask-once-and-sleep.md); this file is deliberately separate, because
> a milestone record is a closed story and a seed is unfinished work, and mixing them leaves the
> next reader unable to tell which is which.

M08 closed after nine blind-judge runs. The defect that named it is dead, with the proof in
bytes. What follows is what the ninth run still said, plus two items the pair found on itself.

## How the naming finding ended, stated precisely

The finding M08 existed to remove — the one-glance answer permanently reading `unknown` — is
gone. The ninth run's transcript settles it without anyone being believed:

    8   POST amend-budget  200  attention="needs_you"  silenceUnevaluated=[]
    10  POST amend-budget  200  attention="needs_you"  silenceUnevaluated=[]
    11  GET  status        200  attention="needs_you"  silenceUnevaluated=[]

In run eight the read at that position reverted to `unknown` with the node repopulated, twice.

An objection still occupies that finding's number, and it must not be read as the same
complaint weakened. It **changed species**:

- **Before:** an assertion that the mechanism fails. That is a defect, and a defect dies to
  evidence.
- **Now:** an argument about a default — why must the operator declare a per-node bound before
  the monitor can speak at all? That makes no factual claim measurement can refute, so no
  amount of evidence retires it. It is answered by a decision, not by a fix.

Recording it as a "downgrade from critical to high" would invite a later reader to treat it as
nearly solved. It is not nearly solved; it is a different kind of question.

## Seeds, in the order they are worth taking

### 1. Evidence that will not open is not evidence

Two journals committed under `docs/acceptance/` fail integrity verification, and they fail at
the parent commit — measured, pre-existing, nobody's regression. This ranks first despite
being the oldest, because those files are cited as proof in an acceptance document. A product
whose thesis is "history that reproduces" cannot have committed history that does not open;
the failure contaminates the argument, not just the files.

### 2. What is the default when nobody declared anything?

Today the answer is `unknown` — honest, and expensive. The judge's standing objection is that
requiring per-node configuration before the surface will answer is the work the surface exists
to avoid. This is a product decision with real alternatives (a declared default that says it is
a default; refusing to start work whose silence cannot be judged; a per-graph rather than
per-node bound), and each alternative has to survive the same rule: absence must never be
laundered into calm.

### 3. A run wedged in the queue never wakes anyone

Silence is evaluated only for `running` nodes. A node that failed, requeued, and then sat in
`queued` for over two minutes reads as healthy — even after a bound was explicitly declared for
it. This is the same shape as the wedge rule's deliberate refusal to treat `Running` as stuck,
one state over, and the two cases genuinely differ: forcing `running` to mean "stuck" would
make the rule lie, while `queued` after a retryable failure is not progress by any reading.
That distinction only surfaced because someone tried it and the world answered.

### 4. The doorbell cannot ring for silence

The wake lease rings on content appends past a cursor, so a silent hang produces no ring by
construction — the one condition an operator most needs to be woken for. Seven consecutive
judge runs named this. It is the oldest surviving finding in the set.

### 5. `needs_you` carries no remedy and no urgency

`silence_unevaluated` reasons carry an operation and a remedy; `silent_node` reasons are bare.
The operator learns they cannot sleep, and not what to do or how bad it is. The machinery to
fix this already exists — it is the same `Remedy` carried one variant over.

### 6. A counter that gives a false negative on a landed action

`acceptedMutations` stays `0` after amendments are accepted and the head moves. An operator
using it to confirm their intervention landed is told it did not.

### 7. Monitoring inflates the counter it is read with

Arming a wake lease advances `headSequence` while `contentHead` stays put, so a read-only act
moves the number an operator reads as progress — and the number the remedy's freshness token is
built from. `contentHead` exists precisely for this and is not used everywhere it should be.

### 8. Auditing the judge should be routine, not exceptional

`serve --read-audit` made the judge auditable, and the pair used that once: run seven's F8
claimed two fields were duplicated "verbatim" and the payload had "~20 top-level fields";
measured from the served bytes, the fields differ by a discriminator and there are 14. The
finding stood on substance and overstated in wording.

That check should run on every verdict, before acting on it. A finding we act on because it is
right must not also teach us to trust numbers nobody checked. This one is about the pair, not
the product, and it belongs here because it will otherwise be rediscovered the expensive way.

### 9. A flaky test on the path seed 4 attacks

`wake_http::a_sleeper_wakes_on_a_peer_append_with_zero_requests_in_the_window` fails by
assertion — not the transport-level timeout of the known flake family — at roughly three runs
in four, and it fails at the parent commit too. It sits in the wake path, which is exactly what
seed 4 is about, so the two are probably worth taking together.

## What is NOT here, and why

No seed asks for a new blind-judge run to "prove" a fix. Nine paid runs bought the findings
above; the recorder makes the judge auditable but does NOT make his scenario reproducible for
free — a node in flight, silent, and unbudgeted requires the real model call. That limit was
found by trying, and it is written here so the next planner does not budget on the belief that
verification became free.
