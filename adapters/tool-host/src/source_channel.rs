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

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use graphhelm_runtime::ports::{
    BoundedSourceSearch, SourceIndexBounds, SourceSearchBounds, SourceSearchError,
};

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
pub struct WorkspaceSourceChannel {
    root: PathBuf,
    /// Checked between directory entries and before every open (#1086): a scan whose drive gave
    /// it up refuses `Unavailable` at the next check instead of finishing the walk.
    cancel: Option<graphhelm_runtime::ports::ScanCancel>,
    /// The token index (#2319), built on the first search and shared by every handle of this
    /// channel (a cancellable copy searches the same index).
    index: Arc<Mutex<Option<SourceIndex>>>,
    /// The ceiling for building that index from scratch.
    index_bounds: SourceIndexBounds,
}

impl std::fmt::Debug for WorkspaceSourceChannel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WorkspaceSourceChannel")
            .field("root", &self.root)
            .field("cancel", &self.cancel)
            .field("index_bounds", &self.index_bounds)
            .finish_non_exhaustive()
    }
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
            index: Arc::new(Mutex::new(None)),
            index_bounds: graphhelm_runtime::context::INDEX_BUILD_BOUNDS,
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

/// BM25 saturation constant: how quickly repeated occurrences of one term stop adding score.
const BM25_K1: f64 = 1.2;
/// BM25 length normalisation: 0 ignores file length, 1 divides fully by it relative to the mean.
const BM25_B: f64 = 0.75;

/// Okapi BM25 over the files that matched at least one needle (#1705). The previous score counted
/// distinct terms matched, so a long broad document that mentions every word once outranked the
/// short file that is about the question. BM25 weights each term by how rare it is across the
/// scanned files (idf), saturates repeats, and normalises by file length against the mean.
/// `documents` and `document_bytes` cover every scanned UTF-8 file, matched or not.
fn rank_bm25(
    matched: &[(Vec<u32>, u64, String)],
    documents: u64,
    document_bytes: u64,
) -> Vec<(f64, String)> {
    let Some(width) = matched.first().map(|(counts, _, _)| counts.len()) else {
        return Vec::new();
    };
    let total = documents.max(1) as f64;
    let mean_length = (document_bytes as f64 / total).max(1.0);
    let idf: Vec<f64> = (0..width)
        .map(|term| {
            let holding = matched
                .iter()
                .filter(|(counts, _, _)| counts[term] > 0)
                .count() as f64;
            (1.0 + (total - holding + 0.5) / (holding + 0.5)).ln()
        })
        .collect();
    matched
        .iter()
        .map(|(counts, length, path)| {
            let norm = BM25_K1 * (1.0 - BM25_B + BM25_B * (*length as f64) / mean_length);
            let score = counts
                .iter()
                .zip(&idf)
                .map(|(&count, weight)| {
                    let tf = f64::from(count);
                    weight * tf * (BM25_K1 + 1.0) / (tf + norm)
                })
                .sum();
            (score, path.clone())
        })
        .collect()
}

impl WorkspaceSourceChannel {
    /// The search as it always ran: walk and read every text file (#219, #1705). Kept for the
    /// queries the token index cannot answer exactly.
    fn live_search(
        &self,
        needles: &std::collections::BTreeSet<String>,
        bounds: &SourceSearchBounds,
    ) -> Result<Vec<String>, SourceSearchError> {
        let mut scanned_files = 0usize;
        let mut scanned_bytes = 0u64;
        // Per matching file: (occurrences of each needle, in `needles` order; byte length; path).
        // The score needs corpus statistics (how many files hold a term, the mean length), so
        // files are counted during the walk and ranked after it (#1705).
        let mut matched: Vec<(Vec<u32>, u64, String)> = Vec::new();
        let mut documents = 0u64;
        let mut document_bytes = 0u64;
        self.walk_text_files(bounds.max_entries_visited, |relative, path, metadata| {
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
            let (read, haystack) =
                self.read_lowered(&path, bounds.max_bytes_scanned - scanned_bytes)?;
            scanned_bytes = scanned_bytes.saturating_add(read);
            let Some(haystack) = haystack else {
                return Ok(());
            };
            documents += 1;
            document_bytes = document_bytes.saturating_add(haystack.len() as u64);
            let counts: Vec<u32> = needles
                .iter()
                .map(|needle| {
                    u32::try_from(haystack.matches(needle.as_str()).count()).unwrap_or(u32::MAX)
                })
                .collect();
            if counts.iter().any(|&count| count > 0) {
                matched.push((counts, haystack.len() as u64, relative));
            }
            Ok(())
        })?;
        Ok(finish_ranking(&matched, documents, document_bytes, bounds))
    }

