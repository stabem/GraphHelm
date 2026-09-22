//! The registry of `GHCLI` diagnostic codes (#131).
//!
//! Every code the CLI puts on the wire is declared here and nowhere else. The number was
//! allocated by grepping the tree, and grepping found 009 twice — `GHCLI009_GATEWAY_INVALID` and
//! `GHCLI009_SERVE_AUDIT_FAILED` — and, once there was a list to read, 023 twice as well. A
//! consumer matching the whole string is unaffected; a human who reads the number as an identifier,
//! sorts by it, or allocates the next one from it is not. The registry makes the next allocation a
//! lookup instead of a search, and a duplicate a red test instead of a discovery.
//!
//! The strings are wire-visible contracts (`docs/acceptance/*` carry them verbatim), so the two
//! collisions are NOT renamed here: they are named in [`KNOWN_NUMBER_COLLISIONS`], which is the
//! closed list the uniqueness test tolerates and nothing else. Renaming either side is a contract
//! change with consumers to consider, and it is a separate decision that this list keeps visible.
//!
//! `GHCLI409_PRECONDITION_FAILED` is not here: it appears only as a fixture payload in
//! `apps/cli/tests/runtime_http.rs`, is emitted by no CLI code path, and would otherwise be
//! registered by mistake as if it were one.

pub const GHCLI001_ARGUMENT_INVALID: &str = "GHCLI001_ARGUMENT_INVALID";
pub const GHCLI002_CONFIG_INVALID: &str = "GHCLI002_CONFIG_INVALID";
pub const GHCLI003_SIGNAL_INVALID: &str = "GHCLI003_SIGNAL_INVALID";
pub const GHCLI004_SIGNAL_UNRECORDABLE: &str = "GHCLI004_SIGNAL_UNRECORDABLE";
pub const GHCLI005_EXECUTION_STATE: &str = "GHCLI005_EXECUTION_STATE";
pub const GHCLI006_SERVE_INVALID: &str = "GHCLI006_SERVE_INVALID";
pub const GHCLI007_SERVE_UNAUTHORIZED: &str = "GHCLI007_SERVE_UNAUTHORIZED";
pub const GHCLI008_SERVE_NOT_FOUND: &str = "GHCLI008_SERVE_NOT_FOUND";
pub const GHCLI009_GATEWAY_INVALID: &str = "GHCLI009_GATEWAY_INVALID";
pub const GHCLI009_SERVE_AUDIT_FAILED: &str = "GHCLI009_SERVE_AUDIT_FAILED";
pub const GHCLI010_GATEWAY_CREDENTIAL: &str = "GHCLI010_GATEWAY_CREDENTIAL";
pub const GHCLI011_GATEWAY_PROBE: &str = "GHCLI011_GATEWAY_PROBE";
pub const GHCLI012_TOOL_INVALID: &str = "GHCLI012_TOOL_INVALID";
pub const GHCLI013_TOOL_DENIED: &str = "GHCLI013_TOOL_DENIED";
pub const GHCLI014_TOOL_HOST: &str = "GHCLI014_TOOL_HOST";
pub const GHCLI015_MCP_INVALID: &str = "GHCLI015_MCP_INVALID";
pub const GHCLI016_DRIVER_FAILURE: &str = "GHCLI016_DRIVER_FAILURE";
pub const GHCLI017_WAKE_INVALID: &str = "GHCLI017_WAKE_INVALID";
pub const GHCLI018_GATE_INVALID: &str = "GHCLI018_GATE_INVALID";
pub const GHCLI019_DRIVER_SETUP: &str = "GHCLI019_DRIVER_SETUP";
pub const GHCLI020_MCP_TOKEN_INVALID: &str = "GHCLI020_MCP_TOKEN_INVALID";
pub const GHCLI021_FIXTURE_ONLY_WAITING_INPUT: &str = "GHCLI021_FIXTURE_ONLY_WAITING_INPUT";
pub const GHCLI022_FIXTURE_ONLY_STATE_UNDETERMINED: &str =
    "GHCLI022_FIXTURE_ONLY_STATE_UNDETERMINED";
