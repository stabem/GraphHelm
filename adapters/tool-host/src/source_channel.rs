//! The bounded source channel (#219): the second evidence path a non-complete coverage state
//! licenses, workspace-scoped and bounded like its sibling [`crate::source_reader`].
//!
//! **Why a second channel rather than a better query, measured rather than assumed.** A
//! structural index answers about what it INDEXED and parsed into constructs. Measured against
//! the shipped index, required evidence living in non-code files — JSON schemas, changelogs — is
//! unreachable through `StructuralCodeIndex` at ANY query, because the provider filters
//! non-construct nodes by design. That is a true finding about the instrument, so the compiler
//! grows a channel instead of the corpus getting easier questions.
//!
//! **What this deliberately is NOT:** a ranker, a reader, or a second index. It answers WHICH
//! files are candidate evidence and nothing else, so every existing escape check, budget and
//! bound in `compile_plan_within` applies to its output exactly as to the index's.
//!
//! **Bounds are ceilings that REFUSE.** A silently truncated search is a partial search wearing a
//! finished search's clothes — the same defect the plan compiler refuses over-budget responses
//! for. The one exception is `max_results`, which caps a search that legitimately FINISHED;
//! refusing there would make an ordinary answer look like a breached ceiling.

use std::path::{Path, PathBuf};

use graphhelm_runtime::ports::{BoundedSourceSearch, SourceSearchBounds, SourceSearchError};

/// Directory prefixes that are never repository evidence, EXCLUDED BY PRINCIPLE.
///
/// These hold the factory's own working notes: boards, blueprints, agent transcripts, process
/// plans. They match query terms extremely well precisely because they discuss the code — and
/// serving them would answer a question about the repository with the diary of the people
/// changing it. Declared here rather than tuned per query, so the exclusion is a stated property
/// of the channel and not an artifact of whichever corpus was measured last.
const EXCLUDED_PREFIXES: &[&str] = &[
    ".factory/",
    ".superpowers/",
    ".git/",
    // #1065: credential locations. The runtime's `context::sensitive_path` refuses these again
    // before any read, so a channel that forgot this list would still not ship them; listed
    // here so the WALK never opens them either.
    ".graphhelm/",
    "keyring/",
    // NARROWED (G's #622 finding): `docs/superpowers/` as a whole also holds
    // `docs/superpowers/specs/`, which carries NORMATIVE subsystem specifications — exactly the
    // kind of document a question about the system should be able to reach. The justification
    // for this list is "working notes about the work", and only the plans directory is that.
    "docs/superpowers/plans/",
];

/// Directory NAMES never entered at any depth, because they are credential locations: the
/// project's own `.graphhelm/` state (the default keyring lives under it) and any `keyring/`.
/// `EXCLUDED_PREFIXES` above covers them at the root; a nested project, a vendored copy or a
/// second checkout inside the tree carries the same directories deeper, where a prefix rule is
/// blind. The runtime's `context::sensitive_path` refuses the same names on the way to a read, so
/// the walk skipping them is the first of two locks, not the only one.
const CREDENTIAL_DIRS: &[&str] = &[".graphhelm", "keyring"];

/// Directory NAMES never entered at any depth, because they are the factory's own state rather
/// than repository evidence: the process diary and the object store. `EXCLUDED_PREFIXES` sees
/// them only at the root; a nested package (`packages/app/.factory/`) or a vendored checkout
/// (`vendor/x/.git/`) carries the same names deeper, where a prefix rule is blind. The runtime's
/// `context::sensitive_path` refuses the same names on the way to a read (`INTERNAL_DIRS`).
const PROCESS_DIRS: &[&str] = &[".factory", ".superpowers", ".git"];

