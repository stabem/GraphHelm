# Why Keel and the journeys missed the Zuvemo QA findings

An external QA ran the Zuvemo app in production and found money, date, plural and status bugs.
Every Zuvemo journey was green and every change had passed a Keel review. Zuvemo's own write-up is
`docs/ERRO-KEEL.md` in `stabem/zuvemo` (#653). This page is the GraphHelm half: what in GraphHelm
and Keel let those bugs through, and which change makes each one caught. Commits below are
GraphHelm `d5d2b9fac` unless named.

## The journeys were on the right screens

On the build the QA saw (Zuvemo `464e0bf`), approved journeys walked through the screens where the
bugs were:

| Bug the user saw | Journey on that screen | What it checked |
|---|---|---|
| Total shown as R$ 123,45 while the room summed R$ 246,90 (unit vs total) | `item-manual` | `{role: textbox, name: Valor}` (an `aria-label` that hid the visible label "Valor total"), typed `12000`, never set the quantity, never read the total |
| A delivery in 2 days shown as "daqui a 1 semana" | `agenda` | `{role: heading, name: Agenda de entregas}`, `{role: button, name: Copiar mensagem}` |
| A delivered item still listed in the agenda | `entrega` | ended on `{role: status, name: Entregue}`, never went back to the agenda |
| 100% in the bar, 101% in the summary; R$ 123 vs R$ 123,45 | `resumo-da-obra` | the column headers `Orçado`, `Gasto`, `Falta`, never a value |

The bugs were not off the map. They were on the map and invisible to it.

## Six blind spots

1. **An expectation cannot hold text.** `$defs/expect` in `schemas/journey-flow.schema.json` is
   `{role, name, absent?}`, matched with `getByRole(role, {name, exact: true})`
   (`tools/journey-driver/driver.mjs:271,436`). A date in a `<span>`, an amount in a `<b>`, a
   sentence in a `<p>` has no role and name to match. A journey can prove that a flow runs, never
   what the screen says. → #1781.
2. **There is no fixed clock.** Relative dates change every day, so no flow can assert "in 2
   days", and fixture seeds use a fixed date weeks away (`2026-11-03`), where the bug did not show.
   AGENTS.md already requires fixed clocks in tests; journeys had none. → #1782.
3. **The draft cannot see numbers.** `skeleton()` (`driver.mjs:338`) replaces every digit with `#`
   and the outline ends with "text left out". That is right for the drift fingerprint, but it is
   also all a drafting agent sees, so it could never propose a check on an amount or a date. →
   #1783.
4. **A flow is seen in one state.** Each journey has one seed (`JOURNEY_SEEDS` in Zuvemo's
   fixture); no seed had a delivered or installed item, so no journey, visual pass or axe pass ever
   rendered one. Nothing reports a state the screens promise but no run reaches. → #1784.
5. **The outcome rule accepts the screen that acted.** `flow.weak_journey` (#1287) asks that the
   last screen expect something its previous screen did not. `{role: status, name: Entregue}` on
   the same item page passes, though the effect the user cares about is on the agenda. → #1785.
6. **Keel never asked what the screen says.** `KEEL.md` measured cohesion, duplication, reliability
   and scope; no value covered units, cross-screen consistency, status visibility, honest labels,
   copy or rules shown before the failure. Unit tests filled the gap with values copied from the
   code's output: Zuvemo's `ComprasPages.test.tsx` expected `'daqui a 3 semanas'`, and 12 such
   assertions in 6 files encoded the bugs as the specification. → this change (#1780):
   `keel.semantic` with its checklist in `KEEL_REVIEW.md`, `keel.proof.reads_screen`,
   `keel.test.intent`, `keel.audit.sha`.

A seventh failure was process, not tooling: the Zuvemo audit ran on a local checkout five days
behind production. `keel.audit.sha` names it; Zuvemo's live runner now fails a run whose deployed
commit changed mid-run (#653), which happened while that change was being proven.

## What each finding needs to be caught

| Finding | Caught by |
|---|---|
| Unit vs total | #1781 `text` on the room's item row with quantity 2, plus checklist line 1 |
| Date phrase | #1782 clock + #1781 `text` on the delivery heading, at 0/1/2/6/7/14 days |
| Delivered item in the agenda | #1784 a delivered-state seed + #1785 the agenda checked after "Arrived" |
| 100% vs 101%, cents | #1781 `text` on the bar label and on the summary, same amount |
| "Top de linha" with two prices | checklist line 4; #1781 on the option card |
| Quantity 0, generic error | #1781 `text` on the field error; checklist line 6 |

Proving this end to end: once #1781 and #1782 land, Zuvemo's `agenda` and `item-manual` flows get
the checks in the table; run against Zuvemo `464e0bf` they must go red, and against `421ea1e` or
later green.
