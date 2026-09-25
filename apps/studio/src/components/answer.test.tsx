import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  AnswerNode,
  MAX_ARTEFACT_BYTES,
  MAX_ARTEFACT_COUNT,
  MAX_PROOF_KIND_LENGTH,
  refusalWords,
  type AnswerOutcome,
} from "./answer";
import type { ClaimEvidence } from "../runtime/types";

afterEach(cleanup);
// `hash` is a MODULE-LEVEL mock, so its call record survives every cell in this file. Any cell
// asserting a call COUNT reads the running total otherwise -- measured: the first artefact-bound
// cell saw 7 calls, none of them its own. Clearing between cells is what makes "not called" and
// "called once" say anything.
afterEach(() => hash.mockClear());

const hash = vi.fn(async (file: File) => ({
  contentHash: `sha256:${"a".repeat(64)}`,
  size: file.size,
}));

function file(name: string): File {
  return new File(["report"], name, { type: "text/plain" });
}

function attach(kind: string, name: string): void {
  fireEvent.change(screen.getByLabelText("Kind of proof"), { target: { value: kind } });
  fireEvent.change(screen.getByLabelText("Artefact"), { target: { files: [file(name)] } });
}

function renderForm(
  onAnswer: (evidence: ClaimEvidence[]) => Promise<AnswerOutcome>,
  waitSeq: number | null = 9,
) {
  return render(
    <AnswerNode node="implementation" waitSeq={waitSeq} onAnswer={onAnswer} hash={hash} />,
  );
}

