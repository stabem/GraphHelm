//! Sealing a work outcome's free-form material (Task 6). The derivation is deterministic and
//! attempt-scoped — `exec-{execution}-{node}-a{attempt}-{suffix}` — so retries never collide
//! and Evidence identity is a pure function of the work's coordinates. Every item seals as
//! `Sensitivity::Confidential`: replies and streams are user material (the chosen variant,
//! reported per the plan's instruction — `Restricted` is the erasure-grade tier, `Internal`
//! the event-payload default; model output sits between them).

use graphhelm_events::{EvidenceError, EvidenceInput, EvidenceSealer, SealedEvidence, SecretBytes};
use graphhelm_protocols::{EvidenceReference, OpaqueId, RepositoryScope, Sensitivity};

use crate::executor::Sealable;

/// The sealed half of one recorded outcome: the encrypted items bound for the append's
/// evidence vector, and the references bound for the event's `evidence_refs` — same order as
/// the sealables they came from.
pub struct SealedWork {
    pub evidence: Vec<SealedEvidence>,
    pub references: Vec<EvidenceReference>,
}

/// Seals every item of `sealables` under the deterministic derivation, or fails without
/// side effects — the caller appends nothing on error (evidence-before-append).
///
/// # Errors
/// Any [`EvidenceError`] the sealer reports; the first failure aborts the batch.
pub async fn seal_work(
    sealer: &dyn EvidenceSealer,
    scope: &RepositoryScope,
    execution_id: &OpaqueId,
    node: &str,
    attempt: u32,
    sealables: &[Sealable],
) -> Result<SealedWork, EvidenceError> {
    let mut evidence = Vec::with_capacity(sealables.len());
    let mut references = Vec::with_capacity(sealables.len());
    for sealable in sealables {
        let local_ref = format!(
            "exec-{execution_id}-{node}-a{attempt}-{suffix}",
            suffix = sealable.local_ref_suffix
        );
        let input = EvidenceInput::new(
            local_ref,
            sealable.media_type,
            Sensitivity::Confidential,
            "standard",
            SecretBytes::new(sealable.bytes.clone()),
        )?;
        let sealed = sealer.seal(scope.clone(), input).await?;
        references.push(sealed.reference().clone());
        evidence.push(sealed);
    }
    Ok(SealedWork {
        evidence,
        references,
    })
}