    /// The walk every path shares (#2319): the same exclusions, link refusals, regular-file and
    /// suffix filters the live search always applied, with `visit` called for each text file
    /// that survives them. Entries are counted against `max_entries` BEFORE any filter.
    fn walk_text_files(
        &self,
        max_entries: usize,
        mut visit: impl FnMut(String, PathBuf, std::fs::Metadata) -> Result<(), SourceSearchError>,
    ) -> Result<(), SourceSearchError> {
        let mut visited_entries = 0usize;
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
                if visited_entries > max_entries {
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
                visit(relative, path, metadata)?;
            }
        }
        Ok(())
    }

    /// One candidate read with every promise the live search made (#2319 shares it): the open
    /// that refuses what it can, the identity re-check, a read capped at `remaining` and refused
    /// when it fills the cap. Returns the bytes that ARRIVED and the lowercased text, `None` when
    /// the file is not UTF-8 (charged, then skipped, as before).
    fn read_lowered(
        &self,
        path: &Path,
        remaining: u64,
    ) -> Result<(u64, Option<String>), SourceSearchError> {
        // The READ ITSELF is bounded, not just reconciled after: `read_to_string` on a
        // file that grew past its quote would allocate the whole enlarged file before any
        // check could refuse it (G's TOCTOU, second half). `take` caps what can ever be
        // pulled — one byte past the remaining budget proves the breach without paying
        // for the rest.
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
        // Not UTF-8: a file this channel does not serve — skipped, AFTER its bytes
        // were charged at their true count.
        let read = bytes.len() as u64;
        Ok((read, String::from_utf8(bytes).ok().map(|content| content.to_lowercase())))
    }
}

/// Rank and cut, the same for every path: BM25, strongest first, ties broken by path so the
/// answer is deterministic, then `max_results`, which caps a search that FINISHED. Unlike the
/// scan bounds it is not a breached ceiling, so it truncates instead of refusing.
fn finish_ranking(
    matched: &[(Vec<u32>, u64, String)],
    documents: u64,
    document_bytes: u64,
    bounds: &SourceSearchBounds,
) -> Vec<String> {
    let mut scored = rank_bm25(matched, documents, document_bytes);
    scored.sort_by(|left, right| {
        right
            .0
            .total_cmp(&left.0)
            .then_with(|| left.1.cmp(&right.1))
    });
    scored.truncate(bounds.max_results as usize);
    scored.into_iter().map(|(_, path)| path).collect()
}

/// Where a workspace that ALREADY has a `.graphhelm/` directory keeps its source index (#2319).
/// The walk never enters `.graphhelm/`, so the cache never indexes itself. A tree without that
/// directory (a fixture, a temporary copy) keeps the index in memory only: the channel never
/// creates `.graphhelm/` and never leaves files in a tree that did not ask for them.
const INDEX_CACHE: &str = ".graphhelm/cache/source-index-v1.json";
const INDEX_SCHEMA: &str = "graphhelm.source-index/1";

