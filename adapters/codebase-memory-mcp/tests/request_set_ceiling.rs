//! ADR-034's ceiling, enforced (#628).
//!
//! ADR-028 requires a maintained MCP SDK for live provider sessions and lists *expanding
//! GraphHelm's hand-rolled MCP server into a client/session implementation* among its rejected
//! alternatives. The producer speaks the wire by hand, and the divergence was real (#628).
//!
//! ADR-034 resolves it by BOUNDING rather than permitting: a fixed, closed request set —
//! `initialize`, `notifications/initialized`, one `tools/call` — over stdio, with no negotiated
//! capabilities and no second tool, is not the session implementation ADR-026 rejected. It cannot
//! grow into one without this file going red.
//!
//! **The ceiling is the whole argument.** The reason one-shot framing is acceptable is precisely
//! that it CANNOT grow: a fourth message, a negotiated capability, a second tool, or a
//! non-stdio transport turns a bounded exchange into a client, and the ADR's acceptance stops
//! applying at exactly that point. So the acceptance is only honest if something breaks when the
//! ceiling is crossed — otherwise the next person adds the fourth message with everything green,
//! which is how the SDK clause survived a merge and every review in the first place.
//!
//! Subject: `ContainedIndexProvider::request_lines` in `src/provider.rs`. Read as SOURCE on
//! purpose: the property is "how many messages does this construct", which no runtime assertion
//! over one response can see.

const ADR: &str = "ADR-034 bounds this exchange to a closed request set: initialize, \
                   notifications/initialized, one tools/call, over stdio, with no negotiated \
                   capabilities and no second tool. Crossing that ceiling makes it the client \
                   session ADR-026 rejected, so the acceptance no longer covers it — REOPEN the \
                   decision (see #628) rather than widening this function.";

fn request_lines_source() -> String {
    let source = include_str!("../src/provider.rs");
    let start = source
        .find("fn request_lines(")
        .expect("the subject function exists; if it was renamed, this guard must follow it");
    let rest = &source[start..];
    // The function ends at the first line that closes it at the impl's indentation level.
    let end = rest
        .find("\n    }\n")
        .expect("the subject function has a body");
    rest[..end].to_owned()
}

/// THE CEILING: exactly three messages leave this producer.
#[test]
fn the_producer_sends_exactly_three_messages() {
    let body = request_lines_source();
    let messages = body.matches("\"jsonrpc\": \"2.0\"").count();

    assert_eq!(
        messages, 3,
        "the producer constructs {messages} JSON-RPC messages, not three.\n\n{ADR}"
    );
}

/// The closed method set. A method outside it is a different exchange, whatever its count.
#[test]
fn the_producer_speaks_only_the_three_named_methods() {
    let body = request_lines_source();
    for method in ["initialize", "notifications/initialized", "tools/call"] {
        assert!(
            body.contains(&format!("\"{method}\"")),
            "the producer no longer sends {method}.\n\n{ADR}"
        );
    }
    for forbidden in [
        "tools/list",
        "resources/",
        "prompts/",
        "completion/",
        "logging/",
        "notifications/cancelled",
        "ping",
    ] {
        assert!(
            !body.contains(&format!("\"{forbidden}")),
            "the producer sends {forbidden}, which is outside the closed set.\n\n{ADR}"
        );
    }
}

/// ONE tool. A second tool is a client choosing between capabilities, which is the thing the
/// rejected alternative names.
#[test]
fn the_producer_calls_exactly_one_tool() {
    let body = request_lines_source();
    let named = body.matches("\"name\": \"search_graph\"").count();
    let any_name = body.matches("\"name\":").count();

    assert_eq!(
        named, 1,
        "search_graph is not called exactly once.\n\n{ADR}"
    );
    // `clientInfo` carries the other `name`, so two is the floor and three is a second tool.
    assert!(
        any_name <= 2,
        "the producer names {any_name} things; a second tool call is a client.\n\n{ADR}"
    );
}

/// NO negotiated capabilities. An empty object is a declaration that nothing is negotiated; a
/// populated one is negotiation, which is what an SDK exists to do properly.
#[test]
fn the_producer_negotiates_no_capabilities() {
    let body = request_lines_source();
    assert!(
        body.contains("\"capabilities\": {}"),
        "the producer's capabilities object is no longer empty, so it negotiates.\n\n{ADR}"
    );
}

/// STDIO only. The transport is the session's other half: a socket or an HTTP endpoint is a
/// connection with a lifetime, which one-shot framing by definition is not.
#[test]
fn the_producer_uses_no_transport_but_stdio() {
    let source = include_str!("../src/provider.rs");
    for transport in [
        "TcpStream",
        "UnixStream",
        "reqwest",
        "hyper",
        "http://",
        "https://",
        "ws://",
        "connect(",
        "bind(",
    ] {
        assert!(
            !source.contains(transport),
            "the provider mentions {transport}: the exchange is stdio-only.\n\n{ADR}"
        );
    }
}