/// Directory NAMES never entered at any depth, because they are an AGENT's or a TOOL's own state
/// rather than repository evidence (#1086 item 9): agent sessions, settings and — the case that
/// was measured — whole agent worktrees (`.claude/worktrees/<lane>/`), each a full copy of the
/// tree. A checkout carrying a dozen of them crossed the 50,000-entry ceiling with no code change,
/// refusing the whole search, and a worktree's stale copy of a file competed with the file itself.
/// A property of the machine the tree sits on, never of the repository, so it is skipped by name.
const AGENT_STATE_DIRS: &[&str] = &[
    ".claude",
    ".codex",
    ".cursor",
    ".windsurf",
    ".aider",
    ".worktrees",
    ".idea",
    ".vscode",
];

/// Directory NAMES never entered at any depth, because they are generated: dependency trees,
/// build output, virtual environments, caches. They hold no repository evidence, they are the
/// bulk of a checked-out tree by entry count — one `node_modules/` is tens of thousands of
/// entries — and walking them spends the traversal ceiling on files the answer cannot cite. A
/// tree that hits the ceiling refuses the whole search, so a generated directory left in the walk
/// turns a bounded search over the SOURCES into a refusal caused by the ARTIFACTS. Skipped by
/// name before the directory is pushed; the directory entry itself is still counted, so the
/// ceiling stays a ceiling.
const GENERATED_DIRS: &[&str] = &[
    "node_modules",
    "target",
    ".venv",
    "venv",
    "vendor",
    "dist",
    "build",
    "__pycache__",
    ".next",
    ".cache",
];

/// Whether a directory entry is one the walk never enters, by its own name and at any depth.
fn skipped_directory_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            let folded = name.to_lowercase();
            CREDENTIAL_DIRS.contains(&folded.as_str())
                || PROCESS_DIRS.contains(&folded.as_str())
                || AGENT_STATE_DIRS.contains(&folded.as_str())
                || GENERATED_DIRS.contains(&folded.as_str())
        })
}

/// Suffixes the channel will open. Everything else (binaries, images, archives) is skipped
/// without being read: a byte bound spent on a PNG buys nothing and the match would be noise.
/// Suffixes the channel will open.
///
/// **A DECLARED BOUND, not a claim about what text is.** A workspace in Go, Java, C#, Kotlin or
/// with extensionless files (`Dockerfile`, `Makefile`) has sources this list does not name, and
/// their absence from a result is a property of THIS LIST rather than of the repository — which
/// is the same absence-vs-silence distinction the rest of the retrieval stack refuses to blur.
/// Widened for the common languages (G's #622 finding); still a list, and callers reading a
/// negative from this channel are reading the list as much as the tree.
const TEXT_SUFFIXES: &[&str] = &[
    ".rs", ".toml", ".json", ".md", ".yaml", ".yml", ".ps1", ".sh", ".py", ".sql", ".ts", ".tsx",
    ".js", ".jsx", ".go", ".java", ".kt", ".kts", ".rb", ".cs", ".c", ".h", ".cc", ".cpp", ".hpp",
    ".swift", ".php", ".scala", ".ex", ".exs", ".proto", ".graphql", ".txt", ".cfg", ".ini",
];

/// A workspace-scoped bounded search over source bytes.
///
/// Construction admits the workspace; a root that cannot be read is [`SourceSearchError::Unavailable`]
/// rather than an empty answer, because an empty answer reads as "the repository does not contain
/// this" — the absence claim this whole mechanism exists to refuse to fabricate.
#[derive(Debug)]
pub struct WorkspaceSourceChannel {
    root: PathBuf,
    /// Checked between directory entries and before every open (#1086): a scan whose drive gave
    /// it up refuses `Unavailable` at the next check instead of finishing the walk.
    cancel: Option<graphhelm_runtime::ports::ScanCancel>,
}

