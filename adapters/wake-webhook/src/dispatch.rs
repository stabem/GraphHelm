//! #2714 slice 3: reads the per-lane webhook config and secret files, delivers a step once, and
//! keeps an append-only delivery log beside the events directory. The journal is never written.
//!
//! Layout under the project's `.graphhelm/` directory (all gitignored, owner-made):
//! `wake-webhooks.json` (lane -> url), `wake-secrets/<lane>.secret` (the signing secret, nothing
//! else) and `wake-webhook.log` (one line per delivery: time, delivery id, outcome code; never a
//! url, a secret or a payload). A lane without a url or a secret is off, only for that lane.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::{Config, DEADLINE, Outcome, Secret, Step, Ureq, deliver, delivery_id};

const CONFIG_FILE: &str = "wake-webhooks.json";
const SECRETS_DIR: &str = "wake-secrets";
const LOG_FILE: &str = "wake-webhook.log";
/// `OpenOptions` flags, named: the log is created when missing and only ever appended to.
const CREATE: bool = true;
const APPEND: bool = true;
/// A delivery that failed (a 503 that outlived its retries, a timeout, a refused connection) is
/// tried again on a later pass, no sooner than `FIRST_BACKOFF_SECS` doubled for each earlier
/// failure, and given up after `MAX_FAILED_PASSES` failures so one dead lane cannot hold the rest.
const MAX_FAILED_PASSES: u64 = 5;
const FIRST_BACKOFF_SECS: u64 = 30;

/// Sends ready steps to their lane's webhook, once each.
pub struct Dispatcher {
    dir: PathBuf,
    config: Config,
    client: Ureq,
}

/// A lane name is a file name: letters, digits, `-`, `_` and `.` only, so it cannot name a path.
fn safe_lane(lane: &str) -> bool {
    !lane.is_empty()
        && !lane.starts_with('.')
        && lane
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn outcome_code(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Accepted => "accepted".to_owned(),
        Outcome::Duplicate => "duplicate".to_owned(),
        Outcome::Rejected(code) => format!("rejected-{code}"),
        Outcome::Failed(_) => "failed".to_owned(),
        Outcome::GaveUp => "gave-up".to_owned(),
    }
}

fn clock() -> (u64, String) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO);
    let iso = chrono::DateTime::from_timestamp(now.as_secs() as i64, 0)
        .map_or_else(String::new, |at| {
            at.format("%Y-%m-%dT%H:%M:%SZ").to_string()
        });
    (now.as_secs(), iso)
}

fn pause(attempt: u32) {
    std::thread::sleep(Duration::from_millis(200 * 2_u64.pow(attempt)));
}

impl Dispatcher {
    /// The dispatcher for the project directory `dir` (the parent of the events directory), or
    /// `None` when there is no config file or it is not a lane -> https url map on the pinned host.
    pub fn load(dir: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(dir.join(CONFIG_FILE)).ok()?;
        let config = Config::parse(&text).ok()?;
        Some(Self {
            dir: dir.to_owned(),
            config,
            client: Ureq::new(DEADLINE),
        })
    }

    /// True when `step`'s delivery id is already in the log as final (delivered, or given up).
    fn delivered(&self, id: &str) -> bool {
        std::fs::read_to_string(self.dir.join(LOG_FILE)).is_ok_and(|log| {
            log.lines().any(|line| {
                let mut parts = line.split_whitespace();
                parts.nth(1) == Some(id)
                    && matches!(parts.next(), Some("accepted" | "duplicate" | "gave-up"))
            })
        })
    }