describe("AnswerNode", () => {
  // The mock declares its PARAMETER, which the zero-argument form does not: `vi.fn(async () =>
  // …)` types `mock.calls` as `[]`, so every `calls[0][0]` below is a type error the suite cannot
  // see — it runs fine and `tsc` refuses it. Found late, because `tsc -b` is incremental and an
  // earlier "typecheck clean" on this file was a cached no-op rather than a reading.
  it("sends the bundle the person built, with the kind they named", async () => {
    const onAnswer = vi.fn(async (_evidence: ClaimEvidence[]) => ({
      step: "claimed-and-cleared",
    }) as AnswerOutcome);
    renderForm(onAnswer);
    attach("test_report", "report.txt");
    await screen.findByText("test_report");
    fireEvent.click(screen.getByRole("button", { name: "Answer this node" }));
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(1));
    expect(onAnswer.mock.calls[0][0]).toEqual([
      { kind: "test_report", contentHash: `sha256:${"a".repeat(64)}`, size: 6 },
    ]);
    await screen.findByText("Answered. The node is released.");
  });

  // THE ARTEFACT MUST NOT TRAVEL. The whole reason this form can take a file at all is that only
  // its digest is sent; a bundle carrying the name, the type or the bytes would be a silent
  // upload from a screen that promises the opposite.
  // The kind is DELIBERATELY not a substring of the name or the bytes here. It was, at first
  // writing, and the cell failed on its own fixture: `test_report` contains `report`, which was
  // also the file's content, so the assertion accused the one field that is supposed to travel.
  it("sends a digest and a size, and nothing else about the file", async () => {
    const onAnswer = vi.fn(async (_evidence: ClaimEvidence[]) => ({
      step: "claimed-and-cleared",
    }) as AnswerOutcome);
    renderForm(onAnswer);
    fireEvent.change(screen.getByLabelText("Kind of proof"), { target: { value: "diff" } });
    fireEvent.change(screen.getByLabelText("Artefact"), {
      target: { files: [new File(["the-bytes-stay-here"], "secret-name.txt", { type: "text/plain" })] },
    });
    await screen.findByText("diff");
    fireEvent.click(screen.getByRole("button", { name: "Answer this node" }));
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(1));
    const sent = JSON.stringify(onAnswer.mock.calls[0][0]);
    expect(sent).not.toContain("secret-name");
    expect(sent).not.toContain("the-bytes-stay-here");
    expect(Object.keys(onAnswer.mock.calls[0][0][0]).sort()).toEqual(["contentHash", "kind", "size"]);
  });

  // A WAIT THAT CANNOT BE READ IS NOT A WAIT TO GUESS AT. Omitting the sequence would ask the
  // Runtime to answer whichever wait is open now, which is the stale rendezvous the field exists
  // to prevent.
  it("offers no controls when the open wait could not be read", () => {
    const onAnswer = vi.fn(async () => ({ step: "claimed-and-cleared" }) as AnswerOutcome);
    renderForm(onAnswer, null);
    expect(screen.queryByRole("button", { name: "Answer this node" })).toBeNull();
    expect(screen.queryByLabelText("Artefact")).toBeNull();
    expect(screen.getByRole("alert").textContent).toContain("could not be read");
  });

  // THIS CELL USED TO ASSERT THE DEFECT. It read "cannot be answered with an empty bundle" and
  // checked the button was disabled with no drafts — which is precisely the behaviour #1187's
  // second review BLOCK named: a node whose declared `proofKinds` is EMPTY is answerable by the
  // Runtime and was unanswerable here, and a node parked by the fixture executor's `NeedsInput`
  // declares no proof kinds at all. A green cell was holding the door shut.
  it("answers with an empty bundle and lets the Runtime decide", async () => {
    const onAnswer = vi.fn(async (_evidence: ClaimEvidence[]) => ({
      step: "claimed-and-cleared",
    }) as AnswerOutcome);
    renderForm(onAnswer);
    // The LABEL says what pressing will send, so the empty bundle is a choice rather than an
    // accident. A button that read "Answer this node" with nothing attached would be the same
    // trap in the other direction.
    const button = screen.getByRole("button", { name: "Answer with no evidence" });
    expect(button).not.toBeDisabled();
    fireEvent.click(button);
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(1));
    expect(onAnswer.mock.calls[0][0]).toEqual([]);
  });

  it("will not take an artefact until its kind is named", () => {
    renderForm(vi.fn(async () => ({ step: "claimed-and-cleared" }) as AnswerOutcome));
    expect(screen.getByLabelText("Artefact")).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Kind of proof"), { target: { value: "diff" } });
    expect(screen.getByLabelText("Artefact")).not.toBeDisabled();
  });

  // A REFUSAL IS NOT A FAILURE OF THE CLICK. The person is told what the Runtime decided, in its
  // own vocabulary, and their bundle is kept so they can fix it rather than rebuild it.
  it("shows the refusal reason and keeps the bundle", async () => {
    const onAnswer = vi.fn(
      async () => ({ step: "claim-refused", reasonCode: "evidence_budget_unmet" }) as AnswerOutcome,
    );
    renderForm(onAnswer);
    attach("wrong_kind", "report.txt");
    await screen.findByText("wrong_kind");
    fireEvent.click(screen.getByRole("button", { name: "Answer this node" }));
    await screen.findByText("The node asked for proof this bundle does not carry.");
    expect(screen.getByText("wrong_kind")).toBeTruthy();
  });

  it("prints an unrecognised reason code rather than swallowing it", () => {
    expect(refusalWords("some_new_code")).toBe("some_new_code");
    expect(refusalWords(null)).toContain("no reason code");
  });

  // THE CLAIM IS SPENT EITHER WAY. Keeping the bundle would invite a second claim against a wait
  // that already has one, which the fold refuses as a duplicate.
  it("drops the bundle and says to re-open when the outcome could not be read", async () => {
    const onAnswer = vi.fn(async () => ({ step: "unknown", claimSeq: 9 }) as AnswerOutcome);
    renderForm(onAnswer);
    attach("test_report", "report.txt");
    await screen.findByText("test_report");
    fireEvent.click(screen.getByRole("button", { name: "Answer this node" }));
    await screen.findByText(/could not read back what happened/);
    expect(screen.queryByText("test_report")).toBeNull();
  });

  it("reports a Runtime it could not reach without claiming anything about the node", async () => {
    const onAnswer = vi.fn(async () => {
      throw new Error("offline");
    });
    renderForm(onAnswer);
    attach("test_report", "report.txt");
    await screen.findByText("test_report");
    fireEvent.click(screen.getByRole("button", { name: "Answer this node" }));
    await screen.findByText("The Runtime could not be reached.");
  });

  // ONE PRESS, ONE CLAIM. A second claim while the first is in flight is refused as a duplicate,
  // and the person would be shown a refusal caused by their own double click.
  //
  // THIS CELL PROVES THE PAIR, NOT EITHER HALF, and that is measured rather than assumed: with
  // only the button's `disabled={busy || ...}` removed it stays green, and with only the `busy`
  // early-return in `submit` removed it stays green. It reddens when BOTH go. So neither guard
  // may be deleted as "covered by the test" — the test says the two of them together hold, and
  // the reason to keep both is that they fail differently: the attribute stops the click, the
  // early-return stops a call that did not come from the button.
  it("does not send a second claim while one is in flight", async () => {
    let release: (outcome: AnswerOutcome) => void = () => {};
    const onAnswer = vi.fn(
      () => new Promise<AnswerOutcome>((resolve) => { release = resolve; }),
    );
    renderForm(onAnswer);
    attach("test_report", "report.txt");
    await screen.findByText("test_report");
    const button = screen.getByRole("button", { name: "Answer this node" });
    fireEvent.click(button);
    await screen.findByRole("button", { name: "Answering…" });
    fireEvent.click(screen.getByRole("button", { name: "Answering…" }));
    expect(onAnswer).toHaveBeenCalledTimes(1);
    release({ step: "claimed-and-cleared" });
    await screen.findByText("Answered. The node is released.");
  });

  it("drops an artefact the person removes", async () => {
    const onAnswer = vi.fn(async () => ({ step: "claimed-and-cleared" }) as AnswerOutcome);
    renderForm(onAnswer);
    attach("test_report", "report.txt");
    await screen.findByText("test_report");
    fireEvent.click(screen.getByRole("button", { name: "Remove report.txt" }));
    expect(screen.queryByText("test_report")).toBeNull();
    // The button does not go dead when the last artefact is removed — it goes back to saying what
    // it would send. Asserting `toBeDisabled()` here was the same defect as the cell above.
    expect(screen.getByRole("button", { name: "Answer with no evidence" })).not.toBeDisabled();
  });
});