impl WorkspaceSourceChannel {
    /// Admit a workspace root.
    ///
    /// # Errors
    /// [`SourceSearchError::Unavailable`] when the root is not a readable directory.
    pub fn open(root: &Path) -> Result<Self, SourceSearchError> {
        // The root that is ITSELF a link is refused (Codex #622): `is_dir()` follows, so a
        // symlinked or junction root would be stored as its link spelling while every search read
        // the TARGET outside that spelling. `symlink_metadata` does not follow the final
        // component, so a root reparse point is visible as itself and refused before anything is
        // stored.
        let metadata =
            std::fs::symlink_metadata(root).map_err(|_| SourceSearchError::Unavailable)?;
        if metadata.file_type().is_symlink() {
            return Err(SourceSearchError::Unavailable);
        }
        // ANCESTOR components are resolved, not merely the final one (Codex #622):
        // `symlink_metadata` spares only the final component, so an intermediate link
        // (`/workspace/link/project`, `link` pointing outside) would be admitted and served under
        // the link-relative spelling. `canonicalize` resolves EVERY component, so the stored root
        // is the real location and hits are consistent with the bytes read; the per-entry
        // no-follow walk then keeps the search inside that canonical tree. Resolving rather than
        // rejecting a symlinked ancestor is deliberate: rejecting ANY symlinked ancestor would
        // refuse legitimate roots on systems where a path prefix is itself a link (macOS `/tmp` ->
        // `/private/tmp`), while canonicalize removes the link-relative spelling the hazard needs.
        let real = std::fs::canonicalize(root).map_err(|_| SourceSearchError::Unavailable)?;
        if !real.is_dir() {
            return Err(SourceSearchError::Unavailable);
        }
        Ok(Self {
            root: real,
            cancel: None,
        })
    }

    /// The same channel, stopping at its next check once `cancel` is set (#1086).
    #[must_use]
    pub fn with_cancel(mut self, cancel: graphhelm_runtime::ports::ScanCancel) -> Self {
        self.cancel = Some(cancel);
        self
    }

    fn cancelled(&self) -> bool {
        self.cancel
            .as_ref()
            .is_some_and(graphhelm_runtime::ports::ScanCancel::is_cancelled)
    }

    /// Repository-relative, forward-slashed. The plan's escape checks compare TEXT, so a
    /// platform separator here would reach them as a different path.
    fn relative(&self, path: &Path) -> Result<String, SourceSearchError> {
        let stripped = path
            .strip_prefix(&self.root)
            .map_err(|_| SourceSearchError::Unavailable)?;
        // NOT `to_string_lossy`: a Unix filename that is not UTF-8 would come out with its
        // invalid bytes replaced by U+FFFD — a hit naming a path that does not exist, and two
        // distinct filenames collapsing to one string (G's #622 finding). A path this channel
        // cannot represent losslessly makes the whole search refuse, because a manufactured
        // path as EVIDENCE is worse than no answer.
        let Some(relative) = stripped.to_str() else {
            return Err(SourceSearchError::Unavailable);
        };
        // A backslash is a legal Unix filename byte, but the DOWNSTREAM consumer
        // (`compile_plan_composed_inner` → `canonical_hit`) rewrites EVERY backslash to `/`
        // unconditionally, so forwarding `dir\evidence.rs` would have the plan name
        // `dir/evidence.rs` — a different file that may itself exist, or none at all (Codex #622).
        // Not fixed by the Windows-only rewrite below: that runs the same mangling here, on the
        // one platform where `\` is a separator; on Unix it left the name untouched to be mangled
        // downstream. This channel cannot represent such a name consistently with its consumer, so
        // it REFUSES rather than mangling — the same posture as a non-UTF-8 name one line up.
        #[cfg(not(windows))]
        if relative.contains('\\') {
            return Err(SourceSearchError::Unavailable);
        }
        // Separator normalisation is a WINDOWS operation, where `\` is the path separator and the
        // rewrite is faithful. (On Unix the refusal above has already returned for any `\`.)
        #[cfg(windows)]
        let relative = relative.replace('\\', "/");
        Ok(relative.to_string())
    }
}

