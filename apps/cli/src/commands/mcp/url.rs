//! URL composition for the MCP adapter: joining a configured base to an API path, and building
//! that path out of caller-supplied segments.
//!
//! Everything this module refuses, it refuses by returning an error rather than by repairing the
//! input. A base carrying a fragment is a configuration mistake; stripping it silently would leave
//! the operator believing they configured something that never reaches the wire.

/// Build an API path from segments, percent-encoding each one so a segment can never leave itself.
///
/// The allowlist is RFC 3986's `pchar` minus the three delimiters that would change what the URL
/// addresses: `/` ends the segment, `?` starts the query, `#` starts a fragment. `%` is encoded too,
/// because an unencoded one makes the rest of the segment read as an escape that was never written.
///
/// Written as an allowlist rather than a denylist on purpose. A denylist is a claim that the author
/// enumerated every dangerous byte; an allowlist is a claim that they enumerated the safe ones, and
/// the failure mode of an incomplete allowlist is an over-encoded URL that breaks loudly rather than
/// a under-encoded one that reaches the wrong endpoint quietly.
pub(crate) fn segment_path(segments: &[&str]) -> String {
    let mut out = String::new();
    for segment in segments {
        out.push('/');
        for byte in segment.bytes() {
            if is_safe_in_segment(byte) {
                out.push(byte as char);
            } else {
                out.push('%');
                out.push_str(&format!("{byte:02X}"));
            }
        }
    }
    out
}

/// Percent-encode one QUERY-STRING VALUE.
///
/// A STRICTER allowlist than [`is_safe_in_segment`], and deliberately a second function rather
/// than a reuse: `pchar` permits the sub-delims `&` and `=`, which are exactly the two bytes that
/// end a query value. A cursor containing either one, encoded with the path rule, would split
/// into a second parameter the caller never wrote. Unreserved characters only here - anything
/// else is escaped, so an over-encoded value round-trips through the server's decoder unchanged
/// while an under-encoded one cannot exist.
pub(crate) fn encode_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push_str(&format!("{byte:02X}"));
        }
    }
    out
}

/// RFC 3986 `pchar` = unreserved / sub-delims / ":" / "@", minus `%` which must always be escaped.
const fn is_safe_in_segment(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'-' | b'.'
                | b'_'
                | b'~'
                | b'!'
                | b'$'
                | b'&'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
                | b':'
                | b'@'
        )
}

/// What a refusal is allowed to quote back.
///
/// #214's invariants say no credential appears in a URL, a diagnostic, or a log — and this change
/// is what taught the adapter to accept a base carrying a query, so the feature itself opened the
/// path by which a `token` or `tenant` parameter could reach a refusal message.
///
/// Scheme and host only. **Userinfo is stripped as well**, because `http://user:password@host`
/// keeps the secret in the authority, and redacting to "scheme plus authority" would still print
/// it. An unparseable base has nothing safe to quote, so it is described rather than shown.
fn redacted(base: &str) -> String {
    let Some((scheme, rest)) = base.split_once("://") else {
        return "the configured --url".to_owned();
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_userinfo, host)| host);
    if host.is_empty() {
        format!("`{scheme}://` with no host")
    } else {
        format!("`{scheme}://{host}`")
    }
}

/// Percent-encode one query VALUE.
///
/// The allowlist here is narrower than the one for path segments, and deliberately so: inside a
/// query, `&` separates parameters, `=` separates a name from its value, `+` is a space by
/// convention, and `#` ends the URL. A value that can emit any of them can add a parameter the
/// caller never wrote, or truncate the request before it leaves the process.
///
/// So this admits only RFC 3986 `unreserved` -- alphanumerics and `-._~` -- and encodes everything
/// else. That over-encodes a few characters a server would have accepted, and that direction is the
/// one to be wrong in: an over-encoded value comes back as a miss the caller can see, while an
/// under-encoded one silently becomes a different request.
pub(crate) fn query_value(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push_str(&format!("{byte:02X}"));
        }
    }
    out
}