pub const GHCLI023_EVIDENCE_UNREADABLE: &str = "GHCLI023_EVIDENCE_UNREADABLE";
pub const GHCLI023_IDEMPOTENCY_REPLY_INVALID: &str = "GHCLI023_IDEMPOTENCY_REPLY_INVALID";
pub const GHCLI024_EXTENSION_LIFECYCLE_REFUSED: &str = "GHCLI024_EXTENSION_LIFECYCLE_REFUSED";
pub const GHCLI025_PAUSE_OUTCOME_UNKNOWN: &str = "GHCLI025_PAUSE_OUTCOME_UNKNOWN";
pub const GHCLI026_ARCHITECT_REFUSED: &str = "GHCLI026_ARCHITECT_REFUSED";
pub const GHCLI027_INIT_REFUSED: &str = "GHCLI027_INIT_REFUSED";
/// A well-formed execution id that names no stream in this store (#1083 F1). Before this code a
/// read of such an id answered success with every field null and the verdict `can_sleep`: a typo
/// told the operator the run needed nothing.
pub const GHCLI028_EXECUTION_NOT_FOUND: &str = "GHCLI028_EXECUTION_NOT_FOUND";
pub const GHCLI029_ADOPTION_REFUSED: &str = "GHCLI029_ADOPTION_REFUSED";

/// Every registered code. A code that is not in this list is not a code: the tests below refuse a
/// literal anywhere else under `apps/cli/src`, so a new allocation has to come through here. The
/// list, the tolerated pairs and `number_of` exist for the tests, which is why they are test-only:
/// the binary reads the constants above, never the list.
#[cfg(test)]
pub const ALL: &[&str] = &[
    GHCLI001_ARGUMENT_INVALID,
    GHCLI002_CONFIG_INVALID,
    GHCLI003_SIGNAL_INVALID,
    GHCLI004_SIGNAL_UNRECORDABLE,
    GHCLI005_EXECUTION_STATE,
    GHCLI006_SERVE_INVALID,
    GHCLI007_SERVE_UNAUTHORIZED,
    GHCLI008_SERVE_NOT_FOUND,
    GHCLI009_GATEWAY_INVALID,
    GHCLI009_SERVE_AUDIT_FAILED,
    GHCLI010_GATEWAY_CREDENTIAL,
    GHCLI011_GATEWAY_PROBE,
    GHCLI012_TOOL_INVALID,
    GHCLI013_TOOL_DENIED,
    GHCLI014_TOOL_HOST,
    GHCLI015_MCP_INVALID,
    GHCLI016_DRIVER_FAILURE,
    GHCLI017_WAKE_INVALID,
    GHCLI018_GATE_INVALID,
    GHCLI019_DRIVER_SETUP,
    GHCLI020_MCP_TOKEN_INVALID,
    GHCLI021_FIXTURE_ONLY_WAITING_INPUT,
    GHCLI022_FIXTURE_ONLY_STATE_UNDETERMINED,
    GHCLI023_EVIDENCE_UNREADABLE,
    GHCLI023_IDEMPOTENCY_REPLY_INVALID,
    GHCLI024_EXTENSION_LIFECYCLE_REFUSED,
    GHCLI025_PAUSE_OUTCOME_UNKNOWN,
    GHCLI026_ARCHITECT_REFUSED,
    GHCLI027_INIT_REFUSED,
    GHCLI028_EXECUTION_NOT_FOUND,
    GHCLI029_ADOPTION_REFUSED,
];

/// Numbers allocated twice BEFORE the registry existed, each pair a wire contract on both sides.
/// This is a closed list: the uniqueness test tolerates exactly these pairs and nothing else, so a
/// third collision fails, and so does a listed pair that stops colliding without this list being
/// updated in the same change (a renamed code is a decision, and the list is where it shows).
#[cfg(test)]
pub const KNOWN_NUMBER_COLLISIONS: &[(&str, &str)] = &[
    (GHCLI009_GATEWAY_INVALID, GHCLI009_SERVE_AUDIT_FAILED),
    (
        GHCLI023_EVIDENCE_UNREADABLE,
        GHCLI023_IDEMPOTENCY_REPLY_INVALID,
    ),
];