/// Open a candidate with the strongest promises the platform offers. Unix: `O_NOFOLLOW`
/// (a symlink swapped in after the walk's check fails the open instead of being followed)
/// and `O_NONBLOCK` (a FIFO swapped in cannot park the open waiting for a writer — the
/// unbounded block G's #622 P1 names). Regular files never return `EWOULDBLOCK` on read, so
/// the flag is free for every file this channel serves. Windows: no such open exists in std;
/// the caller's path-based re-check carries that platform's declared residual.
#[cfg(unix)]
pub(crate) fn open_candidate(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt as _;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
}

#[cfg(not(unix))]
pub(crate) fn open_candidate(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::File::open(path)
}

/// Whether the excluded `prefix` covers `relative`, folding case UNCONDITIONALLY.
///
/// Windows directory lookup is case-insensitive, so `.FACTORY` names the SAME excluded tree as
/// `.factory` and a case-sensitive comparison lets an alias walk straight past the exclusion
/// (G's #622 finding). The first fix folded case on Windows ONLY, assuming every Unix filesystem
/// is case-sensitive -- but a casefold-enabled ext4 directory, or a Linux-mounted VFAT/NTFS
/// workspace, resolves `.FACTORY` and `.factory` to one directory too (Codex #622). Detecting a
/// filesystem's case behaviour at this layer is brittle, and the cost of folding unconditionally
/// is only that a genuinely case-distinct `.FACTORY` on a case-sensitive fs is also excluded --
/// a rare directory whose non-service is harmless, weighed against leaking process notes. So the
/// exclusion folds case on every platform. The prefixes are already lowercase; only `relative`
/// needs folding.
fn excluded_by(relative: &str, prefix: &str) -> bool {
    let folded = relative.to_lowercase();
    folded.starts_with(prefix) || folded == prefix.trim_end_matches('/')
}

/// Whether `path` still names the exact file object behind the open `file`.
///
/// A regular file swapped for ANOTHER regular file after `open_candidate` passes the symlink
/// re-check -- both are regular -- so the handle reads the old inode's bytes while `relative`
/// names the replacement (G's #622 finding, the third of the swap family). On Unix the
/// `(dev, ino)` of the handle's `fstat` versus the path's `lstat` catches it deterministically,
/// and subsumes the symlink swap (a symlink has its own inode). On Windows std exposes no STABLE
/// file identity -- `file_index`/`volume_serial_number` sit behind the unstable
/// `windows_by_handle` feature -- so the regular-file swap joins the symlink swap as a DECLARED
/// residual there, and the fallback is the same symlink re-check that half-closes it. When
/// identity cannot be established the answer is `false`: an unverifiable path refuses rather
/// than assumes.
///
/// **What this deliberately does NOT catch, declared (Codex #622): same-inode content mutation.**
/// A process that overwrites or truncates the opened file IN PLACE preserves `(dev, ino)`, so this
/// check passes while the bytes that matched are already gone. Identity is not content, and no
/// path-and-identity check at this layer can close it: the only real fixes are searching an
/// IMMUTABLE snapshot (the D-042 pinned copy the containment chain already provides for the index
/// path) or re-reading and comparing — which is itself racy and doubles the cost. This channel
/// searches a LIVE workspace by construction, so in-place mutation during the read is a property
/// of that choice, declared here rather than papered over; the closure is to point the channel at
/// the pin, which is architecture, not a line.
#[cfg(unix)]
pub(crate) fn still_names_opened_file(file: &std::fs::File, path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    match (file.metadata(), std::fs::symlink_metadata(path)) {
        (Ok(handle), Ok(now)) => {
            !now.file_type().is_symlink() && handle.dev() == now.dev() && handle.ino() == now.ino()
        }
        _ => false,
    }
}