/// One indexed text file: the (length, mtime) it was read at, and what the live search would
/// have computed from it. `doc_len` is the lowercased text's byte length, `None` for a file that
/// is not UTF-8 (counted as read, never as a document, exactly as the live search does).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
struct IndexedFile {
    len: u64,
    modified: u128,
    /// Unix `ctime` in nanoseconds (0 elsewhere). Changed by every write and not settable by
    /// tools, so an edit that keeps the length and restores the mtime is still seen on Unix.
    /// Windows std exposes no change time: there such an edit stays unseen until the file
    /// changes again or the index is rebuilt (a declared residual, #2319).
    #[serde(default)]
    changed: u128,
    doc_len: Option<u64>,
    /// Each maximal run of non-whitespace characters of the lowercased text, with its count.
    tokens: Vec<(String, u32)>,
}

/// The token index (#2319). A query term holds no whitespace (terms that do keep the live scan),
/// so each of its occurrences lies inside exactly one whitespace-free token, and its count in a
/// file is the sum over that file's tokens of its non-overlapping count in each token: the same
/// number `haystack.matches(term)` gives over the whole text. So the index ranks exactly as the
/// live scan did, without opening every file per search.
#[derive(Default)]
struct SourceIndex {
    files: BTreeMap<String, IndexedFile>,
    /// token -> (file id -> count). Ids are stable for the life of the index.
    postings: HashMap<String, HashMap<u32, u32>>,
    ids: HashMap<String, u32>,
    paths: Vec<String>,
    /// Whether the in-memory state differs from what was last persisted.
    dirty: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PersistedIndex {
    schema: String,
    root: String,
    files: BTreeMap<String, IndexedFile>,
}

impl SourceIndex {
    fn id(&mut self, path: &str) -> u32 {
        if let Some(id) = self.ids.get(path) {
            return *id;
        }
        let id = u32::try_from(self.paths.len()).unwrap_or(u32::MAX);
        self.paths.push(path.to_owned());
        self.ids.insert(path.to_owned(), id);
        id
    }

    fn remove(&mut self, path: &str) {
        let Some(old) = self.files.remove(path) else {
            return;
        };
        if let Some(id) = self.ids.get(path).copied() {
            for (token, _) in &old.tokens {
                if let Some(files) = self.postings.get_mut(token) {
                    files.remove(&id);
                    if files.is_empty() {
                        self.postings.remove(token);
                    }
                }
            }
        }
        self.dirty = true;
    }

    fn insert(&mut self, path: String, file: IndexedFile) {
        self.remove(&path);
        let id = self.id(&path);
        for (token, count) in &file.tokens {
            self.postings
                .entry(token.clone())
                .or_default()
                .insert(id, *count);
        }
        self.files.insert(path, file);
        self.dirty = true;
    }

    fn from_files(files: BTreeMap<String, IndexedFile>) -> Self {
        let mut index = Self::default();
        for (path, file) in files {
            index.insert(path, file);
        }
        index.dirty = false;
        index
    }

    /// The live search's `(matched, documents, document_bytes)`, read from the index.
    fn matches(
        &self,
        needles: &std::collections::BTreeSet<String>,
    ) -> (Vec<(Vec<u32>, u64, String)>, u64, u64) {
        let width = needles.len();
        let mut counts: HashMap<u32, Vec<u32>> = HashMap::new();
        for (term, needle) in needles.iter().enumerate() {
            for (token, files) in &self.postings {
                if !token.contains(needle.as_str()) {
                    continue;
                }
                let per =
                    u32::try_from(token.matches(needle.as_str()).count()).unwrap_or(u32::MAX);
                for (id, count) in files {
                    let row = counts.entry(*id).or_insert_with(|| vec![0; width]);
                    row[term] = row[term].saturating_add(per.saturating_mul(*count));
                }
            }
        }
        let matched = counts
            .into_iter()
            .filter_map(|(id, row)| {
                let path = self.paths.get(id as usize)?;
                let length = self.files.get(path)?.doc_len?;
                Some((row, length, path.clone()))
            })
            .collect();
        let documents = self
            .files
            .values()
            .filter(|file| file.doc_len.is_some())
            .count() as u64;
        let document_bytes = self.files.values().filter_map(|file| file.doc_len).sum();
        (matched, documents, document_bytes)
    }
}

/// Whitespace-free tokens of `text` with their counts, sorted so the persisted form is stable.
fn tokenize(text: &str) -> Vec<(String, u32)> {
    let mut counts: HashMap<&str, u32> = HashMap::new();
    for token in text
        .split(char::is_whitespace)
        .filter(|token| !token.is_empty())
    {
        *counts.entry(token).or_default() += 1;
    }
    let mut tokens: Vec<(String, u32)> = counts
        .into_iter()
        .map(|(token, count)| (token.to_owned(), count))
        .collect();
    tokens.sort();
    tokens
}

#[cfg(unix)]
fn changed_nanos(metadata: &std::fs::Metadata) -> u128 {
    use std::os::unix::fs::MetadataExt as _;
    let seconds = u128::try_from(metadata.ctime()).unwrap_or(0);
    let nanos = u128::try_from(metadata.ctime_nsec()).unwrap_or(0);
    seconds * 1_000_000_000 + nanos
}

#[cfg(not(unix))]
fn changed_nanos(_metadata: &std::fs::Metadata) -> u128 {
    0
}

fn modified_nanos(metadata: &std::fs::Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_nanos())
}

