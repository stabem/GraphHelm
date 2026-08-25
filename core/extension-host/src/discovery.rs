//! Resolving the canonical GraphHelm executable for host adapters.

use crate::activation::ActivationRecord;
use std::path::PathBuf;

/// Why an executable could not be resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscoveryRefusal {
    /// No canonical executable was recorded when this version was installed.
    NoRecordedExecutable,
    /// The activation state names more than one executable and disagrees with itself.
    AmbiguousRecord,
}

/// Resolve the canonical executable for an activated extension.
///
/// # Errors
///
/// Returns [`DiscoveryRefusal`] when no executable can be resolved.
pub fn resolve_executable(record: &ActivationRecord) -> Result<PathBuf, DiscoveryRefusal> {
    // Discovery is a READ of what installation recorded. It is not a search.
    //
    // There is no fallback on purpose. "Look next to the package" is the fallback anyone writes,
    // it makes the feature appear to work, and the package is attacker-supplied by definition --
    // so the convenient branch is the one that hands execution to whoever authored the package.
    // PATH is refused for the same reason one step further out: it resolves to whatever the
    // environment says today, which is not what was verified at install time.
    // Two refusals, not one. "Nothing was recorded" and "the record names several" call for
    // opposite actions -- install the extension, versus stop and repair the state -- and a single
    // code would flatten them into a caller that cannot tell which it is looking at.
    //
    // Picking with `.first()` is the shape that makes a broken activation state look like a working
    // one, and it picks by list order, which is not a decision anyone made.
    match record.recorded_executables() {
        [] => Err(DiscoveryRefusal::NoRecordedExecutable),
        [only] => Ok(only.clone()),
        _ => Err(DiscoveryRefusal::AmbiguousRecord),
    }
}