/// Admit or refuse a configured base URL, before any request is composed from it.
///
/// Separate from `join` so the CLI can refuse **once at startup**, next to the loopback check,
/// rather than failing every request with the same message. A configuration error that surfaces
/// per-call reads as an intermittent fault; the same error at startup reads as what it is.
///
/// Nothing here repairs. A fragment is never transmitted, so a base carrying one describes a
/// request that cannot happen -- stripping it silently would leave the configured URL and the
/// requested URL different without saying so.
pub(crate) fn validate_base(base: &str) -> Result<(), String> {
    let Some((scheme, rest)) = base.split_once("://") else {
        return Err(format!(
            "{} is not an absolute URL: an MCP base must start with http:// or https://",
            redacted(base)
        ));
    };
    if !matches!(scheme, "http" | "https") {
        return Err(format!(
            "`{scheme}` is not a supported scheme: an MCP base must be http or https"
        ));
    }
    if base.contains('#') {
        return Err(format!(
            "{} carries a fragment. A fragment is never sent to a server, so a base with one \
             describes a request that cannot happen -- it is refused rather than stripped, because \
             stripping it silently would leave the configured URL and the requested URL different \
             without saying so.",
            redacted(base)
        ));
    }
    let authority = rest.split(['/', '?']).next().unwrap_or_default();
    if authority.is_empty() {
        return Err(format!(
            "{} has no authority: an MCP base must name a host",
            redacted(base)
        ));
    }
    Ok(())
}