impl WorkspaceSourceChannel {
    /// The same channel with another build ceiling (#2319), for callers that size it themselves.
    #[must_use]
    pub fn with_index_bounds(mut self, bounds: SourceIndexBounds) -> Self {
        self.index_bounds = bounds;
        self
    }

    /// Answer `needles` from the index: build it on the first search (under the build ceiling),
    /// otherwise refresh what changed (charged to this search's bounds) and read the postings.
    fn indexed_search(
        &self,
        needles: &std::collections::BTreeSet<String>,
        bounds: &SourceSearchBounds,
    ) -> Result<Vec<String>, SourceSearchError> {
        let mut guard = self
            .index
            .lock()
            .map_err(|_| SourceSearchError::Unavailable)?;
        if guard.is_none() {
            *guard = self.load_persisted().map(SourceIndex::from_files);
        }
        match guard.as_mut() {
            Some(index) => {
                if !self.refresh(index, bounds)? {
                    *index = self.build(bounds)?;
                }
            }
            None => *guard = Some(self.build(bounds)?),
        }
        let index = guard.as_mut().ok_or(SourceSearchError::Unavailable)?;
        if index.dirty {
            self.persist(index);
            index.dirty = false;
        }
        let (matched, documents, document_bytes) = index.matches(needles);
        Ok(finish_ranking(&matched, documents, document_bytes, bounds))
    }

    /// Index the whole workspace once, under [`SourceIndexBounds`]. Past a ceiling it refuses and
    /// returns nothing: the caller keeps whatever it had, never a partial index. The walk is also
    /// held to the searching caller's traversal bound: a build reads more, it never walks more.
    fn build(&self, search: &SourceSearchBounds) -> Result<SourceIndex, SourceSearchError> {
        let bounds = SourceIndexBounds {
            max_entries_visited: self
                .index_bounds
                .max_entries_visited
                .min(search.max_entries_visited),
            ..self.index_bounds
        };
        let mut files = 0usize;
        let mut bytes = 0u64;
        let mut indexed = BTreeMap::new();
        self.walk_text_files(bounds.max_entries_visited, |relative, path, metadata| {
            files += 1;
            if files > bounds.max_files_indexed {
                return Err(SourceSearchError::BoundExceeded);
            }
            if bytes.saturating_add(metadata.len()) > bounds.max_bytes_indexed {
                return Err(SourceSearchError::BoundExceeded);
            }
            let (file, read) =
                self.index_file(&path, &metadata, bounds.max_bytes_indexed - bytes)?;
            bytes = bytes.saturating_add(read);
            indexed.insert(relative, file);
            Ok(())
        })?;
        let mut index = SourceIndex::from_files(indexed);
        index.dirty = true;
        Ok(index)
    }

