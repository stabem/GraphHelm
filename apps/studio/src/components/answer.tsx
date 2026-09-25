/**
 * Answering a node that is waiting for a person.
 *
 * Two steps, never one, and the separation is the customs design rather than a UI choice: a CLAIM
 * is evidence-bearing testimony that releases nothing, and a CLEARANCE is the countersignature
 * that releases it. Collapsing them into one button would hide the state a claim leaves behind —
 * quarantined testimony, which the fold can reject — and make a rejection look like a failure of
 * the click.
 *
 * WHAT TRAVELS IS A DIGEST. The artefact a person points at is hashed here and its fingerprint is
 * sent; the bytes never leave the browser. That is what the Runtime's evidence shape asks for
 * (`{kind, contentHash, size}`) and it is why this screen can accept a file at all.
 *
 * THE SCREEN CANNOT SAY WHAT PROOF WAS ASKED FOR, and says so rather than implying it knows. The
 * topology route publishes endpoint identities only, and the status payload carries the open wait
 * but not the node's declaration, so nothing the Studio reads names the node's `proofKinds`. The
 * person names the kind; a bundle that does not satisfy the declaration is refused BY THE RUNTIME
 * with its own registry code, which this screen prints verbatim.
 */
import { useState } from "react";

import type { ClaimEvidence } from "../runtime/types";

/** What one answering attempt ended as, decided by the caller from the journal — never from an
 * HTTP status, which says "the mutation landed" for a refusal too. */
export type AnswerOutcome =
  | { step: "claimed-and-cleared" }
  | { step: "claim-refused"; reasonCode: string | null }
  | { step: "clearance-refused"; reasonCode: string | null }
  /** The claim landed and the clearance's fate could not be read back. The node may or may not be
   * released: a surface that guessed either way would be asserting something nobody measured. */
  | { step: "unknown"; claimSeq: number | null };

export interface AnswerDraft {
  kind: string;
  contentHash: string;
  size: number;
  /** The file's own name, kept for the person's benefit only. It is NOT sent: the wire shape
   * carries a digest and a size, and a name is neither. */
  label: string;
}

/**
 * The most a local artefact may weigh before this screen refuses to read it, in bytes.
 *
 * CHECKED BEFORE THE READ, and the ordering is the whole point. A digest needs the bytes, so the
 * only way to bound the work is to refuse on the one fact available WITHOUT reading: `File.size`,
 * which the browser fills from the filesystem entry. Reading first and rejecting afterwards bounds
 * the REQUEST and not the tab — a multi-gigabyte pick allocates before any limit can speak, and
 * the tab that freezes is the one the person was using to answer.
 *
 * THIRTY-TWO MEBIBYTES IS A NUMBER SOMEBODY CHOSE, said plainly rather than dressed as a
 * derivation. The artefacts this screen exists for are test reports, diffs and logs; 32 MiB is far
 * above any of them and small enough that one allocation does not stall a tab. It is NOT the
 * Runtime's 1 MiB evidence-bundle bound — that one bounds the JSON list of digests, a different
 * object — and conflating the two would refuse a legitimate report at 1 MiB.
 */
export const MAX_ARTEFACT_BYTES = 32 * 1024 * 1024;

/**
 * The most artefacts one claim may carry (#1187 review F1).
 *
 * THIRTY-TWO, chosen to equal `MAX_EVIDENCE_ITEMS` in `runtime/client.ts` rather than as a second
 * opinion about the same thing. The client's bound is the one the WIRE is held to and it fires at
 * the press; this one is the same number enforced at the point a file ARRIVES, so the count can
 * never cost a read. Two bounds, one number, and the reason they are not one bound is that they
 * guard different resources: the request's size and the tab's memory.
 */
export const MAX_ARTEFACT_COUNT = 32;

/**
 * The most characters a proof KIND may carry (#1187 review, both lanes, from two angles).
 *
 * ONE HUNDRED AND TWENTY-EIGHT, equal to `MAX_ID_LENGTH` in `runtime/client.ts` for the same
 * reason `MAX_ARTEFACT_COUNT` equals `MAX_EVIDENCE_ITEMS`: the client's bound is what the WIRE is
 * held to, and this is that number enforced where the value ARRIVES. Without it the client's
 * refusal fires at the press, inside `checkedEvidence`, which throws before any request — and the
 * catch in `submit` turns every throw into "The Runtime could not be reached" with the bundle
 * kept. That is the same dead end with the same false explanation that F1/F2 named one bound
 * over, and this file's own comment on that fix says it is worse than either half alone.
 */
export const MAX_PROOF_KIND_LENGTH = 128;

/** Bytes as whole megabytes, for a message a person reads rather than parses. Rounded UP, so a
 * file one byte over the bound never prints as exactly the bound and reads as an arbitrary
 * refusal. */
