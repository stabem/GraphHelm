//! #2714: wakes a parked lane through its OpenBot webhook.
//!
//! The only place with an HTTP client for this feature: `core/*` stays free of one. A caller
//! hands over a ready [`Step`]; this crate signs it, posts it once with a deadline, retries a
//! 503 a bounded number of times and reports an [`Outcome`]. It keeps nothing in memory between
//! calls and never writes the journal. A signing secret is a [`Secret`]: its `Debug` text is
//! fixed, so no log line or error can carry the value.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::Serialize;
use sha2::{Digest, Sha256};

/// The only host a webhook may point at (no SSRF through a mistaken or hostile config).
pub const PINNED_HOST: &str = "signal.openbot.run";
/// The OpenBot event type this adapter sends.
pub const EVENT_TYPE: &str = "graphhelm.step.ready";
/// Seconds a request may take, end to end.
pub const DEADLINE: Duration = Duration::from_secs(10);
/// Retries after the first attempt, for a 503 only.
pub const MAX_RETRIES: u32 = 3;

/// A signing secret. Printing it prints nothing of its value.
pub struct Secret(zeroize::Zeroizing<String>);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(zeroize::Zeroizing::new(value.into().trim().to_owned()))
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

/// One ready step: exactly the fields the receiver gets, nothing private.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Step {
    pub lane: String,
    pub step: String,
    pub skill: String,
    pub issue: u64,
    pub pr: Option<u64>,
    pub head: String,
    pub because: Reason,
}

/// Why a step is ready: a fixed code, never free text, so nothing private can ride along.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    ReviewAssigned,
    BlockVerdict,
    Approved,
    Merged,
}

/// What happened to one delivery.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// 202: accepted.
    Accepted,
    /// 200: the receiver already had this delivery id.
    Duplicate,
    /// 401, 404 or another 4xx: not retried.
    Rejected(u16),
    /// A 503 that outlived the retries, or a transport failure.
    Failed(String),
}

/// `<lane>:<step>:<head>`: one step wakes a lane once.
pub fn delivery_id(step: &Step) -> String {
    format!("{}:{}:{}", step.lane, step.step, step.head)
}

/// RFC 2104 HMAC-SHA256.
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut block = [0_u8; 64];
    if key.len() > 64 {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(block.map(|byte| byte ^ 0x36));
    inner.update(message);
    let inner = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(block.map(|byte| byte ^ 0x5c));
    outer.update(inner);
    outer.finalize().into()
}

/// `sha256=<hex HMAC("<timestamp>.<delivery-id>.<body>")>`, the OpenBot contract.
pub fn signature(secret: &Secret, timestamp: u64, delivery_id: &str, body: &str) -> String {
    let message = format!("{timestamp}.{delivery_id}.{body}");
    format!(
        "sha256={}",
        hex::encode(hmac_sha256(secret.0.as_bytes(), message.as_bytes()))
    )
}

/// The request body for a step.
pub fn body(step: &Step, occurred_at: &str) -> String {
    serde_json::json!({"type": EVENT_TYPE, "occurredAt": occurred_at, "data": step}).to_string()
}