    /// Bring `index` up to the workspace: walk (entries charged to `bounds`), re-read each file
    /// whose (length, mtime) changed (files and bytes charged to `bounds`), drop files that are
    /// gone. Returns `false` when more files changed than one search may read: the caller then
    /// rebuilds under the build ceiling instead. Nothing is applied unless the walk completes.
    fn refresh(
        &self,
        index: &mut SourceIndex,
        bounds: &SourceSearchBounds,
    ) -> Result<bool, SourceSearchError> {
        let mut seen = std::collections::BTreeSet::new();
        let mut fresh: Vec<(String, IndexedFile)> = Vec::new();
        let mut files = 0usize;
        let mut bytes = 0u64;
        let mut too_many = false;
        self.walk_text_files(bounds.max_entries_visited, |relative, path, metadata| {
            let modified = modified_nanos(&metadata);
            let changed = changed_nanos(&metadata);
            let unchanged = index.files.get(&relative).is_some_and(|file| {
                file.len == metadata.len() && file.modified == modified && file.changed == changed
            });
            seen.insert(relative.clone());
            if unchanged || too_many {
                return Ok(());
            }
            files += 1;
            if files > bounds.max_files_scanned
                || bytes.saturating_add(metadata.len()) > bounds.max_bytes_scanned
            {
                too_many = true;
                return Ok(());
            }
            let (file, read) =
                self.index_file(&path, &metadata, bounds.max_bytes_scanned - bytes)?;
            bytes = bytes.saturating_add(read);
            fresh.push((relative, file));
            Ok(())
        })?;
        if too_many {
            return Ok(false);
        }
        let gone: Vec<String> = index
            .files
            .keys()
            .filter(|path| !seen.contains(*path))
            .cloned()
            .collect();
        for path in gone {
            index.remove(&path);
        }
        for (path, file) in fresh {
            index.insert(path, file);
        }
        Ok(true)
    }

    /// Read one file the careful way and turn it into its index entry and the bytes it cost.
    fn index_file(
        &self,
        path: &Path,
        metadata: &std::fs::Metadata,
        remaining: u64,
    ) -> Result<(IndexedFile, u64), SourceSearchError> {
        let (read, haystack) = self.read_lowered(path, remaining)?;
        let file = IndexedFile {
            len: metadata.len(),
            modified: modified_nanos(metadata),
            changed: changed_nanos(metadata),
            doc_len: haystack.as_ref().map(|text| text.len() as u64),
            tokens: haystack.as_deref().map(tokenize).unwrap_or_default(),
        };
        Ok((file, read))
    }

    fn cache_path(&self) -> Option<PathBuf> {
        let state = self.root.join(".graphhelm");
        let metadata = std::fs::symlink_metadata(&state).ok()?;
        (metadata.is_dir() && !metadata.file_type().is_symlink())
            .then(|| self.root.join(INDEX_CACHE))
    }

    /// The persisted index, when this workspace keeps one and it was written for this root and
    /// schema. Anything else is ignored: the next refresh drops entries the walk does not see, so
    /// a stale or foreign cache can only cost a rebuild, never add a path.
    fn load_persisted(&self) -> Option<BTreeMap<String, IndexedFile>> {
        let bytes = std::fs::read(self.cache_path()?).ok()?;
        let persisted: PersistedIndex = serde_json::from_slice(&bytes).ok()?;
        (persisted.schema == INDEX_SCHEMA && persisted.root == self.root.to_string_lossy())
            .then_some(persisted.files)
    }

