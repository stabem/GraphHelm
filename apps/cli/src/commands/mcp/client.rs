//! The MCP server's API client half (Task 3): loopback-only fail-closed URL admission, token
//! handling that never touches argv or any output stream, and the per-logical-act idempotency
//! derivation (`mcp-{nonce}-{rpc-id}`) whose ≤64 bound holds by construction.

// The Task 5 tool table is this module's consumer; until it lands, construction and the
// config refusals are exercised end to end while the request path only exists. The allow
// dies with Task 5.
#![allow(dead_code)]

use std::time::Duration;

use graphhelm_model_gateway::transport::UreqTransport;
use zeroize::Zeroizing;

/// Default per-call timeout for every API request this client places.
pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// The optional parts of one API request (#600): the idempotency key of a mutation, the `If-Match`
/// head pin, and how long to wait.
#[derive(Clone, Copy)]
struct Settings<'a> {
    idempotency_key: Option<&'a str>,
    if_match: Option<u64>,
    timeout: Duration,
}

/// The direct-arm bound for [`derive_key`]: an rpc id that is already `[a-z0-9-]{1,32}` rides
/// verbatim; anything longer (or wider) is digested. Proof by construction that the ≤64
/// header cap can never be exceeded: `"mcp-" (4) + nonce (16 hex) + "-" (1) + marker (1) +
/// 32 = 54` in the direct arm and `4 + 16 + 1 + 1 + 16 = 38` in the digest arm.
pub(crate) const DIRECT_ID_MAX: usize = 32;

