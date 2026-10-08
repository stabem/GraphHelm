//! Folding journey screen captures and walked transitions into a per-contract map (spec §6.4).
//!
//! The fold is pure: it reads repository history only through [`ScopeHistory`]. [`GitHistory`] is
//! the one adapter, a read-only `git` invocation with a fixed argument list. No identifier from a
//! contract or a record reaches git as a pathspec: changed files are listed whole and filtered in
//! Rust with Keel's scope semantics ([`graphhelm_policy::keel::check_scope`]: exact path or a
//! `/`-separated prefix, no globs).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use graphhelm_policy::keel::{ChangedPath, check_scope};
use serde::{Deserialize, Serialize};

/// Longest identifier a phase-4 consumer accepts.
const MAX_ID_LEN: usize = 128;

/// True when `id` is a contract, step or screen identifier every phase-4 consumer accepts:
/// `^[a-z0-9][a-z0-9._-]{0,127}$` and no `..`. Path-like ids (`/`, `\`, `:`) are refused.
#[must_use]
pub fn valid_journey_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    let Some(first) = bytes.first() else {
        return false;
    };
    bytes.len() <= MAX_ID_LEN
        && (first.is_ascii_lowercase() || first.is_ascii_digit())
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
        && !id.contains("..")
}

/// True when `rev` is a full commit id: 40 (SHA-1) or 64 (SHA-256) lowercase hex characters.
#[must_use]
pub fn valid_revision(rev: &str) -> bool {
    matches!(rev.len(), 40 | 64)
        && rev
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// One journey contract, reduced to what the fold needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractInput {
    /// The contract id (Ruling 1).
    pub contract_id: String,
    /// The contract's human title.
    pub title: String,
    /// Steps in contract order; arrows join consecutive steps.
    pub steps: Vec<StepInput>,
}

/// One step of a contract.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepInput {
    /// The step id (Ruling 1).
    pub step_id: String,
    /// The screen behind the step, when the contract names one.
    pub screen: Option<ScreenInput>,
    /// The statements of the contract promises that name this step, in contract order.
    pub promises: Vec<String>,
    /// What the user does on this step (#379): the contract's `semanticAction` without its
    /// `input`, which may be typed text.
    pub action: Option<StepAction>,
    /// The states the step must reach (`expectedStates`), in contract order.
    pub expected_states: Vec<String>,
}

/// A step's semantic action as served (#379): its kind and the element it targets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepAction {
    /// The semantic action kind (`navigate`, `activate`, `enter_text`, ...).
    pub kind: String,
    /// The target's value: an accessible name, a label, visible text or a URL.
    pub target: String,
    /// How the target is located (the contract target strategy: `accessible_name`, `label`,
    /// `visible_text`, `stable_product_id`, ...).
    pub strategy: String,
}

/// The screen a step shows and the source paths that draw it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenInput {
    /// The screen id (Ruling 1).
    pub screen_id: String,
    /// The screen's human title.
    pub title: String,
    /// Project-relative paths; a change under any of them makes a capture stale.
    pub scope_paths: Vec<String>,
}

/// Width and height of a captured image, in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Viewport {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// One decoded `jpd.screen_captured` signal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureRecord {
    /// The signal id.
    pub signal_id: String,
    /// The execution (event stream) the signal was recorded in.
    pub execution_id: String,
    /// The event sequence within that execution; higher is newer inside one execution.
    pub sequence: u64,
    /// When the signal was recorded, in Unix milliseconds; orders records across executions.
    pub recorded_at_ms: i64,
    /// The same instant as an RFC 3339 UTC string, as served.
    pub recorded_at: String,
    /// The image evidence id carried by the signal.
    pub image_evidence_id: String,
    /// The contract the capture belongs to.
    pub contract_id: String,
    /// The step the capture shows.
    pub step_id: String,
    /// The commit the project was at when captured.
    pub revision: String,
    /// True when the working tree had uncommitted changes.
    pub dirty: bool,
    /// The image size.
    pub viewport: Viewport,
    /// The recording actor's id.
    pub observer: String,
    /// Pull request number, when given.
    pub pr: Option<u64>,
    /// `before` or `after`, when given.
    pub phase: Option<String>,
}