/// The lane -> URL map, read from config. Only https URLs on the pinned host are accepted.
#[derive(Default)]
pub struct Config {
    urls: BTreeMap<String, String>,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Lanes only: a webhook url is a capability and never prints.
        f.debug_struct("Config")
            .field("lanes", &self.urls.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// True when `url` is `<scheme>://<host>` followed by a path or a port, with no userinfo and no
/// fragment: the same check for the config and for every delivery.
fn on_pinned_host(url: &str, host: &str, scheme: &str) -> bool {
    let rest = url.strip_prefix(&format!("{scheme}://{host}"));
    rest.is_some_and(|rest| rest.starts_with('/') || rest.starts_with(':'))
        && !url.contains('@')
        && !url.contains('#')
}

impl Config {
    /// `{"openbot-neji": "https://signal.openbot.run/v1/webhooks/<id>", ...}`.
    pub fn parse(json: &str) -> Result<Self, String> {
        let raw: BTreeMap<String, String> = serde_json::from_str(json)
            .map_err(|_| "wake webhook config is not a lane -> url map".to_owned())?;
        for (lane, url) in &raw {
            if !on_pinned_host(url, PINNED_HOST, "https") {
                return Err(format!("webhook for {lane} is not https on {PINNED_HOST}"));
            }
        }
        Ok(Self { urls: raw })
    }

    pub fn url(&self, lane: &str) -> Option<&str> {
        self.urls.get(lane).map(String::as_str)
    }
}

/// One POST: status code, or why there was none. A seam so retries are tested without a network.
pub trait Transport {
    fn post(&self, url: &str, headers: &[(&str, String)], body: &str) -> Result<u16, String>;
}

/// A 4xx/5xx is a status the caller reads, not a transport error (a 401 must not be retried).
const STATUS_IS_ERROR: bool = false;

/// The real transport: ureq, no redirects, a global deadline, a 4xx/5xx is a status not an error.
pub struct Ureq {
    agent: ureq::Agent,
}

impl Ureq {
    pub fn new(deadline: Duration) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(deadline))
            .max_redirects(0)
            .http_status_as_error(STATUS_IS_ERROR)
            .build()
            .into();
        Self { agent }
    }
}

impl Transport for Ureq {
    fn post(&self, url: &str, headers: &[(&str, String)], body: &str) -> Result<u16, String> {
        let mut request = self
            .agent
            .post(url)
            .header("content-type", "application/json");
        for (name, value) in headers {
            request = request.header(*name, value.as_str());
        }
        request
            .send(body)
            .map(|reply| reply.status().as_u16())
            .map_err(|error| format!("transport: {}", error.to_string().replace(url, "<url>")))
    }
}

/// Signs and posts `step`; a 503 is retried up to [`MAX_RETRIES`] times with `pause` between tries.
/// 401/404 and any other 4xx are never retried. No secret or url appears in the outcome.
pub fn deliver(
    transport: &dyn Transport,
    url: &str,
    secret: &Secret,
    step: &Step,
    now: &dyn Fn() -> (u64, String),
    pause: &dyn Fn(u32),
) -> Outcome {
    if !on_pinned_host(url, PINNED_HOST, "https") {
        return Outcome::Failed("url is not https on the pinned host".to_owned());
    }
    deliver_to(transport, url, secret, step, now, pause)
}