/// The numeric prefix a human reads as the identifier: `GHCLI009` of `GHCLI009_GATEWAY_INVALID`.
#[cfg(test)]
pub fn number_of(code: &str) -> &str {
    code.split('_').next().unwrap_or(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn every_code_string_is_registered_once() {
        let mut seen = BTreeMap::new();
        for code in ALL {
            *seen.entry(*code).or_insert(0usize) += 1;
        }
        let dupes: Vec<_> = seen
            .iter()
            .filter(|(_, n)| **n > 1)
            .map(|(c, _)| *c)
            .collect();
        assert!(
            dupes.is_empty(),
            "codes registered more than once: {dupes:?}"
        );
    }

    #[test]
    fn every_code_has_the_ghcli_shape() {
        for code in ALL {
            let number = number_of(code);
            assert!(
                number.len() == 8
                    && number.starts_with("GHCLI")
                    && number[5..].chars().all(|c| c.is_ascii_digit()),
                "{code} does not start with GHCLI followed by three digits"
            );
            assert!(
                code.len() > 9 && code.as_bytes()[8] == b'_',
                "{code} has no name after its number"
            );
        }
    }

    /// The #131 cell. A number allocated twice is the defect; the registry is what lets a test see
    /// it. Tolerated pairs are a CLOSED list: a third collision, or a listed pair that stops
    /// colliding without the list being updated, both fail here.
    #[test]
    fn a_number_is_allocated_once() {
        let mut by_number: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for code in ALL {
            by_number.entry(number_of(code)).or_default().push(code);
        }
        let collisions: Vec<(&str, Vec<&str>)> = by_number
            .into_iter()
            .filter(|(_, codes)| codes.len() > 1)
            .collect();
        let tolerated: Vec<Vec<&str>> = KNOWN_NUMBER_COLLISIONS
            .iter()
            .map(|(a, b)| vec![*a, *b])
            .collect();
        let unexpected: Vec<&(&str, Vec<&str>)> = collisions
            .iter()
            .filter(|(_, codes)| !tolerated.contains(codes))
            .collect();
        assert!(
            unexpected.is_empty(),
            "numbers allocated more than once, not on the tolerated list: {unexpected:?}"
        );
        for pair in &tolerated {
            assert!(
                collisions.iter().any(|(_, codes)| codes == pair),
                "{pair:?} is on KNOWN_NUMBER_COLLISIONS but no longer collides — retire it from the list in the same change"
            );
        }
    }

    /// The registry is the only place a code string may be spelled in the CLI's own source: a
    /// literal anywhere else is an allocation the list does not know about (#131's "a code
    /// assembled at runtime, defined in a doc, or added under a third directory would be
    /// invisible"). Integration tests under `apps/cli/tests` are consumers asserting the wire and
    /// are allowed their literals.
    #[test]
    fn the_registry_is_the_only_source_of_code_literals() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut offenders = Vec::new();
        let mut files_read = 0usize;
        visit(&src, &mut |path| {
            if path.file_name().and_then(|n| n.to_str()) == Some("error_codes.rs") {
                return;
            }
            files_read += 1;
            let text = std::fs::read_to_string(path).expect("source file is readable");
            for (index, line) in text.lines().enumerate() {
                if let Some(start) = line.find("\"GHCLI") {
                    let rest = &line[start + 1..];
                    let is_code = rest.len() > 8
                        && rest.as_bytes()[5..8].iter().all(u8::is_ascii_digit)
                        && rest.as_bytes()[8] == b'_';
                    if is_code {
                        offenders.push(format!(
                            "{}:{}: {}",
                            path.display(),
                            index + 1,
                            line.trim()
                        ));
                    }
                }
            }
        });
        assert!(
            files_read > 10,
            "the scan read {files_read} files — the arrangement, not the code, is wrong"
        );
        assert!(
            offenders.is_empty(),
            "GHCLI literals outside the registry ({}):\n{}",
            offenders.len(),
            offenders.join("\n")
        );
    }

    fn visit(dir: &std::path::Path, f: &mut dyn FnMut(&std::path::Path)) {
        for entry in std::fs::read_dir(dir).expect("source directory is readable") {
            let path = entry.expect("directory entry is readable").path();
            if path.is_dir() {
                visit(&path, f);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                f(&path);
            }
        }
    }
}