/// One decoded `jpd.transition_walked` signal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransitionRecord {
    /// The signal id.
    pub signal_id: String,
    /// The execution (event stream) the signal was recorded in.
    pub execution_id: String,
    /// The event sequence within that execution; higher is newer inside one execution.
    pub sequence: u64,
    /// When the signal was recorded, in Unix milliseconds; orders records across executions.
    pub recorded_at_ms: i64,
    /// The contract the transition belongs to.
    pub contract_id: String,
    /// The step walked from.
    pub from_step_id: String,
    /// The step walked to.
    pub to_step_id: String,
    /// The commit the project was at when walked.
    pub revision: String,
    /// The recording actor's id.
    pub observer: String,
    /// Signal id of the capture of the `from` step.
    pub from_capture_id: String,
    /// Signal id of the capture of the `to` step.
    pub to_capture_id: String,
}

/// What changed in the project between a capture's revision and HEAD.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChangedSince {
    /// The paths changed between the two commits, relative to the project.
    Files(Vec<String>),
    /// Git is unavailable or the project is not a repository.
    NoGit,
    /// The revision is not a commit in the repository.
    RevisionMissing,
}

/// Read access to the project's history, the fold's only side channel.
pub trait ScopeHistory {
    /// The current HEAD commit, or `None` when there is no repository or no HEAD.
    fn head(&self) -> Option<String>;
    /// The paths changed between `revision` and `head`.
    fn changed_since(&self, revision: &str, head: &str) -> ChangedSince;
}

/// [`ScopeHistory`] backed by the `git` executable run read-only in one project directory.
#[derive(Clone, Debug)]
pub struct GitHistory {
    project: PathBuf,
}

impl GitHistory {
    /// A history reader for the repository containing `project`.
    #[must_use]
    pub fn new(project: &Path) -> Self {
        Self {
            project: project.to_path_buf(),
        }
    }

    /// Runs `git -C <project> <args>` with optional locks off and stdin closed. `None` when git
    /// cannot be started.
    fn git(&self, args: &[&str]) -> Option<std::process::Output> {
        Command::new("git")
            .arg("-C")
            .arg(&self.project)
            .args(args)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .stdin(Stdio::null())
            .output()
            .ok()
    }
}

impl ScopeHistory for GitHistory {
    fn head(&self) -> Option<String> {
        let output = self.git(&["rev-parse", "--verify", "--quiet", "HEAD^{commit}"])?;
        if !output.status.success() {
            return None;
        }
        let head = String::from_utf8(output.stdout).ok()?.trim().to_owned();
        valid_revision(&head).then_some(head)
    }

    fn changed_since(&self, revision: &str, head: &str) -> ChangedSince {
        if !valid_revision(head) {
            return ChangedSince::NoGit;
        }
        if !valid_revision(revision) {
            return ChangedSince::RevisionMissing;
        }
        let object = format!("{revision}^{{commit}}");
        let Some(present) = self.git(&["cat-file", "-e", &object]) else {
            return ChangedSince::NoGit;
        };
        if !present.status.success() {
            return ChangedSince::RevisionMissing;
        }
        let Some(diff) = self.git(&[
            "diff",
            "--no-ext-diff",
            "--no-renames",
            "--relative",
            "--name-only",
            "-z",
            revision,
            head,
            "--",
        ]) else {
            return ChangedSince::NoGit;
        };
        if !diff.status.success() {
            return ChangedSince::NoGit;
        }
        ChangedSince::Files(
            diff.stdout
                .split(|byte| *byte == 0)
                .filter(|path| !path.is_empty())
                .map(|path| String::from_utf8_lossy(path).into_owned())
                .collect(),
        )
    }
}

/// Whether a capture still shows the current code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Freshness {
    /// Nothing in the screen's scope changed since the capture's revision.
    Fresh,
    /// A file in the screen's scope changed since the capture's revision.
    Stale,
    /// Freshness cannot be decided; see [`UnknownCause`].
    Unknown,
}

/// Why a capture's freshness is unknown, checked in this order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnknownCause {
    /// The capture was taken from a working tree with uncommitted changes.
    Dirty,
    /// The step has no screen or the screen has no scope paths.
    NoScopePaths,
    /// Git is unavailable, or the project is not a repository or has no HEAD.
    NoGit,
    /// The capture's revision is not in the repository.
    RevisionMissing,
}