// -------------------------------------------------------------------------------------------
// #1187 review BLOCK: the picker read the whole local file before any bound was checked.
//
// `file.arrayBuffer()` allocates the entire file, so a multi-gigabyte pick froze or crashed the
// tab before the request — whose payload IS bounded — could refuse anything. The bound now lives
// where the file arrives, ahead of the read.
//
// A FILE WITH A FAKED `size` IS THE RIGHT FIXTURE, not a real 33 MiB buffer. `File.size` is the
// exact input the guard reads, and materialising 33 MiB to prove a branch that never touches the
// bytes would spend the memory this cell exists to prevent. The `arrayBuffer` spy is what makes
// the claim honest: it fails if anything reads the file, however the guard is written.
// -------------------------------------------------------------------------------------------

/** A file that REPORTS `bytes` and whose `arrayBuffer` records whether anything read it. */
function sizedFile(bytes: number): { file: File; read: ReturnType<typeof vi.fn> } {
  const file = new File(["x"], "report.txt", { type: "text/plain" });
  Object.defineProperty(file, "size", { value: bytes });
  const read = vi.fn(async () => new ArrayBuffer(0));
  Object.defineProperty(file, "arrayBuffer", { value: read });
  return { file, read };
}

/** A size well above any bound this screen would sensibly carry, stated as a LITERAL. */
const HUGE = 64 * 1024 * 1024;
/** A size well below it, also a literal. */
const SMALL = 1024 * 1024;