    /// Write the index atomically (temporary file, then rename). A workspace that cannot be
    /// written keeps the in-memory index; persistence is a cache, never a precondition.
    fn persist(&self, index: &SourceIndex) {
        let Some(path) = self.cache_path() else {
            return;
        };
        let persisted = PersistedIndex {
            schema: INDEX_SCHEMA.to_owned(),
            root: self.root.to_string_lossy().into_owned(),
            files: index.files.clone(),
        };
        let Ok(bytes) = serde_json::to_vec(&persisted) else {
            return;
        };
        let Some(parent) = path.parent() else {
            return;
        };
        // A name of its own per writer: two sessions persisting at once each write a whole file
        // and rename it into place, so a reader only ever sees one complete index or the other.
        static WRITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let temporary = path.with_extension(format!(
            "json.{}.{}.tmp",
            std::process::id(),
            WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        if std::fs::create_dir_all(parent).is_ok() && std::fs::write(&temporary, bytes).is_ok()
            && std::fs::rename(&temporary, &path).is_err()
        {
            let _ = std::fs::remove_file(&temporary);
        }
    }
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

        // #2319: a term with whitespace cannot be answered from the token index (its matches can
        // cross a token boundary), so such a query keeps the live scan; every other query is
        // answered from the index, which counts the same occurrences without opening every file.
        if needles.iter().any(|needle| needle.chars().any(char::is_whitespace)) {
            return self.live_search(&needles, bounds);
        }
        let ranked = self.indexed_search(&needles, bounds)?;
        if self.cancelled() {
            return Err(SourceSearchError::Unavailable);
        }
        Ok(ranked)
    }

    fn search_with_provenance_cancellable(
        &self,
        terms: &[String],
        bounds: &SourceSearchBounds,
        cancel: &graphhelm_runtime::ports::ScanCancel,
    ) -> Result<graphhelm_runtime::ports::SourceSearchResult, SourceSearchError> {
        let channel = Self {
            root: self.root.clone(),
            cancel: Some(cancel.clone()),
            index: Arc::clone(&self.index),
            index_bounds: self.index_bounds,
        };
        channel.search_with_provenance(terms, bounds)
    }
}

#[cfg(test)]
mod tests {
    use super::WorkspaceSourceChannel;
    use super::{INDEX_CACHE, SourceIndexBounds};
    use graphhelm_runtime::context::SEARCH_BOUNDS;
    use graphhelm_runtime::ports::{BoundedSourceSearch, SourceSearchError};
    use std::collections::BTreeSet;
    use std::path::Path;

    fn write(root: &Path, relative: &str, bytes: &[u8]) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    fn needles(terms: &[&str]) -> BTreeSet<String> {
        terms.iter().map(|term| term.to_lowercase()).collect()
    }

    fn search(channel: &WorkspaceSourceChannel, terms: &[&str]) -> Vec<String> {
        let terms: Vec<String> = terms.iter().map(|term| (*term).to_owned()).collect();
        channel.search(&terms, &SEARCH_BOUNDS).unwrap()
    }

    /// #2319: the index answers exactly what the live scan answered: same paths, same order.
    /// Substrings inside identifiers, punctuation-joined tokens, repeated terms, case, Unicode,
    /// a term matching across many files and a non-UTF-8 file (read, never a document) are all
    /// in the tree. Defect caught: any token rule that splits a term's occurrence or counts it
    /// differently changes the BM25 order. Cost: local temp files only.
    #[test]
    fn the_index_ranks_exactly_as_the_live_scan() {
        let root = tempfile::tempdir().unwrap();
        write(root.path(), "src/keel.rs", b"fn keel_readme_marker() { keel(); KEEL::run(); }\n");
        write(root.path(), "src/lib.rs", b"pub mod keel; // the keel module\nuse crate::keel;\n");
        write(root.path(), "docs/guide.md", b"# Guide\nThe Keel method keeps keels keel-shaped.\n");
        write(root.path(), "docs/long.md", "keel ".repeat(400).as_bytes());
        write(root.path(), "notes/unicode.txt", "Straße STRASSE ñandú keel\u{a0}keel\n".as_bytes());
        write(root.path(), "data/blob.json", &[0xff, 0xfe, b'k', b'e', b'e', b'l']);
        write(root.path(), "src/other.rs", b"fn unrelated() {}\n");
        let channel = WorkspaceSourceChannel::open(root.path()).unwrap();
        for terms in [
            vec!["keel"],
            vec!["keel", "marker"],
            vec!["readme_marker"],
            vec!["keel::run"],
            vec!["straße", "ñandú"],
            vec!["fn", "keel", "module"],
            vec!["absent"],
        ] {
            let wanted = needles(&terms);
            let live = channel.live_search(&wanted, &SEARCH_BOUNDS).unwrap();
            let indexed = channel.indexed_search(&wanted, &SEARCH_BOUNDS).unwrap();
            assert_eq!(indexed, live, "terms {terms:?}");
        }
    }