/// The state of the arrow between two consecutive steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArrowState {
    /// The newest transition cites two fresh captures of the matching steps.
    Walked,
    /// No transition between the two steps was recorded.
    NeverWalked,
    /// The newest transition cites a capture that is missing, mismatched or not fresh.
    Stale,
}

/// The folded map of every contract.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JourneysView {
    /// HEAD at fold time, or `None` without a repository.
    pub head: Option<String>,
    /// One entry per contract, in input order.
    pub journeys: Vec<JourneyView>,
}

/// One contract's steps and arrows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JourneyView {
    /// The contract id.
    pub contract_id: String,
    /// The contract title.
    pub title: String,
    /// Steps in contract order.
    pub steps: Vec<StepView>,
    /// One arrow per consecutive step pair.
    pub arrows: Vec<ArrowView>,
}

/// One step with its screen and the capture it shows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepView {
    /// The step id.
    pub step_id: String,
    /// The screen, when the contract names one.
    pub screen: Option<ScreenView>,
    /// The capture shown (Ruling 4), when any exists.
    pub capture: Option<CaptureView>,
    /// The promise statements that name this step, in contract order.
    pub promises: Vec<String>,
    /// What the user does on this step (#379), when the contract names it.
    pub action: Option<StepAction>,
    /// The states the step must reach, in contract order.
    pub expected_states: Vec<String>,
}

/// A step's screen as served.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenView {
    /// The screen id.
    pub screen_id: String,
    /// The screen title.
    pub title: String,
    /// The scope paths.
    pub scope_paths: Vec<String>,
}

/// A capture as served, with its freshness.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureView {
    /// The capture's signal id.
    pub signal_id: String,
    /// The execution the capture was recorded in (journeys are a project property, #332).
    pub execution_id: String,
    /// The event sequence the capture was recorded at within that execution.
    pub sequence: u64,
    /// When the capture was recorded (RFC 3339 UTC).
    pub recorded_at: String,
    /// The image evidence id.
    pub image_evidence_id: String,
    /// The revision captured at.
    pub revision: String,
    /// Whether the working tree was dirty.
    pub dirty: bool,
    /// The image size.
    pub viewport: Viewport,
    /// The recording actor.
    pub observer: String,
    /// Pull request number, when given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr: Option<u64>,
    /// `before` or `after`, when given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    /// Fresh, stale or unknown.
    pub freshness: Freshness,
    /// In-scope files changed since the revision, sorted; empty unless stale.
    pub changed_files: Vec<String>,
    /// Why freshness is unknown; present only when it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unknown_cause: Option<UnknownCause>,
}

/// The arrow between two consecutive steps.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArrowView {
    /// The earlier step.
    pub from_step_id: String,
    /// The later step.
    pub to_step_id: String,
    /// Walked, never walked or stale.
    pub state: ArrowState,
    /// The newest matching transition's signal id, when any exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transition_signal_id: Option<String>,
}

/// Freshness, changed files and cause of one capture under one screen.
type Assessment = (Freshness, Vec<String>, Option<UnknownCause>);

/// Per-fold state: HEAD read once and `changed_since` memoized per revision (Ruling 7).
struct Oracle<'a> {
    history: &'a dyn ScopeHistory,
    head: Option<String>,
    memo: HashMap<String, ChangedSince>,
}