#[cfg(not(unix))]
pub(crate) fn still_names_opened_file(_file: &std::fs::File, path: &Path) -> bool {
    // No stable file identity on Windows: the symlink re-check catches a symlink swapped in but
    // NOT a regular-file swap, which stays a declared residual.
    std::fs::symlink_metadata(path)
        .map(|now| !now.file_type().is_symlink())
        .unwrap_or(false)
}

impl BoundedSourceSearch for WorkspaceSourceChannel {
    fn search(
        &self,
        terms: &[String],
        bounds: &SourceSearchBounds,
    ) -> Result<Vec<String>, SourceSearchError> {
        // An unconstrained search over a workspace is the denial of service the bounds exist for,
        // and an empty term list is a caller asking for it by accident. Nothing is not everything.
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        // An EMPTY term inside a non-empty list slips past the guard above and then
        // `contains("")` succeeds for every file, so the channel returns arbitrary paths as
        // evidence (G's #622 finding). Dropped rather than refused: a caller's blank term is
        // noise in the query, not a malformed request — but it must not become a wildcard.
        // Bounded BEFORE anything allocates or walks, because every scanned file is searched
        // once per term: an unbounded term list multiplies the whole traversal from the query
        // side, defeating ceilings the caller declared (G's #622 finding). The ceilings are the
        // CALLER's, read from `bounds`, not hard-coded constants (Codex #622): the port now
        // carries `max_terms` and an AGGREGATE `max_term_bytes`, so a caller asking for a tighter
        // budget (`max_terms == 0`) must be honoured, and a caller allowing a longer aggregate
        // must not be refused by an undeclared second limit. One struct, one oracle for the query
        // ceilings too.
        // COUNT first, O(1), before summing bytes (Codex #622): otherwise `.sum()` traverses
        // millions of terms before the count ceiling that should have stopped them is consulted.
        if terms.len() > bounds.max_terms {
            return Err(SourceSearchError::BoundExceeded);
        }
        let term_bytes: u64 = terms.iter().map(|term| term.len() as u64).sum();
        if term_bytes > bounds.max_term_bytes {
            return Err(SourceSearchError::BoundExceeded);
        }
        // A SET, not a list: the ranking counts DISTINCT terms, and a duplicated needle counted
        // as two matches — the score said something the documentation did not (G's #622
        // finding). The port does not require caller-side uniqueness, so this side supplies it.
        let needles: std::collections::BTreeSet<String> = terms
            .iter()
            .filter(|term| !term.trim().is_empty())
            .map(|term| term.to_lowercase())
            .collect();
        if needles.is_empty() {
            return Ok(Vec::new());
        }

        let mut visited_entries = 0usize;
        let mut scanned_files = 0usize;
        let mut scanned_bytes = 0u64;
        // (-distinct terms, path) so the sort is by strength first and then stable by name: a
        // deterministic order matters because these paths can end up in a frozen artifact.
        let mut scored: Vec<(usize, String)> = Vec::new();

        let mut stack = vec![self.root.clone()];
        while let Some(directory) = stack.pop() {
            // A cancelled scan stops here, between entries and before every open, and refuses:
            // an abandoned walk is an incomplete search, never a finished one (#1086).
            if self.cancelled() {
                return Err(SourceSearchError::Unavailable);
            }
            let entries =
                std::fs::read_dir(&directory).map_err(|_| SourceSearchError::Unavailable)?;
            for entry in entries {
                if self.cancelled() {
                    return Err(SourceSearchError::Unavailable);
                }
                let entry = entry.map_err(|_| SourceSearchError::Unavailable)?;
                let path = entry.path();
                // Counted BEFORE any filter: the traversal bound exists for the tree that opens
                // no files and reads no bytes — a million empty directories, or entries every
                // suffix filter rejects — which the read bounds cannot see at all (G's #622
                // finding; the field is declared in `SourceSearchBounds` so one struct is the
                // only oracle for how much walking is allowed).
                visited_entries += 1;
                if visited_entries > bounds.max_entries_visited {
                    return Err(SourceSearchError::BoundExceeded);
                }
                let relative = self.relative(&path)?;
                if EXCLUDED_PREFIXES
                    .iter()
                    .any(|prefix| excluded_by(&relative, prefix))
                {
                    continue;
                }
                // A LINK IS NOT A DIRECTORY, however much `is_dir` agrees. A directory symlink
                // or a Windows junction pointing outside would serve foreign bytes as repository
                // evidence — the containment the whole D-042 chain exists for, defeated by a
                // link. Refused by the same property `workspace::resolve_within` has refused
                // since #538: `symlink_metadata` does not follow, so the reparse point is visible
                // as itself. (G's #622 finding, and the useful shame is that this crate already
                // had the seam — my own module doc calls `source_reader` a "sibling" and I never
                // looked at what the neighbour had solved.)
                // Unreadable metadata is NOT absence (G's #622 finding): an ACL or a transient
                // filesystem error that hides an entry must refuse, exactly as an unreadable FILE
                // does below -- skipping it would let an incomplete walk return as a complete
                // search, dropping a file or a whole subtree from the answer in silence.
                let metadata = match std::fs::symlink_metadata(&path) {
                    Ok(metadata) => metadata,
                    Err(_) => return Err(SourceSearchError::Unavailable),
                };
                // WHICH HALF CARRIES THE WEIGHT, measured rather than assumed: removing this
                // `is_symlink` arm leaves the cell GREEN, because `symlink_metadata` does not
                // follow — a junction's metadata already reports NOT-a-directory, so the entry is
                // never pushed onto the stack. The load-bearing change is the switch from
                // `path.is_dir()` (which follows) to `symlink_metadata`; sabotaging THAT is what
                // turns the cell red.
                //
                // Kept anyway, and the redundancy is declared rather than trimmed: this arm is
                // the one that states the INTENT (a link is not evidence), and it is the half
                // that keeps holding if a future refactor reaches for `metadata()` again on a
                // platform where a reparse form reports differently.
                if metadata.file_type().is_symlink() {
                    continue;
                }
                if metadata.is_dir() {
                    // Credential and generated directories are skipped by NAME at any depth
                    // (#1065). The entry was already counted above, so the traversal bound still
                    // sees the directory it refused to enter.
                    if skipped_directory_name(&path) {
                        continue;
                    }
                    stack.push(path);
                    continue;
                }
                // REGULAR files only. A FIFO named `blocked.rs` is not a directory, reports zero
                // bytes, and `read_to_string` BLOCKS on it forever — an untrusted workspace could
                // stop the walk dead, which is the denial of service the bounds exist to prevent
                // and cannot themselves see (G's #622 finding).
                if !metadata.is_file() {
                    continue;
                }
                if !TEXT_SUFFIXES
                    .iter()
                    .any(|suffix| relative.to_lowercase().ends_with(suffix))
                {
                    continue;
                }

                // The bounds are checked BEFORE the read they would bound, so the ceiling is
                // never crossed and then reported.
                scanned_files += 1;
                if scanned_files > bounds.max_files_scanned {
                    return Err(SourceSearchError::BoundExceeded);
                }
                // The declared size is a PRICE QUOTE, and the read is what is actually paid. A
                // file that grows between the two — or a metadata call that lies — meant the
                // ceiling bounded a number nobody spent (G's #622 TOCTOU finding). Quoted here,
                // then RECONCILED against the bytes that arrived, so the bound is over what was
                // read rather than over what was promised.
                // The quote reuses the NO-FOLLOW `metadata` from the walk, not `entry.metadata()`
                // (Codex #622): `DirEntry::metadata()` FOLLOWS a link, so a candidate swapped to a
                // symlink between the walk's `symlink_metadata` and here would trigger a
                // link-following stat outside the workspace — potentially blocking on a slow or
                // unavailable mount — all before the no-follow `open_candidate` guard is reached.
                // `metadata` was taken with `symlink_metadata` and belongs to a file already
                // confirmed regular, so its `len()` is the faithful quote and no new stat is made.
                let quoted = metadata.len();
                if scanned_bytes.saturating_add(quoted) > bounds.max_bytes_scanned {
                    return Err(SourceSearchError::BoundExceeded);
                }

                // The READ ITSELF is bounded, not just reconciled after: `read_to_string` on a
                // file that grew past its quote would allocate the whole enlarged file before any
                // check could refuse it (G's TOCTOU, second half). `take` caps what can ever be
                // pulled — one byte past the remaining budget proves the breach without paying
                // for the rest.
                let remaining = bounds.max_bytes_scanned - scanned_bytes;
                // Opened the way the platform can promise the most about (G's #622 P1): on
                // Unix, `O_NOFOLLOW` refuses a symlink swapped in after the walk's check —
                // closing that TOCTOU outright there — and `O_NONBLOCK` keeps a FIFO swapped
                // in after the regular-file check from parking `open` forever waiting for a
                // writer, a block no declared ceiling can interrupt. Neither flag changes how
                // a regular file reads. Windows has no such open; its residual is declared at
                // the re-check below.
                if self.cancelled() {
                    return Err(SourceSearchError::Unavailable);
                }
                let Ok(file) = open_candidate(&path) else {
                    return Err(SourceSearchError::Unavailable);
                };
                // The HANDLE's metadata, not the path's: `fstat` on what was actually opened is
                // the one statement no concurrent rename can contradict. A candidate confirmed
                // regular at the walk but non-regular HERE was swapped mid-race, and skipping it
                // would drop a file the search should have covered — mutation masquerading as
                // absence (Codex #622). It refuses `Unavailable`, like an unreadable candidate,
                // rather than `continue`: an entry that was never regular is filtered at the walk,
                // but one that CHANGED under us is an incomplete search, not an honest skip.
                match file.metadata() {
                    Ok(now) if now.is_file() => {}
                    _ => return Err(SourceSearchError::Unavailable),
                }
                // Read as BYTES, bounded by `take`; UTF-8 is judged AFTER the arrival is
                // charged. `read_to_string` consumed up to the whole remaining budget before
                // reporting InvalidData while the ledger was charged only the stale quote, so
                // a changing file could pull past `max_bytes_scanned` with no ceiling noticing
                // (G's #622 finding). The count that reconciles is the count that ARRIVED,
                // whatever the bytes turn out to be.
                let mut bytes = Vec::new();
                let read_outcome = {
                    use std::io::Read as _;
                    // `&file`, not `file`: the handle is needed AGAIN below to confirm the path
                    // still names it (`still_names_opened_file`), and `Read::take` consumes its
                    // receiver. `&File` is itself `Read`, so the read borrows rather than moves.
                    //
                    // Capped at EXACTLY `remaining`, not `remaining + 1` (Codex #622): the port
                    // defines `max_bytes_scanned` as the maximum the implementor may READ, and the
                    // earlier one-byte sentinel read that byte BEFORE refusing — a refusal after
                    // the extra I/O does not un-read it. The read now never crosses the ceiling; a
                    // buffer filled exactly to `remaining` is judged over below, without a sentinel.
                    (&file).take(remaining).read_to_end(&mut bytes)
                };
                // RE-CHECKED after the open (G's TOCTOU pair). On Unix this is now redundant —
                // `O_NOFOLLOW` already refused at the open — and kept as the declared-redundant
                // half. On Windows, where std offers no open-that-refuses-to-follow, this
                // path-based re-check only shrinks the window to two adjacent syscalls; that
                // residual is DECLARED, not closed, and the same residual exists on both
                // platforms for a directory swapped between its check and `read_dir`.
                // RE-CHECKED after the open that the path STILL names the file we read (G's #622
                // TOCTOU family): a symlink OR a regular file swapped in between the open and now
                // would attribute the handle's bytes to a path that no longer holds them. On Unix
                // the `(dev, ino)` comparison closes both swaps deterministically; on Windows,
                // where std has neither an open-that-refuses-to-follow nor a stable file identity,
                // the fallback catches the symlink swap and DECLARES the regular-file swap as a
                // residual (`still_names_opened_file`). The same residual exists on both platforms
                // for a directory swapped between its check and `read_dir`.
                if !still_names_opened_file(&file, &path) {
                    return Err(SourceSearchError::Unavailable);
                }
                if read_outcome.is_err() {
                    // A candidate that cannot be READ is not a candidate that is ABSENT.
                    // Skipping made permission-denied indistinguishable from not-matching, and
                    // an incomplete search would return as a successful one (G's finding) —
                    // the exact absence-fabrication the rest of this stack refuses.
                    return Err(SourceSearchError::Unavailable);
                }
                // The read was capped at `remaining`, so a buffer filled to exactly that is a file
                // of AT LEAST `remaining` bytes — admitting it would spend the last of the budget
                // on a file that may be larger, and the ceiling is "bytes READ", already paid. It
                // refuses conservatively without ever having read past the limit (Codex #622); an
                // exact-fit file is the rare cost of that guarantee. A shorter read fit with room.
                if bytes.len() as u64 >= remaining {
                    return Err(SourceSearchError::BoundExceeded);
                }
                scanned_bytes = scanned_bytes.saturating_add(bytes.len() as u64);
                // Not UTF-8: a file this channel does not serve — skipped, AFTER its bytes
                // were charged at their true count.
                let Ok(content) = String::from_utf8(bytes) else {
                    continue;
                };
                let haystack = content.to_lowercase();
                let distinct = needles
                    .iter()
                    .filter(|needle| haystack.contains(needle.as_str()))
                    .count();
                if distinct > 0 {
                    scored.push((distinct, relative));
                }
            }
        }

        // Strongest first; ties broken by path so the answer is deterministic.
        scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
        // `max_results` caps a search that FINISHED. Unlike the scan bounds it is not a breached
        // ceiling — refusing here would make an ordinary successful answer look like one.
        scored.truncate(bounds.max_results as usize);
        Ok(scored.into_iter().map(|(_, path)| path).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::WorkspaceSourceChannel;

    /// Codex #622/#707: the stored root is the CANONICAL location when the supplied path reaches
    /// the workspace through a symlinked ANCESTOR — `symlink_metadata` spares only the final
    /// component, so without `open`'s canonicalize the link-relative spelling would be stored and
    /// later served under it. The hit strings cannot witness this (they strip either prefix to the
    /// same value); the stored root can, and this test reads it DIRECTLY — the field is private and
    /// stays private, so no accessor is added to the production API for a test's sake.
    #[test]
    fn a_symlinked_ancestor_root_is_stored_canonical_not_under_the_link() {
        let target = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(target.path().join("inner")).unwrap();
        let holder = tempfile::tempdir().unwrap();
        let link = holder.path().join("link");

        #[cfg(windows)]
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(target.path())
            .output()
            .is_ok_and(|output| output.status.success());
        #[cfg(not(windows))]
        let made = std::os::unix::fs::symlink(target.path(), &link).is_ok();
        assert!(
            made,
            "arrangement: could not create the ancestor link under test"
        );

        let link_path = link.join("inner");
        let channel = WorkspaceSourceChannel::open(&link_path).unwrap();
        let canonical = std::fs::canonicalize(&link_path).unwrap();

        assert_eq!(
            channel.root, canonical,
            "a root reached through a linked ancestor must be stored canonical"
        );
        assert_ne!(
            channel.root, link_path,
            "the stored root is the real location, not the link-relative path"
        );
    }
}
