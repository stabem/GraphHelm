use graphhelm_tool_broker::path::{PathRuleError, RelativePath};

#[test]
fn plain_relative_paths_parse_and_normalize_to_forward_slashes() {
    let path = RelativePath::parse("src/lib.rs").unwrap();
    assert_eq!(path.as_str(), "src/lib.rs");
    // Backslashes are refused, not silently converted: one canonical spelling only, so two
    // spellings of one file can never pass two different checks.
    assert!(matches!(
        RelativePath::parse("src\\lib.rs").unwrap_err(),
        PathRuleError::BackslashSeparator
    ));
}

#[test]
fn escape_shapes_are_refused_by_form_alone() {
    for bad in [
        "../outside.txt",        // parent traversal
        "src/../../outside.txt", // embedded traversal
        "/etc/passwd",           // absolute
        "C:/Windows/system32",   // drive-absolute
        "C:relative",            // drive-relative
        "//server/share/x",      // UNC
        "",                      // empty
        ".",                     // no-op self
        "src/./lib.rs",          // dot component — one spelling only
    ] {
        assert!(RelativePath::parse(bad).is_err(), "{bad:?} must be refused");
    }
}

#[test]
fn nul_control_bytes_and_oversize_are_refused() {
    assert!(RelativePath::parse("a\0b").is_err());
    assert!(RelativePath::parse("a\nb").is_err());
    let long = "a/".repeat(3000);
    assert!(matches!(
        RelativePath::parse(&long).unwrap_err(),
        PathRuleError::TooLong { .. }
    ));
}

proptest::proptest! {
    // The declared proptest dev-dependency earns its place: whatever string parses, the accepted
    // form contains no traversal, no backslash, no control byte, and round-trips through as_str
    // unchanged — the "one spelling only" rule as a property rather than a case list.
    #[test]
    fn an_accepted_path_is_always_in_canonical_form(candidate in ".{0,128}") {
        if let Ok(path) = RelativePath::parse(&candidate) {
            let text = path.as_str();
            proptest::prop_assert_eq!(text, candidate.as_str());
            proptest::prop_assert!(!text.contains('\\'));
            proptest::prop_assert!(!text.starts_with('/'));
            proptest::prop_assert!(!text.split('/').any(|c| c.is_empty() || c == "." || c == ".."));
            proptest::prop_assert!(!text.bytes().any(|b| b < 0x20));
        }
    }
}

#[test]
fn program_names_are_bare_names_never_paths() {
    use graphhelm_tool_broker::path::validate_program_name;
    assert!(validate_program_name("git").is_ok());
    assert!(validate_program_name("cargo").is_ok());
    // A path smuggles the workspace's own content (or anything on disk) into the "program"
    // position, bypassing the lease's program allowlist by construction. Names only; the OS PATH
    // does the resolution in the host.
    for bad in [
        "./git",
        "bin/git",
        "C:/tools/git.exe",
        "git.exe/",
        "",
        "gi t",
    ] {
        assert!(
            validate_program_name(bad).is_err(),
            "{bad:?} must be refused"
        );
    }
}