impl Oracle<'_> {
    fn assess(&mut self, capture: &CaptureRecord, screen: Option<&ScreenInput>) -> Assessment {
        if capture.dirty {
            return (Freshness::Unknown, Vec::new(), Some(UnknownCause::Dirty));
        }
        let scopes = match screen {
            Some(screen) if !screen.scope_paths.is_empty() => &screen.scope_paths,
            _ => {
                return (
                    Freshness::Unknown,
                    Vec::new(),
                    Some(UnknownCause::NoScopePaths),
                );
            }
        };
        let Some(head) = self.head.as_deref() else {
            return (Freshness::Unknown, Vec::new(), Some(UnknownCause::NoGit));
        };
        let history = self.history;
        let changed = self
            .memo
            .entry(capture.revision.clone())
            .or_insert_with(|| history.changed_since(&capture.revision, head));
        match changed {
            ChangedSince::NoGit => (Freshness::Unknown, Vec::new(), Some(UnknownCause::NoGit)),
            ChangedSince::RevisionMissing => (
                Freshness::Unknown,
                Vec::new(),
                Some(UnknownCause::RevisionMissing),
            ),
            ChangedSince::Files(files) => {
                let mut inside: Vec<String> = files
                    .iter()
                    .filter(|path| in_scope(path, scopes))
                    .cloned()
                    .collect();
                inside.sort();
                inside.dedup();
                if inside.is_empty() {
                    (Freshness::Fresh, inside, None)
                } else {
                    (Freshness::Stale, inside, None)
                }
            }
        }
    }
}

/// A path is in scope when Keel's scope rule does not report it as outside the scope paths.
fn in_scope(path: &str, scopes: &[String]) -> bool {
    let changed = [ChangedPath {
        path: path.to_owned(),
        is_new: false,
        is_deleted: false,
        plain: true,
    }];
    check_scope(&changed, scopes).is_empty()
}

fn capture_is_usable(capture: &CaptureRecord) -> bool {
    valid_journey_id(&capture.contract_id)
        && valid_journey_id(&capture.step_id)
        && valid_revision(&capture.revision)
}

fn transition_is_usable(transition: &TransitionRecord) -> bool {
    valid_journey_id(&transition.contract_id)
        && valid_journey_id(&transition.from_step_id)
        && valid_journey_id(&transition.to_step_id)
}

/// The capture a step shows (Ruling 4): the newest non-dirty one, else the newest dirty one.
fn shown_capture<'a>(
    captures: &'a [CaptureRecord],
    contract_id: &str,
    step_id: &str,
) -> Option<&'a CaptureRecord> {
    let of_step = || {
        captures.iter().filter(move |capture| {
            capture_is_usable(capture)
                && capture.contract_id == contract_id
                && capture.step_id == step_id
        })
    };
    of_step()
        .filter(|capture| !capture.dirty)
        .max_by(|a, b| capture_newness(a).cmp(&capture_newness(b)))
        .or_else(|| of_step().max_by(|a, b| capture_newness(a).cmp(&capture_newness(b))))
}

/// Newest-first ordering across executions (#332): recording time, then the in-execution
/// sequence, then the execution id so equal instants still order deterministically.
fn capture_newness(capture: &CaptureRecord) -> (i64, u64, &str) {
    (
        capture.recorded_at_ms,
        capture.sequence,
        capture.execution_id.as_str(),
    )
}

/// Folds captures and transitions into each contract's steps and arrows, with freshness read
/// through `history`. Records whose ids or revision fail validation are skipped; contracts whose
/// ids fail validation should be refused by the caller before this point.
#[must_use]
pub fn fold_journeys(
    contracts: &[ContractInput],
    captures: &[CaptureRecord],
    transitions: &[TransitionRecord],
    history: &dyn ScopeHistory,
) -> JourneysView {
    let head = history.head().filter(|head| valid_revision(head));
    let mut oracle = Oracle {
        history,
        head: head.clone(),
        memo: HashMap::new(),
    };
    let journeys = contracts
        .iter()
        .map(|contract| fold_one(contract, captures, transitions, &mut oracle))
        .collect();
    JourneysView { head, journeys }
}