/// [`deliver`] without the host pin. Private: only the tests, which post to a loopback receiver.
fn deliver_to(
    transport: &dyn Transport,
    url: &str,
    secret: &Secret,
    step: &Step,
    now: &dyn Fn() -> (u64, String),
    pause: &dyn Fn(u32),
) -> Outcome {
    if secret.is_empty() {
        return Outcome::Failed("no signing secret".to_owned());
    }
    let id = delivery_id(step);
    for attempt in 0..=MAX_RETRIES {
        let (timestamp, occurred_at) = now();
        let payload = body(step, &occurred_at);
        let headers = [
            ("x-openbot-timestamp", timestamp.to_string()),
            ("x-openbot-delivery-id", id.clone()),
            (
                "x-openbot-signature",
                signature(secret, timestamp, &id, &payload),
            ),
        ];
        match transport.post(url, &headers, &payload) {
            Ok(200) => return Outcome::Duplicate,
            Ok(code) if (200..300).contains(&code) => return Outcome::Accepted,
            Ok(503) if attempt < MAX_RETRIES => pause(attempt),
            Ok(503) => return Outcome::Failed("503 after retries".to_owned()),
            Ok(code) => return Outcome::Rejected(code),
            Err(error) => return Outcome::Failed(error),
        }
    }
    Outcome::Failed("unreachable".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn step() -> Step {
        Step {
            lane: "openbot-neji".into(),
            step: "review".into(),
            skill: "blind-review".into(),
            issue: 2694,
            pr: Some(2715),
            head: "878fb578".into(),
            because: Reason::ReviewAssigned,
        }
    }

    #[test]
    fn hmac_matches_the_rfc_4231_vector() {
        let mac = hmac_sha256(&[0x0b; 20], b"Hi There");
        assert_eq!(
            hex::encode(mac),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    #[test]
    fn the_delivery_id_names_lane_step_and_head() {
        assert_eq!(delivery_id(&step()), "openbot-neji:review:878fb578");
    }

    #[test]
    fn the_signature_covers_timestamp_id_and_body() {
        let secret = Secret::new("s3cret");
        let a = signature(&secret, 100, "id", "{}");
        assert!(a.starts_with("sha256="));
        assert_ne!(a, signature(&secret, 101, "id", "{}"));
        assert_ne!(a, signature(&secret, 100, "id2", "{}"));
        assert_ne!(a, signature(&secret, 100, "id", "{ }"));
        assert_ne!(a, signature(&Secret::new("other"), 100, "id", "{}"));
    }

    #[test]
    fn a_secret_never_prints() {
        assert_eq!(
            format!("{:?}", Secret::new("hunter2")),
            "Secret(<redacted>)"
        );
    }

    #[test]
    fn config_accepts_only_https_urls_on_the_pinned_host() {
        let ok = r#"{"openbot-neji":"https://signal.openbot.run/v1/webhooks/abc"}"#;
        assert_eq!(
            Config::parse(ok).unwrap().url("openbot-neji"),
            Some("https://signal.openbot.run/v1/webhooks/abc")
        );
        for bad in [
            r#"{"a":"http://signal.openbot.run/v1/webhooks/abc"}"#,
            r#"{"a":"https://evil.example/v1/webhooks/abc"}"#,
            r#"{"a":"https://signal.openbot.run.evil.example/x"}"#,
            r#"{"a":"https://signal.openbot.run@evil.example/x"}"#,
            r#"{"a":"https://signal.openbot.run"}"#,
            r#"["x"]"#,
        ] {
            assert!(Config::parse(bad).is_err(), "{bad}");
        }
    }

    struct Fake {
        replies: RefCell<Vec<Result<u16, String>>>,
        seen: RefCell<Vec<Vec<(String, String)>>>,
    }

    impl Transport for Fake {
        fn post(&self, _url: &str, headers: &[(&str, String)], _body: &str) -> Result<u16, String> {
            self.seen.borrow_mut().push(
                headers
                    .iter()
                    .map(|(n, v)| ((*n).to_owned(), v.clone()))
                    .collect(),
            );
            self.replies.borrow_mut().remove(0)
        }
    }

    fn run(replies: Vec<Result<u16, String>>) -> (Outcome, usize) {
        let fake = Fake {
            replies: RefCell::new(replies),
            seen: RefCell::new(vec![]),
        };
        let outcome = deliver_to(
            &fake,
            "https://signal.openbot.run/x",
            &Secret::new("s3cret"),
            &step(),
            &|| (1_700_000_000, "2026-11-10T00:00:00Z".to_owned()),
            &|_| {},
        );
        (outcome, fake.seen.borrow().len())
    }

    #[test]
    fn a_503_is_retried_then_accepted() {
        assert_eq!(run(vec![Ok(503), Ok(503), Ok(202)]), (Outcome::Accepted, 3));
    }

    #[test]
    fn a_503_that_never_clears_stops_at_the_bound() {
        let (outcome, tries) = run(vec![Ok(503); 10]);
        assert_eq!(outcome, Outcome::Failed("503 after retries".to_owned()));
        assert_eq!(tries, 1 + MAX_RETRIES as usize);
    }

    #[test]
    fn a_401_or_404_is_never_retried_and_200_is_a_duplicate() {
        assert_eq!(run(vec![Ok(401)]), (Outcome::Rejected(401), 1));
        assert_eq!(run(vec![Ok(404)]), (Outcome::Rejected(404), 1));
        assert_eq!(run(vec![Ok(200)]), (Outcome::Duplicate, 1));
    }

    #[test]
    fn every_try_carries_the_contract_headers_with_the_same_delivery_id() {
        let fake = Fake {
            replies: RefCell::new(vec![Ok(503), Ok(202)]),
            seen: RefCell::new(vec![]),
        };
        deliver_to(
            &fake,
            "u",
            &Secret::new("k"),
            &step(),
            &|| (5, "t".into()),
            &|_| {},
        );
        let seen = fake.seen.borrow();
        assert_eq!(seen.len(), 2);
        for headers in seen.iter() {
            let get = |name: &str| {
                headers
                    .iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, v)| v.clone())
            };
            assert_eq!(
                get("x-openbot-delivery-id").as_deref(),
                Some("openbot-neji:review:878fb578")
            );
            assert_eq!(get("x-openbot-timestamp").as_deref(), Some("5"));
            assert!(get("x-openbot-signature").unwrap().starts_with("sha256="));
        }
    }

    #[test]
    fn no_secret_means_no_request() {
        let fake = Fake {
            replies: RefCell::new(vec![]),
            seen: RefCell::new(vec![]),
        };
        let outcome = deliver_to(
            &fake,
            "u",
            &Secret::new("  "),
            &step(),
            &|| (1, "t".into()),
            &|_| {},
        );
        assert_eq!(outcome, Outcome::Failed("no signing secret".to_owned()));
        assert!(fake.seen.borrow().is_empty());
    }

    /// A loopback receiver on a real socket: 503 once, then 202. It checks, from the bytes that
    /// crossed the wire, the headers, the delivery id and a signature it recomputes itself.
    fn receiver(
        secret: &str,
        replies: Vec<&'static str>,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1/webhooks/x", listener.local_addr().unwrap());
        let secret = secret.to_owned();
        let handle = std::thread::spawn(move || {
            let mut verdicts = Vec::new();
            for status in replies {
                let (mut stream, _) = listener.accept().unwrap();
                let mut raw = Vec::new();
                let mut buffer = [0_u8; 4096];
                loop {
                    let read = stream.read(&mut buffer).unwrap();
                    raw.extend_from_slice(&buffer[..read]);
                    let text = String::from_utf8_lossy(&raw).to_string();
                    if let Some((head, rest)) = text.split_once("\r\n\r\n") {
                        let length = head
                            .lines()
                            .find_map(|line| {
                                line.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if rest.len() >= length {
                            break;
                        }
                    }
                    if read == 0 {
                        break;
                    }
                }
                let text = String::from_utf8_lossy(&raw).to_string();
                let (head, payload) = text.split_once("\r\n\r\n").unwrap();
                let header = |name: &str| {
                    head.lines()
                        .find_map(|line| {
                            let lower = line.to_lowercase();
                            lower
                                .strip_prefix(&format!("{name}:"))
                                .map(|_| line[name.len() + 1..].trim().to_owned())
                        })
                        .unwrap()
                };
                let timestamp: u64 = header("x-openbot-timestamp").parse().unwrap();
                let id = header("x-openbot-delivery-id");
                let expected = signature(&Secret::new(secret.clone()), timestamp, &id, payload);
                verdicts.push(format!(
                    "{id} sig_ok={} type_ok={}",
                    header("x-openbot-signature") == expected,
                    payload.contains(r#""type":"graphhelm.step.ready""#)
                ));
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                )
                .unwrap();
            }
            verdicts
        });
        (url, handle)
    }

    #[test]
    fn a_real_post_is_signed_retried_on_503_and_accepted() {
        let (url, handle) = receiver("s3cret", vec!["503 Service Unavailable", "202 Accepted"]);
        let outcome = deliver_to(
            &Ureq::new(DEADLINE),
            &url,
            &Secret::new("s3cret"),
            &step(),
            &|| (1_700_000_000, "2026-11-10T00:00:00Z".to_owned()),
            &|_| {},
        );
        assert_eq!(outcome, Outcome::Accepted);
        assert_eq!(
            handle.join().unwrap(),
            vec!["openbot-neji:review:878fb578 sig_ok=true type_ok=true"; 2]
        );
    }

    #[test]
    fn break_once_a_wrong_secret_is_refused_by_the_receiver_and_not_retried() {
        // The receiver signs with its own secret; ours differs, so its check fails.
        let (url, handle) = receiver("right", vec!["401 Unauthorized"]);
        let outcome = deliver_to(
            &Ureq::new(DEADLINE),
            &url,
            &Secret::new("wrong"),
            &step(),
            &|| (1_700_000_000, "2026-11-10T00:00:00Z".to_owned()),
            &|_| {},
        );
        assert_eq!(outcome, Outcome::Rejected(401));
        assert_eq!(
            handle.join().unwrap(),
            vec!["openbot-neji:review:878fb578 sig_ok=false type_ok=true"]
        );
    }

    #[test]
    fn a_redirect_is_not_followed() {
        let target = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        target.set_nonblocking(true).unwrap();
        let location = format!(
            "location: http://{}/elsewhere",
            target.local_addr().unwrap()
        );
        let reply: &'static str = Box::leak(format!("302 Found\r\n{location}").into_boxed_str());
        let (url, handle) = receiver("s3cret", vec![reply]);
        let outcome = deliver_to(
            &Ureq::new(DEADLINE),
            &url,
            &Secret::new("s3cret"),
            &step(),
            &|| (1_700_000_000, "2026-11-10T00:00:00Z".to_owned()),
            &|_| {},
        );
        assert_ne!(outcome, Outcome::Accepted, "a redirect is never a delivery");
        assert_eq!(
            handle.join().unwrap().len(),
            1,
            "one request, no second try"
        );
        assert!(
            matches!(target.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
            "the redirect target was never contacted"
        );
    }

    #[test]
    fn deliver_itself_refuses_a_url_off_the_pinned_host() {
        let fake = Fake {
            replies: RefCell::new(vec![]),
            seen: RefCell::new(vec![]),
        };
        for url in [
            "http://signal.openbot.run/x",
            "https://evil.example/x",
            "https://signal.openbot.run@evil.example/x",
            "https://signal.openbot.run.evil.example/x",
        ] {
            let outcome = deliver(
                &fake,
                url,
                &Secret::new("k"),
                &step(),
                &|| (1, "t".into()),
                &|_| {},
            );
            assert!(matches!(outcome, Outcome::Failed(_)), "{url}");
        }
        assert!(fake.seen.borrow().is_empty(), "nothing was sent");
    }

    #[test]
    fn a_key_longer_than_a_block_is_hashed_first() {
        // RFC 4231 test case 6: a 131-byte key.
        let mac = hmac_sha256(
            &[0xaa; 131],
            b"Test Using Larger Than Block-Size Key - Hash Key First",
        );
        assert_eq!(
            hex::encode(mac),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
        // Exactly one block (64 bytes) is used as is, not hashed.
        assert_ne!(hmac_sha256(&[1; 64], b"m"), hmac_sha256(&[1; 65], b"m"));
    }

    #[test]
    fn the_config_never_prints_a_webhook_url() {
        let config = Config::parse(
            r#"{"openbot-neji":"https://signal.openbot.run/v1/webhooks/capability-token"}"#,
        )
        .unwrap();
        let printed = format!("{config:?}");
        assert!(printed.contains("openbot-neji"));
        assert!(!printed.contains("capability-token"));
    }

    #[test]
    fn any_2xx_is_accepted_except_200_which_is_a_duplicate() {
        assert_eq!(run(vec![Ok(201)]), (Outcome::Accepted, 1));
        assert_eq!(run(vec![Ok(204)]), (Outcome::Accepted, 1));
        assert_eq!(run(vec![Ok(200)]), (Outcome::Duplicate, 1));
    }

    #[test]
    fn the_reason_is_a_fixed_code_in_the_body() {
        let payload = body(&step(), "t");
        assert!(
            payload.contains(r#""because":"review_assigned""#),
            "{payload}"
        );
    }

    #[test]
    fn a_wrong_secret_gives_a_different_signature() {
        let id = delivery_id(&step());
        let payload = body(&step(), "t");
        assert_ne!(
            signature(&Secret::new("right"), 1, &id, &payload),
            signature(&Secret::new("wrong"), 1, &id, &payload)
        );
    }
}