    /// #2319: the index follows the workspace. An edited file is found at its new content, an
    /// added file is found, a deleted file is gone, all on the next search of the SAME channel.
    /// The index then persists under the existing `.graphhelm/`, a new channel reuses it, and a
    /// path the walk does not see (here, a forged entry inside `.graphhelm/`) never comes back.
    #[test]
    fn the_index_follows_edits_additions_and_deletions() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join(".graphhelm")).unwrap();
        write(root.path(), "a.rs", b"alpha first\n");
        write(root.path(), "b.rs", b"beta\n");
        let channel = WorkspaceSourceChannel::open(root.path()).unwrap();
        assert_eq!(search(&channel, &["alpha"]), ["a.rs"]);
        assert_eq!(search(&channel, &["gamma"]), Vec::<String>::new());

        // Edit to a different length, add, delete.
        write(root.path(), "a.rs", b"gamma replaced the first word entirely\n");
        write(root.path(), "c.rs", b"delta arrives\n");
        std::fs::remove_file(root.path().join("b.rs")).unwrap();
        assert_eq!(search(&channel, &["alpha"]), Vec::<String>::new());
        assert_eq!(search(&channel, &["gamma"]), ["a.rs"]);
        assert_eq!(search(&channel, &["delta"]), ["c.rs"]);
        assert_eq!(search(&channel, &["beta"]), Vec::<String>::new());

