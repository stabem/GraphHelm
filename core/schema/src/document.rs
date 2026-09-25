use std::fs::File;
use std::io::{Read, Take};
use std::path::Path;

use graphhelm_protocols::{Diagnostic, ExecutionGraph};

use crate::bounded_value::{BoundedValueError, ValueBudget, ValueLimits, parse_json, parse_yaml};
use crate::{validate_extension_value, validate_graph_value};

const MAX_DOCUMENT_BYTES: u64 = 4 * 1024 * 1024;
const DOCUMENT_VALUE_LIMITS: ValueLimits = ValueLimits {
    max_depth: 128,
    max_values: 128 * 1024,
    max_key_bytes: MAX_DOCUMENT_BYTES as usize,
    max_string_bytes: MAX_DOCUMENT_BYTES as usize,
};

/// A parsed, schema-validated and typed graph document.
#[derive(Clone, Debug, PartialEq)]
pub struct LoadedGraph {
    pub source: String,
    pub raw: serde_json::Value,
    pub graph: ExecutionGraph,
}

/// A parsed and schema-validated extension manifest.
#[derive(Clone, Debug, PartialEq)]
pub struct LoadedExtension {
    pub source: String,
    pub raw: serde_json::Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum YamlValueError {
    AnchorOrAlias,
    ByteOrderMark,
    LimitExceeded,
    Invalid,
}

pub(crate) fn parse_yaml_value(bytes: &[u8]) -> Result<serde_json::Value, YamlValueError> {
    let mut budget = ValueBudget::default();
    let value =
        parse_yaml(bytes, DOCUMENT_VALUE_LIMITS, &mut budget).map_err(|error| match error {
            BoundedValueError::AnchorOrAlias => YamlValueError::AnchorOrAlias,
            BoundedValueError::ByteOrderMark | BoundedValueError::Nul => {
                YamlValueError::ByteOrderMark
            }
            BoundedValueError::DepthLimit
            | BoundedValueError::ValueLimit
            | BoundedValueError::KeyBytesLimit
            | BoundedValueError::StringBytesLimit => YamlValueError::LimitExceeded,
            BoundedValueError::Invalid | BoundedValueError::InvalidUtf8 => YamlValueError::Invalid,
        })?;
    debug_assert!(budget.metrics().values <= DOCUMENT_VALUE_LIMITS.max_values);
    Ok(value)
}

/// #592: `source` reaches every diagnostic this call produces, and diagnostics reach the caller
/// verbatim — CLI stdout, and over HTTP via `serve`. An absolute local filesystem path here
/// discloses the machine's disk layout to whoever reads that output. Relative to the current
/// directory when the path is under it (the common case: a path the caller already typed
/// relative, or one under the repo/working tree); otherwise just the file name, never the full
/// path to somewhere else on disk. A path already given as relative is left untouched — it
/// discloses nothing the caller didn't already type. A path with no basename to fall back on
/// (a filesystem root, or one ending in `..`) gets a fixed placeholder instead of the absolute
/// path itself — the fallback exists to avoid disclosure, so it must never re-disclose.
fn diagnostic_source(path: &Path) -> String {
    if !path.is_absolute() {
        return path.to_string_lossy().into_owned();
    }
    if let Ok(cwd) = std::env::current_dir()
        && let Ok(relative) = path.strip_prefix(&cwd)
    {
        return relative.to_string_lossy().into_owned();
    }
    match path.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => "<external>".to_owned(),
    }
}

/// Loads one bounded JSON extension manifest and validates it offline.
pub fn load_extension(path: &Path) -> Result<LoadedExtension, Vec<Diagnostic>> {
    let source = diagnostic_source(path);
    if path.extension().and_then(|value| value.to_str()) != Some("json") {
        return Err(vec![parse_diagnostic(
            &source,
            "unsupported extension manifest; expected .json",
        )]);
    }
    let file = File::open(path).map_err(|error| {
        vec![parse_diagnostic(
            &source,
            format!("cannot read extension manifest: {error}"),
        )]
    })?;
    let metadata = file.metadata().map_err(|error| {
        vec![parse_diagnostic(
            &source,
            format!("cannot inspect extension manifest: {error}"),
        )]
    })?;
    if metadata.len() > MAX_DOCUMENT_BYTES {
        return Err(vec![parse_diagnostic(
            &source,
            "extension manifest exceeds the 4 MiB input limit",
        )]);
    }
    let mut contents = String::new();
    let mut bounded: Take<File> = file.take(MAX_DOCUMENT_BYTES + 1);
    bounded.read_to_string(&mut contents).map_err(|error| {
        vec![parse_diagnostic(
            &source,
            format!("cannot decode extension manifest: {error}"),
        )]
    })?;
    if contents.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err(vec![parse_diagnostic(
            &source,
            "extension manifest exceeds the 4 MiB input limit",
        )]);
    }
    let mut budget = ValueBudget::default();
    let raw =
        parse_json(contents.as_bytes(), DOCUMENT_VALUE_LIMITS, &mut budget).map_err(|error| {
            vec![parse_diagnostic(
                &source,
                extension_json_error_message(error),
            )]
        })?;
    debug_assert!(budget.metrics().values <= DOCUMENT_VALUE_LIMITS.max_values);
    let diagnostics = validate_extension_value(&raw, &source);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    Ok(LoadedExtension { source, raw })
}