/// Loopback-only URL admission — the post-#36 `is_loopback_authority` rule applied to this
/// surface (`core/gateway/src/manifest.rs:584` is the source of truth this mirrors): userinfo
/// (`user:pass@`) is stripped BEFORE any host inspection, so a bracketed IPv6 loopback or a
/// bare `localhost` label sitting in the userinfo position (`[::1]@evil.com`,
/// `localhost:tok@attacker.example`) never reads as loopback while the host actually reached
/// — the text after the LAST `@` — is remote.
pub(crate) fn is_loopback_url(url: &str) -> bool {
    let rest = match url.split_once("://") {
        Some(("http" | "https", rest)) => rest,
        _ => return false,
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let authority = authority
        .rsplit_once('@')
        .map_or(authority, |(_userinfo, host)| host);

    if let Some(bracketed) = authority.strip_prefix('[') {
        let host = bracketed.split(']').next().unwrap_or_default();
        return host == "::1";
    }

    let host = authority.split(':').next().unwrap_or_default();
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<std::net::Ipv4Addr>()
        .is_ok_and(|address| address.is_loopback())
}

/// The per-logical-act idempotency key: `mcp-{nonce}-{marker}{norm}` where `marker` is one
/// fixed character naming the rpc id's JSON type (`s` string, `n` everything else) and
/// `norm` is the id itself when it is already `[a-z0-9-]{1,32}` (a number's decimal form
/// counts), else the first 16 hex of its SHA-256 — digested, never truncated (truncation
/// collides; digests do not). The marker closes the review-found type collision: `7` and
/// `"7"` are distinct rpc ids and derive distinct keys (`n7` vs `s7`), and no string can
/// forge a number's key because the marker position is fixed and the arms are disjoint.
/// The key follows the LOGICAL act: a client retry of the same rpc id reuses the key, and
/// the API's content-aware machinery absorbs it.
pub(crate) fn derive_key(nonce: &str, rpc_id: &serde_json::Value) -> String {
    let (marker, raw) = match rpc_id {
        serde_json::Value::String(id) => ('s', id.clone()),
        other => ('n', other.to_string()),
    };
    let direct = (1..=DIRECT_ID_MAX).contains(&raw.len())
        && raw
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    let norm = if direct {
        raw
    } else {
        graphhelm_graph::raw_content_sha256(raw.as_bytes())
            .expect("a sha256 hex parses")
            .as_str()[..16]
            .to_owned()
    };
    format!("mcp-{nonce}-{marker}{norm}")
}

/// The API client: base URL already admitted as loopback, token held zeroizing and reachable
/// only by the request builder — never by argv, never by any output path. Requests reuse the
/// gateway's transport (redirects hard-off per ADR-025; per-call timeout).
/// Where the bearer token comes from (#1325).
pub(crate) enum TokenSource {
    /// `--token-file` or `GRAPHHELM_API_TOKEN`: one token for the process's life.
    Fixed(Zeroizing<String>),
    /// `--discover`: the token file named by the discovery record of the Runtime serving `port`,
    /// believed only once that Runtime's `/health` reports the record's instance. Cached, and
    /// resolved again after a 401 or a transport failure, so the process follows a Runtime swap
    /// on the same port without a restart.
    Discover {
        port: u16,
        cache: std::sync::Mutex<Option<Zeroizing<String>>>,
    },
    /// `--discover` without `--url`: resolve the current project's verified Runtime, regardless
    /// of its port. The endpoint and token are refreshed after a refused request or transport
    /// failure; a transport failure itself is never retried because it may have landed.
    DiscoverProject {
        project_id: String,
        cache: std::sync::Mutex<Option<DiscoveredEndpoint>>,
    },
}

#[derive(Clone)]
pub(crate) struct DiscoveredEndpoint {
    url: String,
    token: Zeroizing<String>,
}

impl TokenSource {
    pub(crate) fn discover(port: u16) -> Self {
        Self::Discover {
            port,
            cache: std::sync::Mutex::new(None),
        }
    }

    pub(crate) fn discover_project(project_id: String) -> Self {
        Self::DiscoverProject {
            project_id,
            cache: std::sync::Mutex::new(None),
        }
    }
}

pub(crate) struct ApiClient {
    pub(crate) base_url: String,
    token: TokenSource,
    pub(crate) actor: String,
    pub(crate) actor_type: String,
    model: Option<String>,
    effort: Option<String>,
    /// #1057: this process's own session token -- the SAME per-process nonce the idempotency keys
    /// and the wake leases already carry, rather than a second random string meaning the same
    /// thing. It rides every mutation, declared model or not: a session that declares nothing
    /// still has to say WHICH session declared nothing, or the Runtime cannot tell it from the
    /// previous session that reused this actor id.
    session: String,
    transport: UreqTransport,
}

impl ApiClient {
    pub(crate) fn new(
        base_url: String,
        token: TokenSource,
        actor: String,
        actor_type: String,
        model: Option<String>,
        effort: Option<String>,
        session: String,
    ) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            token,
            actor,
            actor_type,
            model,
            effort,
            session,
            transport: UreqTransport::new(),
        }
    }

    /// The attribution headers a mutation carries: actor identity, actor type, this process's
    /// session token, and — only when declared — the model and effort the agent is running.
    /// Absent is absent: a `None` model or effort emits no header at all, never an empty-string
    /// one. Extracted so these can be asserted without a live server (Task 2, #1054).
    ///
    /// The session token is UNCONDITIONAL, and that is #1057's whole point: an undeclared session
    /// that sent nothing would be indistinguishable from the previous session that reused this
    /// actor id, and the board would keep showing that session's model as this one's.
    pub(crate) fn mutation_headers(&self) -> Vec<(String, String)> {
        let mut headers = vec![
            ("X-GraphHelm-Actor".to_owned(), self.actor.clone()),
            ("X-GraphHelm-Actor-Type".to_owned(), self.actor_type.clone()),
            ("X-GraphHelm-Actor-Session".to_owned(), self.session.clone()),
        ];
        if let Some(model) = self.model.as_ref() {
            headers.push(("X-GraphHelm-Actor-Model".to_owned(), model.clone()));
        }
        if let Some(effort) = self.effort.as_ref() {
            headers.push(("X-GraphHelm-Actor-Effort".to_owned(), effort.clone()));
        }
        headers
    }

    /// One API request: `GET`s carry the bearer token only; mutations additionally attach
    /// the attribution headers ([`Self::mutation_headers`]), with the idempotency key derived
    /// from the rpc id, and
    /// the optional `If-Match` head pin (CHAT_SURFACE_SPEC §3: optimistic concurrency is the
    /// chat's default choreography — a stale pin comes back as the API's own 409).
    pub(crate) fn request(
        &self,
        method: &'static str,
        path: &str,
        body: Option<&serde_json::Value>,
        idempotency_key: Option<&str>,
        if_match: Option<u64>,
    ) -> Result<(u16, serde_json::Value), String> {
        self.request_waiting(
            method,
            path,
            body,
            idempotency_key,
            if_match,
            REQUEST_TIMEOUT,
        )
    }

    /// [`Self::request`] with its own wait, for a call whose Runtime side may take longer than
    /// [`REQUEST_TIMEOUT`] (the gateway probe, #600 review).
    pub(crate) fn request_waiting(
        &self,
        method: &'static str,
        path: &str,
        body: Option<&serde_json::Value>,
        idempotency_key: Option<&str>,
        if_match: Option<u64>,
        timeout: Duration,
    ) -> Result<(u16, serde_json::Value), String> {
        let endpoint = self.current_endpoint()?;
        let result = self.request_with(
            &endpoint,
            method,
            path,
            body,
            Settings {
                idempotency_key,
                if_match,
                timeout,
            },
        );
        if !self.is_discovering() {
            return result;
        }
        match &result {
            // A 401 means the request was refused before it did anything, so asking the record
            // again and retrying once is safe: the Runtime on the port may have been swapped.
            Ok((401, _)) => {
                self.clear_discovery_cache();
                let fresh = self.current_endpoint()?;
                if fresh.url == endpoint.url && fresh.token.as_str() == endpoint.token.as_str() {
                    return result;
                }
                self.request_with(
                    &fresh,
                    method,
                    path,
                    body,
                    Settings {
                        idempotency_key,
                        if_match,
                        timeout,
                    },
                )
            }
            // A transport failure may have landed; never retried here, only re-resolved next time.
            Err(_) => {
                self.clear_discovery_cache();
                result
            }
            Ok(_) => result,
        }
    }

    fn is_discovering(&self) -> bool {
        matches!(
            &self.token,
            TokenSource::Discover { .. } | TokenSource::DiscoverProject { .. }
        )
    }

    fn clear_discovery_cache(&self) {
        match &self.token {
            TokenSource::Discover { cache, .. } => {
                *cache
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
            }
            TokenSource::DiscoverProject { cache, .. } => {
                *cache
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
            }
            TokenSource::Fixed(_) => {}
        }
    }

    /// The destination and credential for the next request.
    fn current_endpoint(&self) -> Result<DiscoveredEndpoint, String> {
        match &self.token {
            TokenSource::Fixed(token) => Ok(DiscoveredEndpoint {
                url: self.base_url.clone(),
                token: token.clone(),
            }),
            TokenSource::Discover { port, cache } => {
                let mut cache = cache
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(token) = cache.as_ref() {
                    return Ok(DiscoveredEndpoint {
                        url: self.base_url.clone(),
                        token: token.clone(),
                    });
                }
                let token = self.discover(*port)?;
                *cache = Some(token.clone());
                Ok(DiscoveredEndpoint {
                    url: self.base_url.clone(),
                    token,
                })
            }
            TokenSource::DiscoverProject { project_id, cache } => {
                let mut cache = cache
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(endpoint) = cache.as_ref() {
                    return Ok(endpoint.clone());
                }
                let endpoint = self.discover_project(project_id)?;
                *cache = Some(endpoint.clone());
                Ok(endpoint)
            }
        }
    }

    /// Reads the port's discovery record, confirms the live Runtime's `/health` instance matches
    /// it, and reads the token file it names. Every refusal says what to do; none carries the
    /// token.
    fn discover(&self, port: u16) -> Result<Zeroizing<String>, String> {
        use graphhelm_model_gateway::transport::{HttpTransport, TransportRequest};
        let fail = |reason: &str| format!("runtime discovery (--discover): {reason}");
        let dir = crate::commands::runtime_record::registry_dir()
            .ok_or_else(|| fail("no home directory and no GRAPHHELM_RUNTIME_DIR"))?;
        let record = crate::commands::runtime_record::read(&dir, port).map_err(|e| fail(&e))?;
        let health = TransportRequest {
            method: "GET",
            url: super::url::join(&self.base_url, "/health")?,
            headers: Vec::new(),
            body: Vec::new(),
            timeout: REQUEST_TIMEOUT,
        };
        let response = self.transport.execute(&health).map_err(|_| {
            fail(&format!(
                "no Runtime answers on port {port}; start `graphhelm serve --bind 127.0.0.1:{port}` for the project you want"
            ))
        })?;
        let body: serde_json::Value =
            serde_json::from_slice(&response.body).unwrap_or(serde_json::Value::Null);
        if body["data"]["instance"].as_str() != Some(record.instance.as_str()) {
            return Err(fail(&format!(
                "the Runtime on port {port} is not the one that published its discovery record (the record is stale, or that Runtime was started by an older build); restart `graphhelm serve --bind 127.0.0.1:{port}`"
            )));
        }
        // #380: discovery hands out the agent session token, never the owner token.
        let token = crate::commands::secret_file::read_existing(
            &crate::commands::secret_file::agent_token_beside(&record.token_file),
            "discovered agent session token",
        )
        .map_err(|e| fail(e.message()))?;
        Ok(Zeroizing::new(token))
    }

    /// Selects the one live Runtime registered for a project identity, with no port supplied by
    /// the caller. More than one live instance is ambiguous and is refused before tool data is
    /// returned or changed.
    fn discover_project(&self, project_id: &str) -> Result<DiscoveredEndpoint, String> {
        use graphhelm_model_gateway::transport::{HttpTransport, TransportRequest};
        use std::time::Duration;

        const HEALTH_TIMEOUT: Duration = Duration::from_secs(1);
        let fail = |reason: &str| format!("project Runtime discovery: {reason}");
        let dir = crate::commands::runtime_record::registry_dir()
            .ok_or_else(|| fail("no home directory and no GRAPHHELM_RUNTIME_DIR"))?;
        let records = crate::commands::runtime_record::read_project(&dir, project_id)
            .map_err(|error| fail(&error))?;
        let mut live = Vec::new();
        for record in records {
            if !is_loopback_url(&record.url) || super::url::validate_base(&record.url).is_err() {
                continue;
            }
            let health = TransportRequest {
                method: "GET",
                url: super::url::join(&record.url, "/health")?,
                headers: Vec::new(),
                body: Vec::new(),
                timeout: HEALTH_TIMEOUT,
            };
            let Ok(response) = self.transport.execute(&health) else {
                continue;
            };
            if response.status != 200 {
                continue;
            }
            let body: serde_json::Value =
                serde_json::from_slice(&response.body).unwrap_or(serde_json::Value::Null);
            if body["data"]["instance"].as_str() == Some(record.instance.as_str())
                && body["data"]["projectId"].as_str() == Some(project_id)
            {
                live.push(record);
            }
        }
        if live.len() != 1 {
            return Err(fail(if live.is_empty() {
                "no live Runtime proved this project's identity; start `graphhelm serve --project <project>`"
            } else {
                "more than one live Runtime is registered for this project; stop the duplicate Runtime"
            }));
        }
        let record = live.pop().expect("one live record was checked");
        // #380: discovery hands out the agent session token, never the owner token.
        let token = crate::commands::secret_file::read_existing(
            &crate::commands::secret_file::agent_token_beside(&record.token_file),
            "discovered agent session token",
        )
        .map_err(|error| fail(error.message()))?;
        Ok(DiscoveredEndpoint {
            url: record.url,
            token: Zeroizing::new(token),
        })
    }

    fn request_with(
        &self,
        endpoint: &DiscoveredEndpoint,
        method: &'static str,
        path: &str,
        body: Option<&serde_json::Value>,
        settings: Settings<'_>,
    ) -> Result<(u16, serde_json::Value), String> {
        use graphhelm_model_gateway::transport::{HttpTransport, TransportRequest};
        let Settings {
            idempotency_key,
            if_match,
            timeout,
        } = settings;
        let mut headers = vec![(
            "Authorization".to_owned(),
            format!("Bearer {}", endpoint.token.as_str()),
        )];
        if let Some(key) = idempotency_key {
            headers.push(("Idempotency-Key".to_owned(), key.to_owned()));
            headers.extend(self.mutation_headers());
        }
        if let Some(head) = if_match {
            headers.push(("If-Match".to_owned(), head.to_string()));
        }
        let mut payload = Vec::new();
        if let Some(body) = body {
            headers.push(("Content-Type".to_owned(), "application/json".to_owned()));
            payload = serde_json::to_vec(body).map_err(|_| "the body serializes".to_owned())?;
        }
        let request = TransportRequest {
            method,
            url: super::url::join(&endpoint.url, path)?,
            headers,
            body: payload,
            timeout,
        };
        // The transport error's Display never has access to a header value — safe to relay.
        let response = self
            .transport
            .execute(&request)
            .map_err(|error| error.to_string())?;
        let value = serde_json::from_slice(&response.body).unwrap_or(serde_json::Value::Null);
        Ok((response.status, value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idempotency_keys_are_stable_per_rpc_id_and_bounded() {
        let nonce = "0123456789abcdef";
        let boundary = "a".repeat(DIRECT_ID_MAX);
        let over = "a".repeat(DIRECT_ID_MAX + 1);
        let ids = [
            serde_json::json!(7),
            serde_json::json!("abc"),
            serde_json::json!(boundary),
            serde_json::json!(over),
            serde_json::json!("x".repeat(200)),
            serde_json::json!("ídé-üñíçødé"),
        ];
        let mut seen = std::collections::BTreeSet::new();
        for id in &ids {
            let key = derive_key(nonce, id);
            assert_eq!(key, derive_key(nonce, id), "stable across two calls");
            assert!(key.len() <= 64, "{key} must fit the header cap");
            assert!(
                key.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "charset [a-z0-9-]: {key}"
            );
            assert!(key.starts_with(&format!("mcp-{nonce}-")));
            assert!(seen.insert(key), "distinct across distinct ids");
        }
        // The cutoff is exact: 32 chars ride verbatim, 33 digest.
        let direct = derive_key(nonce, &serde_json::json!(boundary));
        assert!(direct.ends_with(boundary.as_str()), "32 is the direct arm");
        assert_eq!(
            direct.len(),
            4 + 16 + 1 + 1 + 32,
            "the proof-by-construction bound (marker included)"
        );
        let digested = derive_key(nonce, &serde_json::json!(over));
        assert_eq!(digested.len(), 4 + 16 + 1 + 1 + 16, "33 is the digest arm");
        assert!(!digested.contains(&over), "digested, never truncated");

        // The review-found type collision is closed: 7 and "7" are distinct logical acts.
        assert_ne!(
            derive_key(nonce, &serde_json::json!(7)),
            derive_key(nonce, &serde_json::json!("7")),
            "a number and its decimal string derive distinct keys"
        );
    }

    fn url() -> String {
        "http://127.0.0.1:1".to_owned()
    }

    fn token() -> TokenSource {
        TokenSource::Fixed(Zeroizing::new("tok".to_owned()))
    }

    #[test]
    fn an_undeclared_model_emits_no_header_at_all() {
        let client = ApiClient::new(
            url(),
            token(),
            "a".into(),
            "agent".into(),
            None,
            None,
            "0123456789abcdef".to_owned(),
        );
        let names: Vec<_> = client
            .mutation_headers()
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert!(
            !names
                .iter()
                .any(|k| k.starts_with("X-GraphHelm-Actor-Model")),
            "absent must send NO header, never an empty one: an empty string is a value"
        );
        // #1057: and yet the SESSION is named, because an undeclared session that said nothing at
        // all could not be told from the previous session that reused this actor id.
        assert!(
            names.contains(&"X-GraphHelm-Actor-Session".to_owned()),
            "an undeclared session still names itself: {names:?}"
        );
    }

    #[test]
    fn a_declared_model_and_effort_ride_their_own_headers() {
        let client = ApiClient::new(
            url(),
            token(),
            "a".into(),
            "agent".into(),
            Some("claude-opus-5".into()),
            Some("medium".into()),
            "0123456789abcdef".to_owned(),
        );
        let headers = client.mutation_headers();
        assert!(headers.contains(&(
            "X-GraphHelm-Actor-Model".to_owned(),
            "claude-opus-5".to_owned()
        )));
        assert!(headers.contains(&("X-GraphHelm-Actor-Effort".to_owned(), "medium".to_owned())));
        // #1057: the session token rides EVERY mutation, declared or not.
        assert!(headers.contains(&(
            "X-GraphHelm-Actor-Session".to_owned(),
            "0123456789abcdef".to_owned()
        )));
    }

    #[test]
    fn loopback_admission_refuses_the_userinfo_bypass_shapes() {
        for good in [
            "http://127.0.0.1:8080",
            "http://localhost:3000",
            "http://[::1]:9",
            "https://127.0.0.1",
        ] {
            assert!(is_loopback_url(good), "{good} is loopback");
        }
        for bad in [
            "http://api.example.com",
            "http://[::1]@evil.com",
            "http://localhost:tok@attacker.example",
            "http://127.0.0.1.example.com",
            "ftp://127.0.0.1",
            "127.0.0.1:8080",
        ] {
            assert!(!is_loopback_url(bad), "{bad} must be refused");
        }
    }
}