        let cache = root.path().join(INDEX_CACHE);
        assert!(cache.is_file(), "the index persists under the existing .graphhelm/");
        let mut persisted: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&cache).unwrap()).unwrap();
        persisted["files"][".graphhelm/forged.rs"] =
            persisted["files"]["c.rs"].clone();
        std::fs::write(&cache, serde_json::to_vec(&persisted).unwrap()).unwrap();
        let reopened = WorkspaceSourceChannel::open(root.path()).unwrap();
        assert_eq!(search(&reopened, &["delta"]), ["c.rs"]);
    }

    /// A tree with no `.graphhelm/` is never written to: the index stays in memory.
    #[test]
    fn a_tree_without_graphhelm_state_gets_no_cache_file() {
        let root = tempfile::tempdir().unwrap();
        write(root.path(), "a.rs", b"alpha\n");
        let channel = WorkspaceSourceChannel::open(root.path()).unwrap();
        assert_eq!(search(&channel, &["alpha"]), ["a.rs"]);
        assert!(!root.path().join(".graphhelm").exists());
    }

    /// #2319: building the index has its own ceiling. Past it the search refuses with
    /// `BoundExceeded`, nothing is persisted, and no partial index is kept: a later search with
    /// room builds the whole index and finds every file.
    #[test]
    fn a_build_past_its_ceiling_refuses_and_keeps_nothing() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join(".graphhelm")).unwrap();
        for name in ["a", "b", "c"] {
            write(root.path(), &format!("{name}.rs"), format!("{name} common\n").as_bytes());
        }
        let tight = SourceIndexBounds {
            max_entries_visited: 1_000,
            max_files_indexed: 2,
            max_bytes_indexed: 1024 * 1024,
        };
        let channel = WorkspaceSourceChannel::open(root.path())
            .unwrap()
            .with_index_bounds(tight);
        let terms = vec!["common".to_owned()];
        assert_eq!(
            channel.search(&terms, &SEARCH_BOUNDS),
            Err(SourceSearchError::BoundExceeded)
        );
        assert!(!root.path().join(INDEX_CACHE).exists(), "nothing persisted");
        assert!(channel.index.lock().unwrap().is_none(), "no partial index kept");
        let roomy = WorkspaceSourceChannel::open(root.path()).unwrap();
        assert_eq!(search(&roomy, &["common"]).len(), 3);
    }

    /// #2319 risk 1: two sessions on one project. One channel keeps building and persisting
    /// (files change under it) while others open the same project and search. Every search
    /// answers, every answer names only files that exist, and the cache on disk always parses:
    /// a reader never sees a half-written index (each writer renames its own whole file).
    #[test]
    fn two_sessions_on_one_project_never_read_a_half_written_index() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join(".graphhelm")).unwrap();
        for index in 0..40 {
            write(root.path(), &format!("f{index}.rs"), format!("shared token{index}\n").as_bytes());
        }
        let path = root.path().to_path_buf();
        let writer = {
            let path = path.clone();
            std::thread::spawn(move || {
                let channel = WorkspaceSourceChannel::open(&path).unwrap();
                for round in 0..15 {
                    write(&path, &format!("f{}.rs", round % 40), format!("shared round{round} longer\n").as_bytes());
                    assert!(!search(&channel, &["shared"]).is_empty());
                }
            })
        };
        let readers: Vec<_> = (0..3)
            .map(|_| {
                let path = path.clone();
                std::thread::spawn(move || {
                    for _ in 0..10 {
                        let channel = WorkspaceSourceChannel::open(&path).unwrap();
                        let found = search(&channel, &["shared"]);
                        assert!(!found.is_empty(), "a concurrent search answered nothing");
                        assert!(found.iter().all(|hit| path.join(hit).is_file()));
                    }
                })
            })
            .collect();
        writer.join().unwrap();
        for reader in readers {
            reader.join().unwrap();
        }
        let cache = std::fs::read(root.path().join(INDEX_CACHE)).unwrap();
        assert!(serde_json::from_slice::<serde_json::Value>(&cache).is_ok());
        let leftovers = std::fs::read_dir(root.path().join(".graphhelm/cache"))
            .unwrap()
            .filter(|entry| {
                entry.as_ref().unwrap().file_name().to_string_lossy().ends_with(".tmp")
            })
            .count();
        assert_eq!(leftovers, 0, "no temporary file left behind");
    }

    /// #2319 risk 2: a corrupt or truncated index file is ignored and rebuilt; the search answers.
    #[test]
    fn a_corrupt_or_truncated_index_is_rebuilt() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join(".graphhelm")).unwrap();
        write(root.path(), "a.rs", b"alpha\n");
        assert_eq!(search(&WorkspaceSourceChannel::open(root.path()).unwrap(), &["alpha"]), ["a.rs"]);
        let cache = root.path().join(INDEX_CACHE);
        let whole = std::fs::read(&cache).unwrap();
        for broken in [whole[..whole.len() / 2].to_vec(), b"not json at all".to_vec(), Vec::new()] {
            std::fs::write(&cache, &broken).unwrap();
            let channel = WorkspaceSourceChannel::open(root.path()).unwrap();
            assert_eq!(search(&channel, &["alpha"]), ["a.rs"]);
            assert!(serde_json::from_slice::<serde_json::Value>(&std::fs::read(&cache).unwrap()).is_ok());
        }
    }

    /// #2319 risk 3: an edit that keeps the length and restores the mtime. On Unix the index keys
    /// on `ctime` too, which every write changes and no tool can set back, so the edit is seen.
    /// (Windows std exposes no change time; the card declares that residual.)
    #[cfg(unix)]
    #[test]
    fn an_edit_with_the_same_length_and_mtime_is_seen_on_unix() {
        let root = tempfile::tempdir().unwrap();
        write(root.path(), "a.rs", b"alpha\n");
        let channel = WorkspaceSourceChannel::open(root.path()).unwrap();
        assert_eq!(search(&channel, &["alpha"]), ["a.rs"]);
        let path = root.path().join("a.rs");
        let before = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&path, b"gamma\n").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(before)
            .unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), before);
        assert_eq!(search(&channel, &["gamma"]), ["a.rs"]);
        assert_eq!(search(&channel, &["alpha"]), Vec::<String>::new());
    }


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
        let made = graphhelm_process::command("cmd")
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