/// Loads a JSON graph document from BYTES ALREADY IN MEMORY and validates it offline — the same
/// parse, bounds and validation `load_graph` performs, minus the filesystem.
///
/// This exists for callers who never had a file: an HTTP request whose body carries the graph
/// itself, so a browser can start work without first writing a document onto the server's disk.
/// The bound is not weakened by the missing file — `load_graph_bytes` re-checks the 4 MiB limit
/// against the slice it is handed rather than trusting a caller's stated length, which is the
/// check that matters when the length is attacker-supplied.
///
/// JSON ONLY, deliberately. `load_graph`'s YAML arm is selected by a file EXTENSION; bytes have
/// no extension, so admitting YAML here would mean inventing a format parameter and trusting a
/// caller to describe their own payload. Callers with YAML have a file, and `load_graph` reads it.
///
/// `source` is the label diagnostics are reported against. Callers without a filesystem path must
/// pass one that is not a path (see the HTTP start handler): it reaches lint diagnostics, and a
/// server-side path in a reply tells a remote caller about a disk they cannot see.
pub fn load_graph_json(contents: &[u8], source: &str) -> Result<LoadedGraph, Vec<Diagnostic>> {
    load_graph_bytes(contents, "json", source)
}

/// Loads YAML or JSON from a bounded local file and validates it offline.
pub fn load_graph(path: &Path) -> Result<LoadedGraph, Vec<Diagnostic>> {
    let source = diagnostic_source(path);
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    if !matches!(
        extension.to_ascii_lowercase().as_str(),
        "json" | "yaml" | "yml"
    ) {
        return Err(vec![parse_diagnostic(
            &source,
            "unsupported graph extension; expected .json, .yaml, or .yml",
        )]);
    }

    // STAT BEFORE OPEN (#559). The size bound below only ever protected against a big
    // regular file: a FIFO reports length 0 and then blocks at OPEN on Unix until a writer
    // appears, so nothing after the open could reject what the open never returned from -- and
    // `serve` ran this on its single-thread reactor. `metadata` does not block on a FIFO. The
    // post-open check further down repeats the question on the handle actually opened, which
    // closes the swap window between the two calls for everything but the blocking case this
    // one exists for.
    let kind = std::fs::metadata(path).map_err(|error| {
        vec![parse_diagnostic(
            &source,
            format!("cannot inspect graph: {error}"),
        )]
    })?;
    if !kind.is_file() {
        return Err(vec![parse_diagnostic(&source, NOT_A_REGULAR_FILE)]);
    }

    let file = File::open(path).map_err(|error| {
        vec![parse_diagnostic(
            &source,
            format!("cannot read graph: {error}"),
        )]
    })?;
    let metadata = file.metadata().map_err(|error| {
        vec![parse_diagnostic(
            &source,
            format!("cannot inspect graph: {error}"),
        )]
    })?;
    if !metadata.is_file() {
        return Err(vec![parse_diagnostic(&source, NOT_A_REGULAR_FILE)]);
    }
    if metadata.len() > MAX_DOCUMENT_BYTES {
        return Err(vec![parse_diagnostic(
            &source,
            "graph exceeds the 4 MiB input limit",
        )]);
    }

    let mut contents = Vec::with_capacity(metadata.len() as usize);
    let mut bounded: Take<File> = file.take(MAX_DOCUMENT_BYTES + 1);
    bounded.read_to_end(&mut contents).map_err(|error| {
        vec![parse_diagnostic(
            &source,
            format!("cannot read graph: {error}"),
        )]
    })?;
    if contents.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err(vec![parse_diagnostic(
            &source,
            "graph exceeds the 4 MiB input limit",
        )]);
    }

    load_graph_bytes(&contents, extension, &source)
}