describe("AnswerNode artefact bound", () => {
  it("refuses a file above the bound without reading a byte of it", async () => {
    const onAnswer = vi.fn(async () => ({ step: "claimed-and-cleared" }) as AnswerOutcome);
    renderForm(onAnswer);
    const { file, read } = sizedFile(HUGE);
    fireEvent.change(screen.getByLabelText("Kind of proof"), { target: { value: "test_report" } });
    fireEvent.change(screen.getByLabelText("Artefact"), { target: { files: [file] } });

    await screen.findByRole("alert");
    // THE POINT OF THE CELL: nothing read the file. `hash` is the injected reader in this test
    // and `arrayBuffer` is what the real one calls; both must be untouched, because a guard that
    // refuses AFTER the allocation has already cost the tab what it was meant to save.
    expect(read).not.toHaveBeenCalled();
    expect(hash).not.toHaveBeenCalled();
    // And nothing was added to the bundle.
    expect(screen.queryByText("test_report")).toBeNull();
  });

  // A SIZE THAT IS NOT A WHOLE MEBIBYTE, and the reviewer is why this cell exists. Both fixtures
  // that reached the formatter were exact multiples, so `Math.ceil` -> `Math.floor` stayed green
  // at 29/29: the rounding was unmeasured because nothing ever had a remainder to round. 32 MiB
  // plus one byte must print as 33, not 32 — a file one byte over the bound that printed the
  // bound back at the person would read as an arbitrary refusal.
  it("rounds the reported size UP, so one byte over never prints as the bound", async () => {
    renderForm(vi.fn(async () => ({ step: "claimed-and-cleared" }) as AnswerOutcome));
    const { file } = sizedFile(MAX_ARTEFACT_BYTES + 1);
    fireEvent.change(screen.getByLabelText("Kind of proof"), { target: { value: "test_report" } });
    fireEvent.change(screen.getByLabelText("Artefact"), { target: { files: [file] } });
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("33 MB");
    expect(alert.textContent).toContain("32 MB");
  });

  it("says how big the file is and how big it may be", async () => {
    renderForm(vi.fn(async () => ({ step: "claimed-and-cleared" }) as AnswerOutcome));
    const { file } = sizedFile(100 * 1024 * 1024);
    fireEvent.change(screen.getByLabelText("Kind of proof"), { target: { value: "test_report" } });
    fireEvent.change(screen.getByLabelText("Artefact"), { target: { files: [file] } });

    const alert = await screen.findByRole("alert");
    // BOTH NUMBERS. "Too large" alone leaves a person guessing which file to pick instead.
    expect(alert.textContent).toContain("100 MB");
    expect(alert.textContent).toContain("32 MB");
  });

  // THE CONTROL, and its fixture is a LITERAL for a measured reason. It was written as
  // `sizedFile(MAX_ARTEFACT_BYTES)` first, and the sabotage matrix showed it green with the bound
  // set to ZERO: the fixture followed the constant, so a guard that refuses everything sized its
  // own control down to nothing and passed. A control has to be stated independently of the thing
  // it controls for, or it measures the sabotage rather than the subject.
  it("accepts a file under the bound and reads that one", async () => {
    const onAnswer = vi.fn(async () => ({ step: "claimed-and-cleared" }) as AnswerOutcome);
    renderForm(onAnswer);
    const { file } = sizedFile(SMALL);
    fireEvent.change(screen.getByLabelText("Kind of proof"), { target: { value: "test_report" } });
    fireEvent.change(screen.getByLabelText("Artefact"), { target: { files: [file] } });

    await screen.findByText("test_report");
    expect(hash).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  // THE BOUNDARY, and the one cell that is deliberately coupled to the constant: it pins the
  // comparison as `>` rather than `>=`, so a file of exactly the declared size is accepted and
  // the next byte is not. Stating the bound twice here is the point — the two literals above
  // cannot say which side of the line the constant sits on.
  it("accepts exactly the declared size and refuses one byte more", async () => {
    const onAnswer = vi.fn(async () => ({ step: "claimed-and-cleared" }) as AnswerOutcome);
    const view = renderForm(onAnswer);
    const { file: atBound } = sizedFile(MAX_ARTEFACT_BYTES);
    fireEvent.change(screen.getByLabelText("Kind of proof"), { target: { value: "test_report" } });
    fireEvent.change(screen.getByLabelText("Artefact"), { target: { files: [atBound] } });
    await screen.findByText("test_report");
    expect(screen.queryByRole("alert")).toBeNull();

    view.unmount();
    hash.mockClear();
    renderForm(onAnswer);
    const { file: overBound, read } = sizedFile(MAX_ARTEFACT_BYTES + 1);
    fireEvent.change(screen.getByLabelText("Kind of proof"), { target: { value: "test_report" } });
    fireEvent.change(screen.getByLabelText("Artefact"), { target: { files: [overBound] } });
    await screen.findByRole("alert");
    expect(read).not.toHaveBeenCalled();
    expect(hash).not.toHaveBeenCalled();
  });
});

// -------------------------------------------------------------------------------------------
// #1187 review, second round. Three findings, three cells, each named for the reading that
// produced it rather than for the code it touches.
// -------------------------------------------------------------------------------------------

describe("AnswerNode after a clearance refusal", () => {
  // BLOCK: `drafts` was cleared for `claimed-and-cleared` and `unknown` and NOT for
  // `clearance-refused`. At that point the claim is already recorded and spent, so pressing again
  // starts a SECOND claim with the same bundle — a refusal caused by the screen, presented to the
  // person as if it were the node's answer.
  //
  // The `unknown` branch already reasoned exactly this way in its own comment ("THE CLAIM IS
  // SPENT EITHER WAY") and I did not carry it across. The two branches differ in what is known,
  // not in whether the claim was spent.
  it("treats a refused clearance as a spent claim: the bundle goes, and so does the offer to resend it", async () => {
    const onAnswer = vi.fn(
      async (_evidence: ClaimEvidence[]) =>
        ({ step: "clearance-refused", reasonCode: "hash_mismatch" }) as AnswerOutcome,
    );
    renderForm(onAnswer);
    attach("test_report", "report.txt");
    await screen.findByText("test_report");
    fireEvent.click(screen.getByRole("button", { name: "Answer this node" }));

    const alert = await screen.findByRole("alert");
    // It says the claim LANDED — the half a person cannot guess — and then what to do next.
    expect(alert.textContent).toContain("recorded");
    expect(alert.textContent).toContain("Re-open this node");
    // And it carries the Runtime's own reason rather than a generic failure.
    expect(alert.textContent).toContain("did not match what the claim recorded");
    // THE BUNDLE IS GONE, so the same submission is not on offer a second time.
    expect(screen.queryByText("test_report")).toBeNull();
    expect(onAnswer).toHaveBeenCalledTimes(1);
  });
});

describe("AnswerNode artefact count", () => {
  /** Fills the form to exactly `MAX_ARTEFACT_COUNT` artefacts. */
  async function fillToBound(): Promise<void> {
    for (let index = 0; index < MAX_ARTEFACT_COUNT; index += 1) {
      const { file } = sizedFile(SMALL);
      fireEvent.change(screen.getByLabelText("Kind of proof"), {
        target: { value: `kind_${index}` },
      });
      fireEvent.change(screen.getByLabelText("Artefact"), { target: { files: [file] } });
      await screen.findByText(`kind_${index}`);
    }
  }

  // F1: the size bound is per file and nothing bounded HOW MANY. The reviewer measured 33
  // attaches of a 30 MiB file: `hash` called 33 times, no alert, ~990 MiB read — because the only
  // count bound lived in the client and fired at the PRESS, after every byte was already read.
  it("refuses the artefact past the bound without reading it", async () => {
    const onAnswer = vi.fn(async (_evidence: ClaimEvidence[]) => ({
      step: "claimed-and-cleared",
    }) as AnswerOutcome);
    renderForm(onAnswer);
    await fillToBound();
    hash.mockClear();

    const { file, read } = sizedFile(SMALL);
    fireEvent.change(screen.getByLabelText("Kind of proof"), { target: { value: "one_too_many" } });
    fireEvent.change(screen.getByLabelText("Artefact"), { target: { files: [file] } });

    await screen.findByRole("alert");
    expect(read).not.toHaveBeenCalled();
    expect(hash).not.toHaveBeenCalled();
    expect(screen.queryByText("one_too_many")).toBeNull();
  });

  // F2: when the count bound DID fire, it fired inside `checkedEvidence` at the press, which
  // throws before any request — and the catch turned every throw into "The Runtime could not be
  // reached". The person was told the network was at fault, kept the bundle, retried, and got the
  // same sentence forever. A dead end is survivable; a dead end with a false explanation is not.
  it("names the count as the cause, and never blames the Runtime for it", async () => {
    renderForm(
      vi.fn(async (_evidence: ClaimEvidence[]) => ({ step: "claimed-and-cleared" }) as AnswerOutcome),
    );
    await fillToBound();
    const { file } = sizedFile(SMALL);
    fireEvent.change(screen.getByLabelText("Kind of proof"), { target: { value: "one_too_many" } });
    fireEvent.change(screen.getByLabelText("Artefact"), { target: { files: [file] } });

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain(String(MAX_ARTEFACT_COUNT));
    expect(alert.textContent).toContain("Remove one");
    // THE NEGATIVE HALF IS THE FINDING. Without it this cell passes on a message that names the
    // count AND blames the Runtime in the same breath.
    expect(alert.textContent).not.toContain("Runtime could not be reached");
  });

  // The control: at the bound exactly, the artefact is still taken. Without it, a guard that
  // refused everything would satisfy both cells above.
  it("accepts the artefact that reaches the bound", async () => {
    renderForm(
      vi.fn(async (_evidence: ClaimEvidence[]) => ({ step: "claimed-and-cleared" }) as AnswerOutcome),
    );
    for (let index = 0; index < MAX_ARTEFACT_COUNT - 1; index += 1) {
      const { file } = sizedFile(SMALL);
      fireEvent.change(screen.getByLabelText("Kind of proof"), {
        target: { value: `kind_${index}` },
      });
      fireEvent.change(screen.getByLabelText("Artefact"), { target: { files: [file] } });
      await screen.findByText(`kind_${index}`);
    }
    hash.mockClear();
    const { file } = sizedFile(SMALL);
    fireEvent.change(screen.getByLabelText("Kind of proof"), { target: { value: "the_last_one" } });
    fireEvent.change(screen.getByLabelText("Artefact"), { target: { files: [file] } });
    await screen.findByText("the_last_one");
    expect(hash).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("alert")).toBeNull();
  });
});

describe("AnswerNode proof-kind bounds", () => {
  // LANE B's S3: dropping the `trimmedKind === ""` half of attach's opening guard left the suite
  // GREEN, because the only cell that touched it asserted the file input's `disabled` ATTRIBUTE
  // and never the function behind it. An attribute is a hint to a person; the guard is what holds
  // when something dispatches a change anyway. This drives the function.
  it("takes no artefact when no kind has been named, whatever the attribute says", async () => {
    const onAnswer = vi.fn(async (_evidence: ClaimEvidence[]) => ({
      step: "claimed-and-cleared",
    }) as AnswerOutcome);
    renderForm(onAnswer);
    const { file, read } = sizedFile(SMALL);
    // The kind box is left empty on purpose and the change is fired at the file input directly.
    fireEvent.change(screen.getByLabelText("Artefact"), { target: { files: [file] } });
    await waitFor(() => expect(read).not.toHaveBeenCalled());
    expect(hash).not.toHaveBeenCalled();
    expect(screen.queryByRole("listitem")).toBeNull();
  });

  // LANE A: the kind's LENGTH was unbounded here while `client.ts` refuses over 128, so an
  // over-long kind was accepted, the file hashed, and the refusal arrived at the press inside
  // `checkedEvidence` — which throws, and the catch reports the Runtime as the cause while
  // keeping the bundle. The same dead end with the same false explanation as F1/F2, one bound
  // over, and this file's own comment on that fix is the argument against leaving it.
  it("refuses an over-long kind before reading the artefact, and names the length as the cause", async () => {
    renderForm(
      vi.fn(async (_evidence: ClaimEvidence[]) => ({ step: "claimed-and-cleared" }) as AnswerOutcome),
    );
    const tooLong = "k".repeat(MAX_PROOF_KIND_LENGTH + 1);
    const { file, read } = sizedFile(SMALL);
    fireEvent.change(screen.getByLabelText("Kind of proof"), { target: { value: tooLong } });
    fireEvent.change(screen.getByLabelText("Artefact"), { target: { files: [file] } });

    const alert = await screen.findByRole("alert");
    expect(read).not.toHaveBeenCalled();
    expect(hash).not.toHaveBeenCalled();
    expect(alert.textContent).toContain(String(MAX_PROOF_KIND_LENGTH + 1));
    expect(alert.textContent).toContain(String(MAX_PROOF_KIND_LENGTH));
    // THE NEGATIVE HALF IS THE WHOLE FINDING. Without it this passes on a message that names the
    // length and blames the network in the same breath, which is the state it was in.
    expect(alert.textContent).not.toContain("Runtime could not be reached");
  });

  // The control, and it is the REFUSING side that discriminates: a bound of zero would refuse
  // this too, which is what the sabotage matrix checks. Accepting exactly the bound is what
  // separates "128 is the limit" from "no kind is ever long enough".
  it("accepts a kind of exactly the bound", async () => {
    renderForm(
      vi.fn(async (_evidence: ClaimEvidence[]) => ({ step: "claimed-and-cleared" }) as AnswerOutcome),
    );
    const exact = "k".repeat(MAX_PROOF_KIND_LENGTH);
    const { file } = sizedFile(SMALL);
    fireEvent.change(screen.getByLabelText("Kind of proof"), { target: { value: exact } });
    fireEvent.change(screen.getByLabelText("Artefact"), { target: { files: [file] } });
    await screen.findByText(exact);
    expect(hash).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  // AND THE BOUND IS THE CLIENT'S. Two numbers that must not drift: if `client.ts` tightens and
  // this does not, the dead end comes back at the new difference.
  it("is the same number the client refuses on", async () => {
    const { RuntimeClient, RuntimeError } = await import("../runtime/client");
    const fetchImpl = vi.fn();
    const client = new RuntimeClient("t", {
      baseUrl: "http://runtime.test",
      fetch: fetchImpl as never,
    });
    await expect(
      client.claimNode("exec-1", {
        file: "g.yaml",
        node: "n",
        evidence: [
          {
            kind: "k".repeat(MAX_PROOF_KIND_LENGTH + 1),
            contentHash: `sha256:${"a".repeat(64)}`,
            size: 1,
          },
        ],
      }),
    ).rejects.toBeInstanceOf(RuntimeError);
    expect(fetchImpl).not.toHaveBeenCalled();
    // And one under it goes through, so this pins the boundary rather than "the client refuses".
    expect(MAX_PROOF_KIND_LENGTH).toBe(128);
  });
});
