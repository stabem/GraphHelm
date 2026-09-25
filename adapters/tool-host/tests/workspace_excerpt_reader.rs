//! #1065: the production `BoundedSourceReader` never leaves the project root, never follows a
//! link, never pulls a byte past the bound, and reads deterministically.

use std::path::Path;

use graphhelm_runtime::ports::{BoundedSourceReader, SourceReadError};
use graphhelm_tool_host::source_reader::WorkspaceExcerptReader;

fn project() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("project/src")).unwrap();
    std::fs::write(
        directory.path().join("project/src/alpha.rs"),
        b"fn alpha() {}\n",
    )
    .unwrap();
    std::fs::write(directory.path().join("outside.txt"), b"OUTSIDE-MARKER\n").unwrap();
    directory
}

#[test]
fn a_prefix_read_returns_the_bytes_and_the_full_length() {
    let directory = project();
    let reader = WorkspaceExcerptReader::open(&directory.path().join("project")).unwrap();
    let whole = reader.read_prefix("src/alpha.rs", 1024).unwrap();
    assert_eq!(whole.bytes, b"fn alpha() {}\n");
    assert_eq!(whole.file_len, 14);

    let prefix = reader.read_prefix("src/alpha.rs", 5).unwrap();
    assert_eq!(prefix.bytes, b"fn al", "never a byte past the bound");
    assert_eq!(prefix.file_len, 14, "the full length is still declared");
    assert_eq!(
        reader.read_prefix("src/alpha.rs", 5).unwrap(),
        prefix,
        "the same read twice is the same excerpt"
    );
}

#[test]
fn every_escape_spelling_is_refused_before_anything_is_opened() {
    let directory = project();
    let reader = WorkspaceExcerptReader::open(&directory.path().join("project")).unwrap();
    let absolute = directory
        .path()
        .join("outside.txt")
        .to_str()
        .unwrap()
        .replace('\\', "/");
    for path in [
        "../outside.txt",
        "src/../../outside.txt",
        "./src/alpha.rs",
        "src\\alpha.rs",
        "/outside.txt",
        absolute.as_str(),
        "",
    ] {
        assert_eq!(
            reader.read_prefix(path, 1024),
            Err(SourceReadError::Escape),
            "{path:?} must be refused as an escape"
        );
    }
}

#[test]
fn a_directory_or_a_missing_file_is_unreadable_not_empty() {
    let directory = project();
    let reader = WorkspaceExcerptReader::open(&directory.path().join("project")).unwrap();
    assert_eq!(
        reader.read_prefix("src", 1024),
        Err(SourceReadError::Unreadable)
    );
    assert_eq!(
        reader.read_prefix("src/missing.rs", 1024),
        Err(SourceReadError::Unreadable)
    );
}

#[cfg(unix)]
#[test]
fn a_symlink_inside_the_root_pointing_outside_is_refused() {
    let directory = project();
    let root = directory.path().join("project");
    std::os::unix::fs::symlink(
        directory.path().join("outside.txt"),
        root.join("src/link.txt"),
    )
    .unwrap();
    let reader = WorkspaceExcerptReader::open(&root).unwrap();
    assert_eq!(
        reader.read_prefix("src/link.txt", 1024),
        Err(SourceReadError::Escape)
    );
}

#[cfg(windows)]
#[test]
fn a_junction_inside_the_root_pointing_outside_is_refused() {
    let directory = project();
    let root = directory.path().join("project");
    std::fs::create_dir_all(directory.path().join("elsewhere")).unwrap();
    std::fs::write(
        directory.path().join("elsewhere/secret.txt"),
        b"OUTSIDE-MARKER\n",
    )
    .unwrap();
    // A directory junction needs no Windows privilege (symlinks do); creating it via cmd is
    // harness territory, the same fixture `workspace_containment.rs` stages.
    let junction = root.join("src").join("link");
    let status = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&junction)
        .arg(directory.path().join("elsewhere"))
        .status()
        .unwrap();
    assert!(
        status.success(),
        "mklink /J must succeed to stage the junction"
    );
    let reader = WorkspaceExcerptReader::open(&root).unwrap();
    assert_eq!(
        reader.read_prefix("src/link/secret.txt", 1024),
        Err(SourceReadError::Escape)
    );
}

/// #1086 (Codex P1 on #1092): a read whose drive gave it up refuses before it opens the file.
#[test]
fn a_cancelled_read_refuses_before_it_opens_the_file() {
    let directory = project();
    let cancel = graphhelm_runtime::ports::ScanCancel::new();
    let reader = WorkspaceExcerptReader::open(&directory.path().join("project"))
        .unwrap()
        .with_cancel(cancel.clone());
    assert!(reader.read_prefix("src/alpha.rs", 1024).is_ok());
    cancel.cancel();
    assert_eq!(
        reader.read_prefix("src/alpha.rs", 1024),
        Err(SourceReadError::Unreadable)
    );
}

#[test]
fn a_root_that_is_not_a_directory_is_not_admitted() {
    let directory = project();
    assert!(WorkspaceExcerptReader::open(&directory.path().join("outside.txt")).is_err());
    assert!(WorkspaceExcerptReader::open(Path::new("this-root-does-not-exist-1065")).is_err());
}
