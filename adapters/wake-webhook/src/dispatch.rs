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

/// What a logged code means to the readers of the log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Meaning {
    /// The step is done: delivered, or given up. It is never sent again.
    Final,
    /// A failed attempt: counted for the backoff and for the give-up limit.
    Retryable,
    /// Written for the record only: a rejection or a skip. Neither final nor counted.
    Other,
}

/// The code on a log line: the one place its text is made and read back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LogCode {
    Accepted,
    Duplicate,
    Rejected(u16),
    Failed,
    GaveUp,
    SkippedNoUrl,
    SkippedNoSecret,
    SkippedNoAuthor,
}

impl LogCode {
    fn text(self) -> String {
        match self {
            Self::Accepted => "accepted".to_owned(),
            Self::Duplicate => "duplicate".to_owned(),
            Self::Rejected(code) => format!("rejected-{code}"),
            Self::Failed => "failed".to_owned(),
            Self::GaveUp => "gave-up".to_owned(),
            Self::SkippedNoUrl => "skipped-no-url".to_owned(),
            Self::SkippedNoSecret => "skipped-no-secret".to_owned(),
            Self::SkippedNoAuthor => "skipped-no-author".to_owned(),
        }
    }

    /// The code a log line carries, or `None` for a word this log never writes.
    fn parse(text: &str) -> Option<Self> {
        match text {
            "accepted" => Some(Self::Accepted),
            "duplicate" => Some(Self::Duplicate),
            "failed" => Some(Self::Failed),
            "gave-up" => Some(Self::GaveUp),
            "skipped-no-url" => Some(Self::SkippedNoUrl),
            "skipped-no-secret" => Some(Self::SkippedNoSecret),
            "skipped-no-author" => Some(Self::SkippedNoAuthor),
            other => other
                .strip_prefix("rejected-")
                .and_then(|status| status.parse().ok())
                .map(Self::Rejected),
        }
    }

    fn meaning(self) -> Meaning {
        match self {
            Self::Accepted | Self::Duplicate | Self::GaveUp => Meaning::Final,
            Self::Failed => Meaning::Retryable,
            Self::Rejected(_)
            | Self::SkippedNoUrl
            | Self::SkippedNoSecret
            | Self::SkippedNoAuthor => Meaning::Other,
        }
    }
}

impl From<&Outcome> for LogCode {
    fn from(outcome: &Outcome) -> Self {
        match outcome {
            Outcome::Accepted => Self::Accepted,
            Outcome::Duplicate => Self::Duplicate,
            Outcome::Rejected(code) => Self::Rejected(*code),
            Outcome::Failed(_) => Self::Failed,
            Outcome::GaveUp => Self::GaveUp,
        }
    }
}

/// The lines of the log for delivery `id`: the time word and the parsed code (a line whose code
/// is not one this log writes is skipped).
fn lines_for<'a>(log: &'a str, id: &'a str) -> impl Iterator<Item = (&'a str, LogCode)> {
    log.lines().filter_map(move |line| {
        let mut parts = line.split_whitespace();
        let at = parts.next()?;
        (parts.next()? == id).then_some(())?;
        Some((at, LogCode::parse(parts.next()?)?))
    })
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
        std::fs::read_to_string(self.dir.join(LOG_FILE))
            .is_ok_and(|log| lines_for(&log, id).any(|(_, code)| code.meaning() == Meaning::Final))
    }

    /// How many times `id` failed so far, and the unix time of the last failure.
    fn failures(&self, id: &str) -> (u64, u64) {
        let log = std::fs::read_to_string(self.dir.join(LOG_FILE)).unwrap_or_default();
        let mut count = 0;
        let mut last = 0;
        for (at, _) in lines_for(&log, id).filter(|(_, code)| code.meaning() == Meaning::Retryable)
        {
            count += 1;
            last = chrono::DateTime::parse_from_rfc3339(at).map_or(last, |moment| {
                u64::try_from(moment.timestamp()).unwrap_or(last)
            });
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
            self.log(&id, &LogCode::SkippedNoUrl.text());
            return None;
        };
        let Some(secret) = self.secret(&step.lane) else {
            self.log(&id, &LogCode::SkippedNoSecret.text());
            return None;
        };
        if self.delivered(&id) {
            return None;
        }
        let (failures, last_failure) = self.failures(&id);
        if failures >= MAX_FAILED_PASSES {
            self.log(&id, &LogCode::GaveUp.text());
            return Some(Outcome::GaveUp);
        }
        if failures > 0
            && clock().0 < last_failure + FIRST_BACKOFF_SECS * 2_u64.pow(failures as u32 - 1)
        {
            // Still backing off: nothing is sent and nothing is logged; the caller holds its place.
            return Some(Outcome::Failed("backing off".to_owned()));
        }
        let outcome = send(&self.client, url, &secret, step);
        self.log(&id, &LogCode::from(&outcome).text());
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
    fn every_logged_code_has_one_text_and_one_meaning() {
        let table = [
            ("accepted", Meaning::Final),
            ("duplicate", Meaning::Final),
            ("gave-up", Meaning::Final),
            ("failed", Meaning::Retryable),
            ("rejected-401", Meaning::Other),
            ("rejected-404", Meaning::Other),
            ("skipped-no-url", Meaning::Other),
            ("skipped-no-secret", Meaning::Other),
            ("skipped-no-author", Meaning::Other),
        ];
        for (text, meaning) in table {
            let code = LogCode::parse(text).unwrap_or_else(|| panic!("{text} must parse"));
            assert_eq!(code.text(), text, "round trip");
            assert_eq!(code.meaning(), meaning, "{text}");
        }
        assert_eq!(LogCode::parse("rejected-"), None);
        assert_eq!(LogCode::parse("rejected-x"), None);
        assert_eq!(LogCode::parse("whatever"), None);
        // Every outcome is written as the code the readers parse back to the same meaning.
        for (outcome, meaning) in [
            (Outcome::Accepted, Meaning::Final),
            (Outcome::Duplicate, Meaning::Final),
            (Outcome::GaveUp, Meaning::Final),
            (Outcome::Failed("x".into()), Meaning::Retryable),
            (Outcome::Rejected(401), Meaning::Other),
        ] {
            let text = LogCode::from(&outcome).text();
            assert_eq!(LogCode::parse(&text).unwrap().meaning(), meaning, "{text}");
        }
    }

    #[test]
    fn the_readers_follow_the_meaning_of_each_logged_line() {
        let dir = tempfile::tempdir().unwrap();
        let d = dispatcher(dir.path(), &["openbot-neji"]);
        let id = delivery_id(&step("openbot-neji"));
        let at = "2000-01-01T00:00:00Z";
        for (code, delivered, failed) in [
            ("accepted", true, 0),
            ("duplicate", true, 0),
            ("gave-up", true, 0),
            ("failed", false, 1),
            ("rejected-401", false, 0),
            ("skipped-no-url", false, 0),
            ("garbage", false, 0),
        ] {
            std::fs::write(dir.path().join(LOG_FILE), format!("{at} {id} {code}\n")).unwrap();
            assert_eq!(d.delivered(&id), delivered, "{code}");
            assert_eq!(d.failures(&id).0, failed, "{code}");
        }
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