function megabytes(bytes: number): number {
  return Math.ceil(bytes / (1024 * 1024));
}

/** The refusal vocabulary, in words a person can act on. An unknown code prints as itself rather
 * than as a generic failure: the registry is meant to be closed, and a code this list has not
 * caught up with is still more useful than "something went wrong". */
const REFUSAL_WORDS: Record<string, string> = {
  not_waiting: "That node is not waiting for anyone right now.",
  unknown_wait: "The wait this answers is not open any more.",
  stale_rendezvous: "The node parked again since this screen read it — re-open it and answer the new wait.",
  duplicate_completion: "Someone already claimed this node and it is waiting to be cleared.",
  evidence_budget_unmet: "The node asked for proof this bundle does not carry.",
  hash_mismatch: "The clearance did not match what the claim recorded.",
  unknown_identity: "The countersigner is not registered for this graph.",
};

export function refusalWords(reasonCode: string | null): string {
  if (reasonCode === null) return "The Runtime refused it and recorded no reason code.";
  return REFUSAL_WORDS[reasonCode] ?? reasonCode;
}

/**
 * @param node the parked node this answers.
 * @param waitSeq the sequence of the wait being answered, or `null` when the status carries none.
 *   A null is NOT sent as "let the Runtime pick": the form refuses instead, because answering
 *   whichever wait happens to be open is exactly the stale rendezvous the sequence prevents.
 * @param onAnswer claims and clears, and returns what the journal said.
 * @param hash how a file becomes `{contentHash, size}`. Injected so a test can drive this form
 *   without a real SubtleCrypto and without asserting a hash it computed itself.
 */