pub(crate) fn load_graph_bytes(
    contents: &[u8],
    extension: &str,
    source: &str,
) -> Result<LoadedGraph, Vec<Diagnostic>> {
    if contents.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err(vec![parse_diagnostic(
            source,
            "graph exceeds the 4 MiB input limit",
        )]);
    }

    let raw = if extension.eq_ignore_ascii_case("json") {
        let mut budget = ValueBudget::default();
        let value = parse_json(contents, DOCUMENT_VALUE_LIMITS, &mut budget).map_err(|error| {
            vec![parse_diagnostic(
                source,
                document_value_error_message("JSON", error),
            )]
        })?;
        debug_assert!(budget.metrics().values <= DOCUMENT_VALUE_LIMITS.max_values);
        value
    } else if matches!(extension.to_ascii_lowercase().as_str(), "yaml" | "yml") {
        parse_yaml_value(contents).map_err(|error| {
            vec![parse_diagnostic(
                source,
                match error {
                    YamlValueError::AnchorOrAlias => {
                        "YAML anchors and aliases are not allowed in bounded documents"
                    }
                    YamlValueError::ByteOrderMark => {
                        "YAML byte-order marks are not allowed in bounded documents"
                    }
                    YamlValueError::LimitExceeded => "YAML document exceeds a bounded value limit",
                    YamlValueError::Invalid => "invalid YAML",
                },
            )]
        })?
    } else {
        return Err(vec![parse_diagnostic(
            source,
            "unsupported graph extension; expected .json, .yaml, or .yml",
        )]);
    };

    let diagnostics = validate_graph_value(&raw, source);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    let graph = serde_json::from_value(raw.clone()).map_err(|error| {
        vec![Diagnostic::error(
            "GHS003_TYPED",
            format!("graph is outside the typed v1 subset: {error}"),
            "/",
            source,
        )]
    })?;

    Ok(LoadedGraph {
        source: source.to_owned(),
        raw,
        graph,
    })
}

fn extension_json_error_message(error: BoundedValueError) -> &'static str {
    match error {
        BoundedValueError::DepthLimit => "extension manifest JSON depth limit exceeded",
        BoundedValueError::ValueLimit => "extension manifest JSON value limit exceeded",
        BoundedValueError::KeyBytesLimit => "extension manifest JSON key byte limit exceeded",
        BoundedValueError::StringBytesLimit => "extension manifest JSON string byte limit exceeded",
        _ => "extension manifest contains invalid JSON",
    }
}

fn document_value_error_message(format: &str, error: BoundedValueError) -> String {
    match error {
        BoundedValueError::DepthLimit => format!("{format} document depth limit exceeded"),
        BoundedValueError::ValueLimit => format!("{format} document value limit exceeded"),
        BoundedValueError::KeyBytesLimit => format!("{format} document key byte limit exceeded"),
        BoundedValueError::StringBytesLimit => {
            format!("{format} document string byte limit exceeded")
        }
        _ => format!("invalid {format}"),
    }
}

fn parse_diagnostic(source: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error("GHS001_PARSE", message, "/", source)
}