    /// How many times `id` failed so far, and the unix time of the last failure.
    fn failures(&self, id: &str) -> (u64, u64) {
        let log = std::fs::read_to_string(self.dir.join(LOG_FILE)).unwrap_or_default();
        let mut count = 0;
        let mut last = 0;
        for line in log.lines() {
            let mut parts = line.split_whitespace();
            let (Some(at), Some(line_id), Some("failed")) =
                (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            if line_id == id {
                count += 1;
                last = chrono::DateTime::parse_from_rfc3339(at).map_or(last, |moment| {
                    u64::try_from(moment.timestamp()).unwrap_or(last)
                });
            }
        }
        (count, last)
    }

    fn secret(&self, lane: &str) -> Option<Secret> {
        if !safe_lane(lane) {
            return None;
        }
        let text =
            std::fs::read_to_string(self.dir.join(SECRETS_DIR).join(format!("{lane}.secret")))
                .ok()?;
        let secret = Secret::new(text);
        (!secret.is_empty()).then_some(secret)
    }

    fn log(&self, id: &str, code: &str) {
        let line = format!("{} {} {}\n", clock().1, id, code);
        // Local state only: a log that cannot be written never fails the sweep, but it is said
        // (the error kind only: no path, no content).
        let written = std::fs::OpenOptions::new()
            .create(CREATE)
            .append(APPEND)
            .open(self.dir.join(LOG_FILE))
            .and_then(|mut file| file.write_all(line.as_bytes()));
        if let Err(error) = written {
            eprintln!("wake webhook: delivery log not written ({})", error.kind());
        }
    }

    /// Writes one line for something that made no step (`what` is a label, `code` the reason).
    pub fn log_skip(&self, what: &str, code: &str) {
        self.log(what, code);
    }

    /// Delivers `step` to its lane once. `None` when the lane is off (no url or no secret: logged
    /// as skipped) or the step was already delivered; else the outcome.
    pub fn notify(&self, step: &Step) -> Option<Outcome> {
        self.notify_with(step, &|client, url, secret, step| {
            deliver(client, url, secret, step, &clock, &pause)
        })
    }

    fn notify_with(
        &self,
        step: &Step,
        send: &dyn Fn(&Ureq, &str, &Secret, &Step) -> Outcome,
    ) -> Option<Outcome> {
        let id = delivery_id(step);
        // A lane that is off says so in the log (the answer to "why did it not wake?").
        let Some(url) = self.config.url(&step.lane) else {
            self.log(&id, "skipped-no-url");
            return None;
        };
        let Some(secret) = self.secret(&step.lane) else {
            self.log(&id, "skipped-no-secret");
            return None;
        };
        if self.delivered(&id) {
            return None;
        }
        let (failures, last_failure) = self.failures(&id);
        if failures >= MAX_FAILED_PASSES {
            self.log(&id, &outcome_code(&Outcome::GaveUp));
            return Some(Outcome::GaveUp);
        }
        if failures > 0
            && clock().0 < last_failure + FIRST_BACKOFF_SECS * 2_u64.pow(failures as u32 - 1)
        {
            // Still backing off: nothing is sent and nothing is logged; the caller holds its place.
            return Some(Outcome::Failed("backing off".to_owned()));
        }
        let outcome = send(&self.client, url, &secret, step);
        self.log(&id, &outcome_code(&outcome));
        Some(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Reason, deliver_to};
    use std::collections::BTreeMap;

    fn step(lane: &str) -> Step {
        Step {
            lane: lane.into(),
            step: "review".into(),
            skill: "blind-review".into(),
            issue: 2714,
            pr: Some(2750),
            head: "abcdef12".into(),
            because: Reason::ReviewAssigned,
        }
    }

    fn dispatcher(dir: &Path, lanes: &[&str]) -> Dispatcher {
        let urls: BTreeMap<String, String> = lanes
            .iter()
            .map(|lane| ((*lane).to_owned(), "http://127.0.0.1:9/x".to_owned()))
            .collect();
        Dispatcher {
            dir: dir.to_owned(),
            config: Config { urls },
            client: Ureq::new(Duration::from_millis(500)),
        }
    }

    fn secret_for(dir: &Path, lane: &str, value: &str) {
        std::fs::create_dir_all(dir.join(SECRETS_DIR)).unwrap();
        std::fs::write(dir.join(SECRETS_DIR).join(format!("{lane}.secret")), value).unwrap();
    }

    /// The test sender skips the host pin (a loopback receiver is not on it) but is the real
    /// retry/sign code; the answers are scripted.
    fn scripted(
        code: u16,
        seen: &std::cell::RefCell<Vec<String>>,
    ) -> impl Fn(&Ureq, &str, &Secret, &Step) -> Outcome + '_ {
        move |_client, _url, secret, step| {
            struct Fixed(u16);
            impl crate::Transport for Fixed {
                fn post(&self, _u: &str, _h: &[(&str, String)], _b: &str) -> Result<u16, String> {
                    Ok(self.0)
                }
            }
            seen.borrow_mut().push(delivery_id(step));
            deliver_to(
                &Fixed(code),
                "u",
                secret,
                step,
                &|| (1, "t".into()),
                &|_| {},
            )
        }
    }

    #[test]
    fn a_lane_without_a_url_or_a_secret_is_off_and_the_log_says_why() {
        let dir = tempfile::tempdir().unwrap();
        let seen = std::cell::RefCell::new(vec![]);
        let d = dispatcher(dir.path(), &["openbot-neji"]);
        // no secret file for openbot-neji, and no url for openbot-sai
        assert_eq!(
            d.notify_with(&step("openbot-neji"), &scripted(202, &seen)),
            None
        );
        secret_for(dir.path(), "openbot-sai", "s");
        assert_eq!(
            d.notify_with(&step("openbot-sai"), &scripted(202, &seen)),
            None
        );
        assert!(seen.borrow().is_empty());
        let log = std::fs::read_to_string(dir.path().join(LOG_FILE)).unwrap();
        assert!(
            log.contains("openbot-neji:review:abcdef12 skipped-no-secret"),
            "{log}"
        );
        assert!(
            log.contains("openbot-sai:review:abcdef12 skipped-no-url"),
            "{log}"
        );
    }

    #[test]
    fn a_step_is_delivered_once_and_logged_without_url_or_secret() {
        let dir = tempfile::tempdir().unwrap();
        secret_for(dir.path(), "openbot-neji", "hunter2\n");
        let seen = std::cell::RefCell::new(vec![]);
        let d = dispatcher(dir.path(), &["openbot-neji"]);
        assert_eq!(
            d.notify_with(&step("openbot-neji"), &scripted(202, &seen)),
            Some(Outcome::Accepted)
        );
        assert_eq!(
            d.notify_with(&step("openbot-neji"), &scripted(202, &seen)),
            None,
            "once per step"
        );
        assert_eq!(seen.borrow().len(), 1);
        let log = std::fs::read_to_string(dir.path().join(LOG_FILE)).unwrap();
        assert!(
            log.contains("openbot-neji:review:abcdef12 accepted"),
            "{log}"
        );
        assert!(
            !log.contains("hunter2") && !log.contains("127.0.0.1"),
            "{log}"
        );
    }

    #[test]
    fn a_failed_delivery_is_logged_and_tried_again_next_time() {
        let dir = tempfile::tempdir().unwrap();
        secret_for(dir.path(), "openbot-neji", "s");
        let seen = std::cell::RefCell::new(vec![]);
        let d = dispatcher(dir.path(), &["openbot-neji"]);
        assert_eq!(
            d.notify_with(&step("openbot-neji"), &scripted(401, &seen)),
            Some(Outcome::Rejected(401))
        );
        assert_eq!(
            d.notify_with(&step("openbot-neji"), &scripted(202, &seen)),
            Some(Outcome::Accepted)
        );
        assert_eq!(seen.borrow().len(), 2);
    }

    #[test]
    fn a_lane_name_that_is_a_path_reads_no_secret() {
        let dir = tempfile::tempdir().unwrap();
        secret_for(dir.path(), "x", "s");
        let d = dispatcher(dir.path(), &["../x", "wake-secrets/x"]);
        assert!(d.secret("../x").is_none() && d.secret("wake-secrets/x").is_none());
        assert!(d.secret("x").is_some());
    }

    #[test]
    fn the_real_notify_is_pinned_even_with_a_secret_and_a_url() {
        let dir = tempfile::tempdir().unwrap();
        secret_for(dir.path(), "openbot-neji", "s");
        let d = dispatcher(dir.path(), &["openbot-neji"]);
        assert_eq!(
            d.notify(&step("openbot-neji")),
            Some(Outcome::Failed(
                "url is not https on the pinned host".to_owned()
            ))
        );
    }

    #[test]
    fn load_accepts_only_the_pinned_config() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Dispatcher::load(dir.path()).is_none(), "no file: off");
        std::fs::write(
            dir.path().join(CONFIG_FILE),
            r#"{"a":"http://evil.example/x"}"#,
        )
        .unwrap();
        assert!(
            Dispatcher::load(dir.path()).is_none(),
            "a url off the pin: off"
        );
        std::fs::write(
            dir.path().join(CONFIG_FILE),
            r#"{"openbot-neji":"https://signal.openbot.run/v1/webhooks/x"}"#,
        )
        .unwrap();
        assert!(Dispatcher::load(dir.path()).is_some());
    }

    fn old_failures(dir: &Path, id: &str, times: usize, at: &str) {
        let line = format!(
            "{at} {id} failed
"
        )
        .repeat(times);
        std::fs::write(dir.join(LOG_FILE), line).unwrap();
    }

    #[test]
    fn a_failed_delivery_waits_out_its_backoff_then_is_tried_again() {
        let dir = tempfile::tempdir().unwrap();
        secret_for(dir.path(), "openbot-neji", "s");
        let seen = std::cell::RefCell::new(vec![]);
        let d = dispatcher(dir.path(), &["openbot-neji"]);
        let id = delivery_id(&step("openbot-neji"));
        // Failed a moment ago: still backing off, nothing sent.
        old_failures(dir.path(), &id, 1, &clock().1);
        assert_eq!(
            d.notify_with(&step("openbot-neji"), &scripted(202, &seen)),
            Some(Outcome::Failed("backing off".to_owned()))
        );
        assert!(seen.borrow().is_empty());
        // Failed long ago: tried again, and delivered.
        old_failures(dir.path(), &id, 1, "2000-01-01T00:00:00Z");
        assert_eq!(
            d.notify_with(&step("openbot-neji"), &scripted(202, &seen)),
            Some(Outcome::Accepted)
        );
        assert_eq!(seen.borrow().len(), 1);
    }

    #[test]
    fn a_delivery_that_keeps_failing_is_given_up_once_and_then_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        secret_for(dir.path(), "openbot-neji", "s");
        let seen = std::cell::RefCell::new(vec![]);
        let d = dispatcher(dir.path(), &["openbot-neji"]);
        let id = delivery_id(&step("openbot-neji"));
        old_failures(
            dir.path(),
            &id,
            MAX_FAILED_PASSES as usize,
            "2000-01-01T00:00:00Z",
        );
        assert_eq!(
            d.notify_with(&step("openbot-neji"), &scripted(202, &seen)),
            Some(Outcome::GaveUp)
        );
        assert_eq!(
            d.notify_with(&step("openbot-neji"), &scripted(202, &seen)),
            None,
            "given up is final"
        );
        assert!(seen.borrow().is_empty(), "nothing was sent");
        let log = std::fs::read_to_string(dir.path().join(LOG_FILE)).unwrap();
        assert_eq!(log.matches("gave-up").count(), 1, "{log}");
    }

    #[test]
    fn a_401_or_404_is_logged_and_final() {
        let dir = tempfile::tempdir().unwrap();
        secret_for(dir.path(), "openbot-neji", "s");
        let seen = std::cell::RefCell::new(vec![]);
        let d = dispatcher(dir.path(), &["openbot-neji"]);
        assert_eq!(
            d.notify_with(&step("openbot-neji"), &scripted(404, &seen)),
            Some(Outcome::Rejected(404))
        );
        let log = std::fs::read_to_string(dir.path().join(LOG_FILE)).unwrap();
        assert!(
            log.contains("openbot-neji:review:abcdef12 rejected-404"),
            "{log}"
        );
    }
}