export function AnswerNode({
  node,
  waitSeq,
  onAnswer,
  hash,
}: {
  node: string;
  waitSeq: number | null;
  onAnswer: (evidence: ClaimEvidence[]) => Promise<AnswerOutcome>;
  hash: (file: File) => Promise<{ contentHash: string; size: number }>;
}) {
  const [drafts, setDrafts] = useState<AnswerDraft[]>([]);
  const [kind, setKind] = useState("");
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<{ tone: "good" | "bad"; text: string } | null>(null);

  const trimmedKind = kind.trim();

  async function attach(file: File | null): Promise<void> {
    if (file === null || trimmedKind === "") return;
    setNotice(null);
    // BEFORE `hash`, which is the thing that reads the bytes. The bound lives here, where the
    // file first arrives, rather than inside the hashing function: a guard downstream of the
    // read is not a guard, it is a report.
    if (file.size > MAX_ARTEFACT_BYTES) {
      setNotice({
        tone: "bad",
        text:
          `That file is ${megabytes(file.size)} MB and this screen reads at most ` +
          `${megabytes(MAX_ARTEFACT_BYTES)} MB. Point at the report itself rather than an ` +
          `archive, or hash it yourself and claim from the CLI.`,
      });
      return;
    }
    // #1187 review F1: the SIZE bound is per file and nothing bounded HOW MANY. Measured by the
    // reviewer: 33 attaches of a 30 MiB file read and hashed ~990 MiB with no alert, because the
    // only count bound lived in `client.ts` and fired at the press, after every byte was already
    // read. This one is checked HERE, before the read, for the same reason the size bound is.
    //
    // #1187 review F2: and it names its own cause. `checkedEvidence` throws before any request,
    // and the catch below turns every throw into "The Runtime could not be reached" — so the
    // person was told the network was at fault, kept the bundle, retried, and got the same
    // sentence forever. A dead end with a false explanation is worse than either half alone.
    // BEFORE THE READ, like the other two. A kind the client will refuse is refused here, where
    // the person can still fix it and where nothing has been hashed yet.
    if (trimmedKind.length > MAX_PROOF_KIND_LENGTH) {
      setNotice({
        tone: "bad",
        text:
          `That kind of proof is ${trimmedKind.length} characters and the most one may carry is ` +
          `${MAX_PROOF_KIND_LENGTH}. Shorten it before adding the artefact.`,
      });
      return;
    }
    if (drafts.length >= MAX_ARTEFACT_COUNT) {
      setNotice({
        tone: "bad",
        text:
          `This answer already carries ${MAX_ARTEFACT_COUNT} artefacts, which is the most one ` +
          `claim may present. Remove one before adding another.`,
      });
      return;
    }
    try {
      const { contentHash, size } = await hash(file);
      setDrafts((held) => [...held, { kind: trimmedKind, contentHash, size, label: file.name }]);
      setKind("");
    } catch {
      setNotice({ tone: "bad", text: "That file could not be read." });
    }
  }

  async function submit(): Promise<void> {
    // #1187 review: `drafts.length === 0` used to refuse here, which made a node whose declared
    // `proofKinds` is EMPTY permanently unanswerable from this screen while the Runtime accepts
    // an empty bundle for it and can clear the claim. A node that reaches `WaitingInput` through
    // an executor `NeedsInput` outcome declares no proof kinds at all, so that was not a corner:
    // it is every node the fixture path parks.
    //
    // THE RUNTIME STAYS AUTHORITATIVE. This screen does not decide whether proof is required; it
    // sends what the person assembled and prints what the journal answered. A node that DOES
    // require proof refuses the empty claim with `evidence_budget_unmet`, which costs the
    // claimant their testimony and not their turn — the wait survives and can be claimed again.
    if (busy || waitSeq === null) return;
    setBusy(true);
    setNotice(null);
    try {
      const outcome = await onAnswer(
        drafts.map((draft) => ({ kind: draft.kind, contentHash: draft.contentHash, size: draft.size })),
      );
      if (outcome.step === "claimed-and-cleared") {
        setDrafts([]);
        setNotice({ tone: "good", text: "Answered. The node is released." });
      } else if (outcome.step === "unknown") {
        // THE CLAIM IS SPENT EITHER WAY. Keeping the drafts would invite a second claim against a
        // wait that already has one, which the fold refuses as a duplicate — a worse message than
        // the truth.
        setDrafts([]);
        setNotice({
          tone: "bad",
          text: "The answer was sent and we could not read back what happened to it. Re-open this node to see where it stands.",
        });
      } else if (outcome.step === "clearance-refused") {
        // #1187 review: THE CLAIM IS SPENT HERE TOO, and this branch used to keep the drafts.
        // A clearance refusal means the claim was RECORDED and then the countersignature was
        // withheld; pressing again starts a SECOND claim with the same bundle, which the fold
        // answers with a refusal of its own and which hides what actually happened. The `unknown`
        // branch above already reasoned this way and I did not carry it across.
        setDrafts([]);
        setNotice({
          tone: "bad",
          text: `Your claim was recorded, but the clearance was refused: ${refusalWords(
            outcome.reasonCode,
          )} Re-open this node to see where it stands before answering again.`,
        });
      } else {
        setNotice({ tone: "bad", text: refusalWords(outcome.reasonCode) });
      }
    } catch {
      setNotice({ tone: "bad", text: "The Runtime could not be reached." });
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="answer" aria-label={`Answer ${node}`}>
      <h3>This node is waiting for you</h3>
      {waitSeq === null ? (
        <p className="notice bad" role="alert">
          This node is waiting, but the wait it is holding open could not be read, so there is
          nothing this screen can safely answer.
        </p>
      ) : (
        <>
          <p className="hint">
            Point at what shows the work happened. The file stays on your machine — only its
            fingerprint and size are sent.
          </p>
          <div className="answer-add">
            <label>
              <span className="lbl">Kind of proof</span>
              <input
                type="text"
                value={kind}
                onChange={(change) => setKind(change.target.value)}
                placeholder="test_report"
                aria-label="Kind of proof"
              />
            </label>
            <label>
              <span className="lbl">Artefact</span>
              <input
                type="file"
                aria-label="Artefact"
                disabled={trimmedKind === ""}
                onChange={(change) => {
                  void attach(change.target.files?.[0] ?? null);
                  change.target.value = "";
                }}
              />
            </label>
          </div>
          {drafts.length > 0 && (
            <ul className="answer-bundle">
              {drafts.map((draft, index) => (
                <li key={`${draft.kind}-${draft.contentHash}-${index}`}>
                  <strong>{draft.kind}</strong>
                  <span className="lbl">{draft.label}</span>
                  {/* The digest is shown SHORT but is what was computed — a person comparing it
                    * against their own `sha256sum` needs the real leading characters, not a
                    * decorative stand-in. */}
                  <code>{draft.contentHash.slice(0, 19)}…</code>
                  <button
                    type="button"
                    className="ghost"
                    aria-label={`Remove ${draft.label}`}
                    onClick={() => setDrafts((held) => held.filter((_, at) => at !== index))}
                  >
                    Remove
                  </button>
                </li>
              ))}
            </ul>
          )}
          {/* #1187 review: enabled with an EMPTY bundle, because a node whose declared proofKinds
            * is empty is answerable by the Runtime and was unanswerable here. The LABEL changes
            * with the bundle rather than the button disappearing: a person pressing "Answer with
            * no evidence" has been told what they are about to send, and a person who meant to
            * attach something sees that they have not. The Runtime decides whether that is
            * enough; this screen only stops being the thing that prevents asking. */}
          <button type="button" disabled={busy} onClick={() => void submit()}>
            {busy
              ? "Answering…"
              : drafts.length === 0
                ? "Answer with no evidence"
                : "Answer this node"}
          </button>
        </>
      )}
      {notice !== null && (
        <p className={`notice ${notice.tone}`} role="alert">
          {notice.text}
        </p>
      )}
    </section>
  );
}