/// The diagnostic a graph path that is not a regular file earns (#559): a directory, a FIFO, a
/// device. One string, so the two checks in `load_graph` and the cells cannot drift apart.
pub const NOT_A_REGULAR_FILE: &str = "graph is not a regular file";

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh directory under the OS temp root, removed on drop. This crate carries no
    /// `tempfile`; the name is unique per process and per call.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new() -> Self {
            static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("graphhelm-schema-{}-{unique}", std::process::id()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A directory is not a graph, and the answer is the regular-file diagnostic rather than
    /// whatever the platform's `read` says about directories (#559).
    #[test]
    fn a_directory_is_refused_as_not_a_regular_file_before_any_read() {
        let scratch = Scratch::new();
        let path = scratch.0.join("graph.yaml");
        std::fs::create_dir(&path).unwrap();
        let diagnostics = load_graph(&path).expect_err("a directory must not load");
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].code, "GHS001_PARSE");
        assert_eq!(diagnostics[0].message, NOT_A_REGULAR_FILE);
    }

    /// THE HANG THIS EXISTS TO REMOVE. A FIFO with no writer blocks `File::open` forever on
    /// Unix, and before #559 `load_graph` opened before it looked. The load runs on its own
    /// thread and the test waits with a ceiling, so a regression is a red rather than a hung
    /// gate -- a hang has no colour.
    #[cfg(unix)]
    #[test]
    fn a_fifo_is_refused_without_opening_it() {
        let scratch = Scratch::new();
        let path = scratch.0.join("graph.yaml");
        let c_path = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);

        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(load_graph(&path).map(|_| ()));
        });
        let outcome = receiver
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("load_graph blocked on the FIFO: it opened before it inspected");
        let diagnostics = outcome.expect_err("a FIFO must not load");
        assert_eq!(
            diagnostics[0].message, NOT_A_REGULAR_FILE,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn one_leading_utf8_bom_is_normalized_before_yaml_parsing() {
        let value = parse_yaml_value(b"\xEF\xBB\xBFvalue: accepted\n").unwrap();
        assert_eq!(value["value"], "accepted");
    }

    #[test]
    fn utf16_bom_documents_are_rejected_before_yaml_materialization() {
        let source = "shared: &shared value\nalias: *shared\n";
        for mut bytes in [vec![0xFF, 0xFE], vec![0xFE, 0xFF]] {
            for code_unit in source.encode_utf16() {
                if bytes.starts_with(&[0xFF, 0xFE]) {
                    bytes.extend_from_slice(&code_unit.to_le_bytes());
                } else {
                    bytes.extend_from_slice(&code_unit.to_be_bytes());
                }
            }
            assert_eq!(parse_yaml_value(&bytes), Err(YamlValueError::Invalid));
        }
    }

    #[test]
    fn an_embedded_utf8_bom_cannot_hide_yaml_alias_syntax() {
        assert_eq!(
            parse_yaml_value(b"shared: \xEF\xBB\xBF&shared value\nalias: \xEF\xBB\xBF*shared\n"),
            Err(YamlValueError::ByteOrderMark)
        );
    }

    #[test]
    fn graph_json_value_limit_is_reported_during_parsing() {
        let contents = format!("[{}]", vec!["null"; 128 * 1024].join(","));

        let diagnostics = load_graph_bytes(contents.as_bytes(), "json", "bounded.json")
            .expect_err("the excess value must be rejected before schema validation");

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "GHS001_PARSE");
        assert_eq!(diagnostics[0].message, "JSON document value limit exceeded");
    }

    #[test]
    fn normal_graph_and_extension_documents_still_parse() {
        let graph = load_graph_bytes(
            include_bytes!("../../../examples/graphs/software-feature.yaml"),
            "yaml",
            "software-feature.yaml",
        )
        .unwrap();
        assert_eq!(graph.raw["kind"], "ExecutionGraph");

        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../extensions/builtin/graphhelm-jpd/extension.json");
        let extension = load_extension(&manifest).unwrap();
        assert_eq!(extension.raw["metadata"]["id"], "graphhelm-jpd");
    }

    #[test]
    fn diagnostic_source_leaves_an_already_relative_path_untouched() {
        let path = Path::new("examples/graphs/software-feature.yaml");
        assert_eq!(
            diagnostic_source(path),
            "examples/graphs/software-feature.yaml"
        );
    }

    #[test]
    fn diagnostic_source_strips_the_absolute_prefix_for_a_path_under_the_working_directory() {
        let cwd = std::env::current_dir().unwrap();
        let path = cwd
            .join("examples")
            .join("graphs")
            .join("software-feature.yaml");

        let redacted = diagnostic_source(&path);

        assert!(
            !redacted.contains(&cwd.to_string_lossy().into_owned()),
            "redacted source must not carry the working-directory prefix: {redacted}"
        );
        assert_eq!(
            redacted,
            Path::new("examples")
                .join("graphs")
                .join("software-feature.yaml")
                .to_string_lossy()
                .into_owned()
        );
    }

    #[test]
    fn diagnostic_source_falls_back_to_the_file_name_outside_the_working_directory() {
        let outside = std::env::temp_dir()
            .join("graphhelm-592-unrelated")
            .join("secret-project")
            .join("graph.yaml");

        let redacted = diagnostic_source(&outside);

        assert_eq!(redacted, "graph.yaml");
        assert!(!redacted.contains(std::path::MAIN_SEPARATOR));
    }

    #[test]
    fn diagnostic_source_does_not_restore_the_absolute_path_when_no_basename_exists() {
        let outside = std::env::temp_dir().join("graphhelm-592-noname").join("..");
        assert!(
            outside.file_name().is_none(),
            "arrangement check: a path ending in `..` must have no basename"
        );

        let redacted = diagnostic_source(&outside);

        assert!(
            !redacted.contains(&outside.to_string_lossy().into_owned()),
            "redacted source must not fall back to the absolute path when there is no basename: {redacted}"
        );
    }

    #[test]
    fn load_graph_error_diagnostics_do_not_carry_the_working_directory_prefix() {
        let cwd = std::env::current_dir().unwrap();
        let missing = cwd.join("does-not-exist-592.yaml");

        let diagnostics = load_graph(&missing).expect_err("a nonexistent path must fail to load");

        assert_eq!(diagnostics.len(), 1);
        assert!(
            !diagnostics[0]
                .source
                .contains(&cwd.to_string_lossy().into_owned()),
            "error diagnostic source must not carry the working-directory prefix: {}",
            diagnostics[0].source
        );
        assert_eq!(diagnostics[0].source, "does-not-exist-592.yaml");
    }
}