/// Join a configured base URL to an API path, preserving the base's prefix and query.
///
/// Parsed by hand rather than with a URL crate, following `is_loopback_url` in the sibling module:
/// this repository already splits on `://`, `/`, `?` and `#` deliberately, and the whole workspace
/// declares its dependencies through one table that is outside this issue's scope. What is needed
/// here is narrow — scheme, authority presence, fragment absence, query split — and none of it is
/// the part of URL handling that deserves a parser.
///
/// Refusals are refusals, never repairs. A base carrying a fragment describes something that cannot
/// happen: fragments are not transmitted. Stripping it quietly would leave the operator believing
/// the configuration they wrote is the one in force.
pub(crate) fn join(base: &str, path: &str) -> Result<String, String> {
    validate_base(base)?;
    let (scheme, rest) = base
        .split_once("://")
        .expect("validate_base admitted the scheme");

    let (authority_and_path, base_query) = rest.split_once('?').map_or((rest, None), |(l, r)| {
        (l, if r.is_empty() { None } else { Some(r) })
    });
    // No emptiness check here: `validate_base` owns that refusal. A second copy would be a
    // redundant blade -- it would keep this function's tests green while the real guard was
    // removed, which is the shape that makes a sabotage prove nothing.
    let authority = authority_and_path.split('/').next().unwrap_or_default();

    let base_path = authority_and_path[authority.len()..].trim_end_matches('/');
    let (api_path, api_query) = path.split_once('?').map_or((path, None), |(l, r)| {
        (l, if r.is_empty() { None } else { Some(r) })
    });

    // Dot-segments are refused here rather than encoded in `segment_path`, and the unit is the
    // reason. The segment allowlist prices BYTES, and `.` belongs in it -- real identifiers carry
    // dots. But an HTTP stack removes `.` and `..` SEGMENTS before sending, so a caller-supplied id
    // of `..` still chooses the endpoint without containing a single illegal byte. The rule has to
    // be written in the unit the threat lives in.
    //
    // `join` is the single chokepoint every composed URL passes through, so one rule covers the
    // thirteen call sites and a configured base prefix too. In `segment_path` it would need a
    // `Result` at thirteen closures and would still miss the prefix.
    if let Some(bad) = base_path
        .split('/')
        .chain(api_path.split('/'))
        .find(|segment| *segment == "." || *segment == "..")
    {
        return Err(format!(
            "the composed path contains a `{bad}` segment, which an HTTP stack removes before \
             sending -- so the request would reach an endpoint other than the one named. Dot \
             segments are refused rather than encoded: a dot INSIDE a segment is ordinary content \
             and stays legal."
        ));
    }

    let mut composed = format!("{scheme}://{authority}{base_path}{api_path}");
    // The base's parameters come first and the request's follow, so a base configured with a tenant
    // or routing parameter keeps it no matter what the call adds.
    match (base_query, api_query) {
        (Some(base_q), Some(api_q)) => {
            composed.push('?');
            composed.push_str(base_q);
            composed.push('&');
            composed.push_str(api_q);
        }
        (Some(only), None) | (None, Some(only)) => {
            composed.push('?');
            composed.push_str(only);
        }
        (None, None) => {}
    }
    Ok(composed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The positive control, first by intent: the base the built-in package uses today must keep
    /// composing byte-identically.
    ///
    /// Every refusal below is satisfied by a composer that refuses everything, and every encoding
    /// case is satisfied by one that encodes so aggressively the ordinary path stops working. This
    /// is the case that stops both.
    #[test]
    fn the_loopback_root_composes_exactly_as_it_does_today() {
        assert_eq!(
            join("http://127.0.0.1:8080", "/v1/executions/abc").unwrap(),
            "http://127.0.0.1:8080/v1/executions/abc"
        );
        assert_eq!(
            join("http://127.0.0.1:8080/", "/v1/executions/abc").unwrap(),
            "http://127.0.0.1:8080/v1/executions/abc",
            "a trailing slash on the base must not change the result -- the join is normalised, \
             not concatenated"
        );
        assert_eq!(
            segment_path(&["v1", "executions", "abc"]),
            "/v1/executions/abc",
            "an ordinary id must survive encoding unchanged, or every call site breaks"
        );
    }

    /// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **placing a
    /// caller-supplied id into a path without encoding it**, which is what `format!` does today.
    ///
    /// The id arrives in the MCP tool-call arguments. With `/` in it the request reaches a
    /// different endpoint entirely; the CLI believes it asked about one execution and the server
    /// answers about another resource. This is endpoint confusion driven by data, and it needs no
    /// reverse proxy to reach.
    #[test]
    fn a_slash_in_an_id_cannot_change_the_endpoint() {
        let path = segment_path(&["v1", "executions", "a/../../admin"]);
        assert_eq!(
            path, "/v1/executions/a%2F..%2F..%2Fadmin",
            "the id must be encoded as ONE segment. It composed to `{path}`, which addresses a \
             different endpoint than the caller named."
        );
    }

    /// THE PRODUCTION CHANGE THAT MAKES THIS FAIL: **leaving `?` unencoded in a path segment.**
    ///
    /// Everything after the `?` becomes a query the caller never wrote, and the path the server
    /// matches is shorter than the one this client believes it requested.
    #[test]
    fn a_question_mark_in_an_id_cannot_start_a_query() {
        let path = segment_path(&["v1", "executions", "a?x=1"]);
        assert_eq!(
            path, "/v1/executions/a%3Fx=1",
            "the id must not be able to open a query; it composed to `{path}`"
        );
    }

    /// THE PRODUCTION CHANGE THAT MAKES THIS FAIL: **leaving `#` unencoded in a path segment.**
    ///
    /// A fragment is never transmitted. Everything after it is dropped before the request leaves
    /// the process, so the client asks for less than it thinks and no error says so.
    #[test]
    fn a_hash_in_an_id_cannot_truncate_the_request() {
        let path = segment_path(&["v1", "executions", "a#frag"]);
        assert_eq!(
            path, "/v1/executions/a%23frag",
            "the id must not be able to truncate the path; it composed to `{path}`"
        );
    }

    /// THE PRODUCTION CHANGE THAT MAKES THIS FAIL: **appending the API path to a base that carries
    /// a query**, which lands the whole path inside the query string.
    ///
    /// Today `http://127.0.0.1:8080/?tenant=a` plus `/v1/executions/abc` produces
    /// `http://127.0.0.1:8080/?tenant=a/v1/executions/abc` — a request to `/` with a nonsense
    /// parameter, which no server answers as intended and nothing reports as wrong.
    #[test]
    fn a_base_query_is_preserved_and_the_path_lands_in_the_path() {
        let composed = join("http://127.0.0.1:8080/?tenant=a", "/v1/executions/abc").unwrap();
        assert_eq!(
            composed, "http://127.0.0.1:8080/v1/executions/abc?tenant=a",
            "the base's parameters must survive and the API path must land in the PATH; it \
             composed to `{composed}`"
        );
    }

    /// THE PRODUCTION CHANGE THAT MAKES THIS FAIL: **accepting a base with a fragment.**
    ///
    /// Refused rather than stripped. A fragment never reaches a server, so a base carrying one
    /// describes something that cannot happen — and repairing it quietly would leave the operator
    /// believing the configuration they wrote is the one in force.
    #[test]
    fn a_base_carrying_a_fragment_is_refused_rather_than_repaired() {
        let refusal = join("http://127.0.0.1:8080/#frag", "/v1/executions/abc")
            .expect_err("a base with a fragment must be refused");
        assert!(
            refusal.contains("fragment"),
            "the refusal must name what is wrong with the base; it said: {refusal}"
        );
    }

    /// THE PRODUCTION CHANGE THAT MAKES THIS FAIL: **accepting a base that is not an absolute
    /// http(s) URL**, which would make the composed request address something unpredictable.
    #[test]
    fn a_base_that_is_not_an_absolute_http_url_is_refused() {
        for bad in ["127.0.0.1:8080", "file:///etc/passwd", "/v1", ""] {
            assert!(
                join(bad, "/v1/executions/abc").is_err(),
                "`{bad}` is not an absolute http(s) base and must be refused"
            );
        }
    }

    /// THE PRODUCTION CHANGE THAT MAKES THIS FAIL: **dropping a configured path prefix.**
    ///
    /// The prefix is the whole point of a reverse-proxy deployment. Losing it sends every request
    /// to the proxy's root, where something else may well answer.
    #[test]
    fn a_base_path_prefix_is_preserved() {
        assert_eq!(
            join("http://127.0.0.1:8080/graphhelm", "/v1/executions/abc").unwrap(),
            "http://127.0.0.1:8080/graphhelm/v1/executions/abc"
        );
        assert_eq!(
            join("http://127.0.0.1:8080/graphhelm/", "/v1/executions/abc").unwrap(),
            "http://127.0.0.1:8080/graphhelm/v1/executions/abc",
            "with or without the trailing slash must give the same URL"
        );
    }

    // -----------------------------------------------------------------------------------------
    // Query values, found late and by someone else.
    //
    // This module's first version claimed the query half was "safe by TYPE, not by care" -- the
    // only values built were `after` and `limit`, both read as u64. That claim was false, and the
    // reason it survived is the shape this repository keeps catching: **the population was the
    // instrument.** The substitution guard asserted that no site of the form
    // `format!("/v1/executions/{var}...")` remained, and it was exhaustive over that population.
    // The file held fifteen `/v1` sites, not thirteen, and the two outside my pattern were the two
    // that interpolate caller-supplied STRINGS into a query.
    //
    // L counted the wide population from the blob and asked about 13 against 15. The gap was real.
    // -----------------------------------------------------------------------------------------

    /// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **placing a
    /// caller-supplied string into a query without encoding it**, which is what the gateway tools
    /// did.
    ///
    /// `&` starts another parameter. A `manifest` or `route` argument carrying one appends a
    /// parameter the caller never wrote to a request the Runtime API will answer.
    #[test]
    fn an_ampersand_in_a_query_value_cannot_add_a_parameter() {
        let encoded = query_value("abc&admin=true");
        assert_eq!(
            encoded, "abc%26admin%3Dtrue",
            "the value must not be able to open another parameter; it encoded to `{encoded}`"
        );
    }

    /// THE PRODUCTION CHANGE THAT MAKES THIS FAIL: **leaving `#` unencoded in a query value.**
    ///
    /// Everything after it is dropped before the request leaves the process, so the client asks
    /// for less than it believes and nothing reports the difference.
    #[test]
    fn a_hash_in_a_query_value_cannot_truncate_the_request() {
        let encoded = query_value("abc#rest");
        assert_eq!(encoded, "abc%23rest");
    }

    /// THE PRODUCTION CHANGE THAT MAKES THIS FAIL: **leaving a space or a `+` unencoded.**
    ///
    /// In a query, `+` is a space by convention, so an unencoded `+` silently becomes something
    /// the caller did not send, and a raw space is not legal in a URL at all.
    #[test]
    fn spaces_and_plus_signs_survive_a_round_trip_unambiguously() {
        assert_eq!(query_value("a b"), "a%20b");
        assert_eq!(query_value("a+b"), "a%2Bb");
    }

    /// The positive control, and it is not optional: a plain value must pass through untouched, or
    /// every real gateway call breaks while the hostile cases above stay green.
    #[test]
    fn an_ordinary_query_value_is_unchanged() {
        assert_eq!(query_value("routes-manifest-v1"), "routes-manifest-v1");
        assert_eq!(query_value("abc123"), "abc123");
    }

    // -----------------------------------------------------------------------------------------
    // Two findings from L's review of #301, both of them about the UNIT a rule is written in.
    // -----------------------------------------------------------------------------------------

    /// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **treating a
    /// dot-segment as ordinary content because its bytes are legal.**
    ///
    /// `.` belongs in the segment allowlist — it appears in real identifiers — so the encoder is
    /// byte-correct and byte-blind. But an HTTP stack removes `.` and `..` segments *before*
    /// sending, so a caller-supplied id of `..` still chooses the endpoint, without containing
    /// `/`, `?` or `#`. **The allowlist prices bytes; the threat lives in segments.** That is the
    /// same wrong-unit shape this contract exists to close, one level up from where it was written.
    ///
    /// The refusal lives in `join` rather than in `segment_path` on purpose: `join` is the single
    /// chokepoint every composed URL passes through, so one rule covers the thirteen call sites and
    /// a configured base prefix as well. Putting it in `segment_path` would need a `Result` at
    /// thirteen closures and would still miss a prefix.
    #[test]
    fn a_dot_segment_cannot_survive_into_a_composed_url() {
        let path = segment_path(&["v1", "executions", ".."]);
        assert_eq!(
            path, "/v1/executions/..",
            "the encoder is byte-correct here and that is the point: nothing in the bytes is illegal"
        );

        let refusal = join("http://127.0.0.1:8080", &path)
            .expect_err("a path with a dot-segment must be refused");
        assert!(
            refusal.contains("segment"),
            "the refusal must name the unit that is wrong; it said: {refusal}"
        );

        let single = segment_path(&["v1", "executions", "."]);
        assert!(
            join("http://127.0.0.1:8080", &single).is_err(),
            "a single-dot segment is removed by the same normalisation and must be refused too"
        );
    }

    /// The positive control for the case above, and it is what stops the fix from being a ban on
    /// the character.
    ///
    /// Identifiers contain dots. A rule that refused every segment holding one would satisfy the
    /// case above perfectly and break every real call with a versioned or dotted id.
    #[test]
    fn a_dot_inside_a_segment_is_ordinary_content() {
        let path = segment_path(&["v1", "executions", "run.2026-08-24.v2"]);
        assert_eq!(path, "/v1/executions/run.2026-08-24.v2");
        assert!(
            join("http://127.0.0.1:8080", &path).is_ok(),
            "a dot inside a segment is not a dot-segment and must compose normally"
        );
    }

    /// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **quoting the
    /// configured base back in a refusal.**
    ///
    /// #214's own invariants say no credential appears in a URL, a diagnostic, or a log. A
    /// reverse-proxy base is exactly the kind that carries a `token` or `tenant` parameter — and
    /// carrying a query in the base is a shape this change *introduced support for*. So the feature
    /// creates the leak path, and the refusal text is where it escapes.
    ///
    /// Userinfo matters as much as the query: `http://user:password@host` puts the secret in the
    /// authority, so redacting to "scheme plus authority" is not enough.
    #[test]
    fn a_refusal_never_echoes_the_configured_base() {
        for base in [
            "http://127.0.0.1:8080/?token=SECRET-VALUE#frag",
            "http://user:SECRET-VALUE@127.0.0.1:8080/#frag",
        ] {
            let refusal = join(base, "/v1/executions/abc")
                .expect_err("each of these bases is refused for its own reason");
            assert!(
                !refusal.contains("SECRET-VALUE"),
                "the refusal quoted the configured base and leaked a secret with it: {refusal}"
            );
        }

        let refusal = join("not-a-url?token=SECRET-VALUE", "/v1/executions/abc")
            .expect_err("a base with no scheme is refused");
        assert!(
            !refusal.contains("SECRET-VALUE"),
            "the unparseable case is the one with nothing safe to echo, so it must echo nothing: \
             {refusal}"
        );
    }

    /// The positive control for the redaction: a refusal that named nothing would satisfy the case
    /// above and leave the operator guessing which of their flags is wrong.
    #[test]
    fn a_refusal_still_says_enough_to_act_on() {
        let refusal = join("http://127.0.0.1:8080/#frag", "/v1/executions/abc").unwrap_err();
        assert!(
            refusal.contains("fragment"),
            "the refusal must name what is wrong: {refusal}"
        );
        assert!(
            refusal.contains("127.0.0.1"),
            "and it must name the host, which is not a secret and is how the operator recognises \
             which configuration is at fault: {refusal}"
        );
    }
}
