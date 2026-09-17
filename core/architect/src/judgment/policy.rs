//! Named thresholds (spec D6). These are VALUES TO BE MEASURED, chosen conservatively; a change
//! needs a recorded Jev run under `docs/acceptance/` and a cell on each side of the new value.

/// A `Choice`/`Score` answer acts on the compiler only at or above this confidence.
pub const ACT_THRESHOLD: f64 = 0.80;
/// A `Noul` at or below this is read as "no"; between the two thresholds it is unresolved.
pub const NOUL_NO_THRESHOLD: f64 = 0.35;
/// A `Noul` at or above this is read as "yes".
pub const NOUL_YES_THRESHOLD: f64 = 0.65;

/// Whether a `Choice`/`Score` confidence is high enough to act on.
#[must_use]
pub fn acts(confidence: f64) -> bool {
    confidence >= ACT_THRESHOLD
}

/// Whether a `Noul` probability reads as "no".
#[must_use]
pub fn noul_is_no(probability: f64) -> bool {
    probability <= NOUL_NO_THRESHOLD
}

/// Whether a `Noul` probability reads as "yes".
#[must_use]
pub fn noul_is_yes(probability: f64) -> bool {
    probability >= NOUL_YES_THRESHOLD
}
