use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use graphhelm_policy::keel::{self as policy_keel, Card, KeelPolicy};
use graphhelm_policy::keel_prove::{self, ProveOptions};
use graphhelm_process_tree::{ProcessGroup, TerminationOutcome};
use graphhelm_protocols::Diagnostic;

use crate::output::{CommandOutput, Outcome};

const COMMAND: &str = "keel";

/// The shipped Keel rules, embedded so the verdict names the policy version the binary was built
/// with and never depends on where the package is installed. The same bytes are the
/// `policy/keel` contribution `extension.json` binds by digest.
const KEEL_POLICY: &str = include_str!(
    "../../../../extensions/builtin/graphhelm-development-contracts/policies/keel.yaml"
);

/// A card is a few hundred bytes (`keel.yaml` `card.maxCardBytes`); this only stops an unbounded
/// read. A card between the two is read and reported as `keel.card.too_large`.
const MAX_CARD_READ_BYTES: u64 = 1024 * 1024;

const INPUT_INVALID: &str = crate::error_codes::GHCLI031_KEEL_CHECK_INPUT;

pub(super) fn run(operation: keel_contract_index::Operation) -> Outcome {
    match keel_contract_index::execute_public(operation) {
        Ok(value) => Outcome::success(COMMAND, value),
        Err(error) => Outcome::domain(
            COMMAND,
            vec![Diagnostic::error(
                error.code,
                error.message,
                "/keel",
                "keel",
            )],
        ),
    }
}

fn input_error(message: impl Into<String>, path: &str) -> Outcome {
    Outcome::application(
        COMMAND,
        Diagnostic::error(INPUT_INVALID, message, path, "keel"),
    )
}

/// `--prove-new-tests` and its bounds (#1333).
pub(super) struct ProveArgs {
    pub(super) target_dir: Option<PathBuf>,
    pub(super) timeout_secs: u64,
}

/// `graphhelm keel check --diff <base>..<head> [--card <card.json>] [--repo <dir>]
/// [--prove-new-tests]` (#1330, #1333).
///
/// Exit 0 when nothing blocks (signals travel as warnings), 2 when a finding blocks (the report
/// still travels in `data`), 3 when the range, the repository or the card cannot be read, or the
/// proving worktrees cannot be set up.
pub(super) fn check(
    repo: &Path,
    range: &str,
    card_path: Option<&Path>,
    prove: Option<ProveArgs>,
) -> Outcome {
    if range.starts_with('-') || !range.contains("..") {
        return input_error("--diff takes a git range `<base>..<head>`", "/diff");
    }
    let card = match card_path.map(read_card).transpose() {
        Ok(card) => card,
        Err(outcome) => return *outcome,
    };
    let output = match Command::new("git")
        .arg("-C")
        .arg(repo)
        .args([
            "-c",
            "core.quotePath=false",
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--find-renames",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            range,
            "--",
        ])
        .output()
    {
        Ok(output) => output,
        Err(error) => return input_error(format!("git did not start: {error}"), "/repo"),
    };
    if !output.status.success() {
        return input_error(
            format!(
                "git diff {range} failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
            "/diff",
        );
    }
    let diff = String::from_utf8_lossy(&output.stdout);
    let policy: KeelPolicy = match serde_yaml_ng::from_str(KEEL_POLICY) {
        Ok(policy) => policy,
        Err(error) => {
            return Outcome::internal(COMMAND, format!("shipped keel.yaml unreadable: {error}"));
        }
    };
    let report = policy_keel::check(
        &diff,
        card.as_ref().map(|(card, bytes)| (card, *bytes)),
        &policy,
    );
    let proof = match prove {
        None => None,
        Some(args) => {
            let (base, head) = range
                .split_once("...")
                .or_else(|| range.split_once(".."))
                .unwrap_or((range, "HEAD"));
            let scratch_root = std::env::temp_dir();
            let target_dir = args
                .target_dir
                .or_else(|| std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from))
                .unwrap_or_else(|| scratch_root.join("graphhelm-keel-prove-target"));
            let options = ProveOptions {
                repo: repo.to_path_buf(),
                base: if base.is_empty() { "HEAD" } else { base }.to_owned(),
                head: if head.is_empty() { "HEAD" } else { head }.to_owned(),
                target_dir,
                scratch_root,
                timeout: Duration::from_secs(args.timeout_secs),
            };
            match keel_prove::prove_new_tests(&diff, &options, run_bounded) {
                Ok(proof) => Some(proof),
                Err(error) => {
                    return input_error(format!("--prove-new-tests: {error}"), "/diff");
                }
            }
        }
    };
    let diagnostics: Vec<Diagnostic> = report
        .findings
        .iter()
        .chain(proof.iter().flat_map(|proof| proof.findings.iter()))
        .map(|finding| {
            let path = finding.path.as_deref().unwrap_or("/");
            let message = format!("{}: {}", finding.rule, finding.detail);
            if finding.blocking {
                Diagnostic::error(finding.rule.clone(), message, path, "keel")
            } else {
                Diagnostic::warning(finding.rule.clone(), message, path, "keel")
            }
        })
        .collect();
    let refused = report.refused;
    let mut data = match serde_json::to_value(&report) {
        Ok(data) => data,
        Err(error) => return Outcome::internal(COMMAND, error.to_string()),
    };
    if let (Some(proof), Some(object)) = (proof, data.as_object_mut()) {
        match serde_json::to_value(proof) {
            Ok(value) => {
                object.insert("testProof".into(), value);
            }
            Err(error) => return Outcome::internal(COMMAND, error.to_string()),
        }
    }
    Outcome {
        output: CommandOutput {
            ok: !refused,
            command: COMMAND,
            data: Some(data),
            diagnostics,
        },
        exit_code: if refused { 2 } else { 0 },
    }
}