fn fold_one(
    contract: &ContractInput,
    captures: &[CaptureRecord],
    transitions: &[TransitionRecord],
    oracle: &mut Oracle<'_>,
) -> JourneyView {
    let steps = contract
        .steps
        .iter()
        .map(|step| {
            let capture =
                shown_capture(captures, &contract.contract_id, &step.step_id).map(|capture| {
                    let (freshness, changed_files, unknown_cause) =
                        oracle.assess(capture, step.screen.as_ref());
                    CaptureView {
                        signal_id: capture.signal_id.clone(),
                        execution_id: capture.execution_id.clone(),
                        sequence: capture.sequence,
                        recorded_at: capture.recorded_at.clone(),
                        image_evidence_id: capture.image_evidence_id.clone(),
                        revision: capture.revision.clone(),
                        dirty: capture.dirty,
                        viewport: capture.viewport,
                        observer: capture.observer.clone(),
                        pr: capture.pr,
                        phase: capture.phase.clone(),
                        freshness,
                        changed_files,
                        unknown_cause,
                    }
                });
            StepView {
                step_id: step.step_id.clone(),
                screen: step.screen.as_ref().map(|screen| ScreenView {
                    screen_id: screen.screen_id.clone(),
                    title: screen.title.clone(),
                    scope_paths: screen.scope_paths.clone(),
                }),
                capture,
                promises: step.promises.clone(),
                action: step.action.clone(),
                expected_states: step.expected_states.clone(),
            }
        })
        .collect();
    let arrows = contract
        .steps
        .windows(2)
        .map(|pair| arrow(contract, &pair[0], &pair[1], captures, transitions, oracle))
        .collect();
    JourneyView {
        contract_id: contract.contract_id.clone(),
        title: contract.title.clone(),
        steps,
        arrows,
    }
}

fn arrow(
    contract: &ContractInput,
    from: &StepInput,
    to: &StepInput,
    captures: &[CaptureRecord],
    transitions: &[TransitionRecord],
    oracle: &mut Oracle<'_>,
) -> ArrowView {
    let newest = transitions
        .iter()
        .filter(|transition| {
            transition_is_usable(transition)
                && transition.contract_id == contract.contract_id
                && transition.from_step_id == from.step_id
                && transition.to_step_id == to.step_id
        })
        .max_by_key(|transition| {
            (
                transition.recorded_at_ms,
                transition.sequence,
                transition.execution_id.as_str(),
            )
        });
    let Some(transition) = newest else {
        return ArrowView {
            from_step_id: from.step_id.clone(),
            to_step_id: to.step_id.clone(),
            state: ArrowState::NeverWalked,
            transition_signal_id: None,
        };
    };
    // A transition cites captures by signal id, and signal ids are unique only inside one
    // execution, so a citation resolves in the transition's own execution first (#332).
    let mut cited_fresh = |capture_id: &str, step: &StepInput| {
        let matches = |capture: &&CaptureRecord| {
            capture.signal_id == capture_id
                && capture_is_usable(capture)
                && capture.contract_id == contract.contract_id
                && capture.step_id == step.step_id
        };
        captures
            .iter()
            .filter(matches)
            .find(|capture| capture.execution_id == transition.execution_id)
            .or_else(|| captures.iter().find(matches))
            .is_some_and(|capture| {
                oracle.assess(capture, step.screen.as_ref()).0 == Freshness::Fresh
            })
    };
    let walked = cited_fresh(&transition.from_capture_id, from)
        && cited_fresh(&transition.to_capture_id, to);
    ArrowView {
        from_step_id: from.step_id.clone(),
        to_step_id: to.step_id.clone(),
        state: if walked {
            ArrowState::Walked
        } else {
            ArrowState::Stale
        },
        transition_signal_id: Some(transition.signal_id.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::{valid_journey_id, valid_revision};

    #[test]
    fn journey_ids_refuse_path_like_and_malformed_values() {
        let long = "a".repeat(129);
        for bad in [
            "../x",
            "a/b",
            "a:b",
            "A",
            "",
            "a..b",
            "a\\b",
            ".a",
            long.as_str(),
        ] {
            assert!(!valid_journey_id(bad), "{bad:?} must be refused");
        }
        let longest = "a".repeat(128);
        for good in ["cart", "checkout.v2", "step-1_a", "0", longest.as_str()] {
            assert!(valid_journey_id(good), "{good:?} must be accepted");
        }
    }

    #[test]
    fn revisions_are_full_lowercase_hex_commit_ids() {
        assert!(valid_revision(&"a".repeat(40)));
        assert!(valid_revision(&"0123456789abcdef".repeat(4)));
        for bad in [
            "a".repeat(39),
            "a".repeat(41),
            "A".repeat(40),
            "g".repeat(40),
            String::new(),
            format!("{}^", "a".repeat(39)),
            "HEAD".to_owned(),
        ] {
            assert!(!valid_revision(&bad), "{bad:?} must be refused");
        }
    }
}