type ProveOutput = keel_prove::ProveOutput;

/// Runs one proof command with a process group/job and one execution deadline. Cleanup has its
/// own bounded observer because terminating a Windows job is asynchronous.
fn run_bounded(mut command: Command, timeout: Duration) -> Result<Option<ProveOutput>, String> {
    graphhelm_process_tree::configure(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("cargo did not start: {error}"))?;
    let mut group = match graphhelm_process_tree::create(&child) {
        Ok(group) => group,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("process tree setup failed: {error}"));
        }
    };
    let readers = [child.stdout.take(), child.stderr.take()].map(|pipe| {
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut bytes);
            }
            let _ = sender.send(String::from_utf8_lossy(&bytes).into_owned());
        });
        receiver
    });
    let deadline = Instant::now() + timeout;
    loop {
        match graphhelm_process_tree::leader_exited(&mut child) {
            Ok(true) => {
                let read_to_deadline = |reader: &Receiver<String>| match reader
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                {
                    Ok(output) => Some(output),
                    Err(mpsc::RecvTimeoutError::Disconnected) => Some(String::new()),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                };
                let out = read_to_deadline(&readers[0]);
                let err = out.as_ref().and_then(|_| read_to_deadline(&readers[1]));
                if let (Some(out), Some(err)) = (out, err) {
                    let succeeded = cleanup_process_tree(&mut child, &mut group, true)?;
                    return Ok(Some((succeeded, out, err)));
                }
                cleanup_process_tree(&mut child, &mut group, true)?;
                return Ok(None);
            }
            Ok(false) if Instant::now() >= deadline => {
                cleanup_process_tree(&mut child, &mut group, false)?;
                return Ok(None);
            }
            Ok(false) => std::thread::sleep(Duration::from_millis(10)),
            Err(error) => {
                cleanup_process_tree(&mut child, &mut group, false)?;
                return Err(format!("waiting on cargo failed: {error}"));
            }
        }
    }
}

fn cleanup_process_tree(
    child: &mut std::process::Child,
    group: &mut ProcessGroup,
    leader_already_exited: bool,
) -> Result<bool, String> {
    let outcome = graphhelm_process_tree::terminate(child.id(), *group);
    graphhelm_process_tree::close(group);
    let succeeded = reap_leader(child, leader_already_exited)?;
    match outcome {
        TerminationOutcome::Complete => Ok(succeeded),
        other => Err(format!("process-tree cleanup inconclusive: {other:?}")),
    }
}

fn reap_leader(
    child: &mut std::process::Child,
    leader_already_exited: bool,
) -> Result<bool, String> {
    #[cfg(windows)]
    if leader_already_exited {
        return Ok(true);
    }
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        if graphhelm_process_tree::leader_exited(child)
            .map_err(|error| format!("reaping cargo failed: {error}"))?
        {
            #[cfg(unix)]
            return child
                .wait()
                .map(|status| status.success())
                .map_err(|error| format!("reaping cargo failed: {error}"));
            #[cfg(windows)]
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Err("process-tree cleanup could not reap cargo within 1s".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn read_card(path: &Path) -> Result<(Card, u64), Box<Outcome>> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| Box::new(input_error(format!("card unreadable: {error}"), "/card")))?;
    if metadata.len() > MAX_CARD_READ_BYTES {
        return Err(Box::new(input_error(
            format!("card is {} bytes; not read", metadata.len()),
            "/card",
        )));
    }
    let bytes = std::fs::read(path)
        .map_err(|error| Box::new(input_error(format!("card unreadable: {error}"), "/card")))?;
    let card: Card = serde_json::from_slice(&bytes).map_err(|error| {
        Box::new(input_error(
            format!("card is not a keel card (keel-card.schema.json): {error}"),
            "/card",
        ))
    })?;
    Ok((card, bytes.len() as u64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn bounded_proof_kills_a_descendant_that_holds_an_output_pipe() {
        let scratch = tempfile::tempdir().unwrap();
        let pid_path = scratch.path().join("descendant.pid");
        let mut command = if cfg!(windows) {
            let mut command = Command::new("powershell");
            command.args([
                "-NoProfile",
                "-Command",
                "$p=Start-Process ping -ArgumentList '-n','30','127.0.0.1' -PassThru; Set-Content -LiteralPath $env:GRAPHHELM_TEST_PID_FILE -Value $p.Id",
            ]);
            command
        } else {
            let mut command = Command::new("sh");
            command.args([
                "-c",
                "sleep 30 & echo $! > \"$GRAPHHELM_TEST_PID_FILE\"; exit 0",
            ]);
            command
        };
        command
            .env("GRAPHHELM_TEST_PID_FILE", &pid_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let worker = thread::spawn(move || run_bounded(command, Duration::from_millis(100)));
        let identity = (0..200).find_map(|_| {
            let pid = std::fs::read_to_string(&pid_path)
                .ok()?
                .trim()
                .parse()
                .ok()?;
            graphhelm_process_tree::ProcessIdentity::capture(pid).ok()
        });
        let result = worker.join().unwrap().unwrap();

        assert!(result.is_none(), "inherited pipe bypassed the deadline");
        let identity = identity.expect("descendant identity was not observed");
        assert!(
            identity.wait_until_gone(Duration::from_secs(1)).unwrap(),
            "timed out process tree still has a live descendant"
        );
    }
}
