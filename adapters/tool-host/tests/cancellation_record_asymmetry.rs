//! #868: provisioning's cancellation drops the sweep answer, and the reason it is allowed to is a
//! MEASUREMENT that nothing has ever checked. This file checks it.
//!
//! `run_supervised`'s two `terminate` calls discard `TerminationOutcome` by name, so a cancelled
//! provision that left an escapee is indistinguishable from a clean kill. The comment at the first
//! of those calls says why it was filed rather than fixed:
//!
//! > *"Carrying it here would mean widening `HostError`, and a value nothing reads is not
//! > containment, it is a field to maintain."*
//!
//! That argument is sound and it rests on a premise about the OTHER boundary. #748 widened
//! `CapturedProcess` with `tree_kill` for the tool-call path, and the case for not doing the same
//! here is that the field would have no consumer. **Measured on this tree, `tree_kill` has no
//! production consumer either**: every mention outside its defining file writes `tree_kill: None`
//! at a construction site, and `host.rs`'s disposition ladder — which reads `readers_abandoned`,
//! `reader_lost`, `cancelled` and `timed_out` — does not read it.
//!
//! So the two boundaries are not asymmetric in what they REPORT; they are asymmetric in what they
//! RECORD, and neither is read. That is the state the deferral assumes.
//!
//! **THE DAY A CONSUMER APPEARS, THE DEFERRAL EXPIRES.** Once production code branches on
//! `tree_kill`, one boundary can act on an escapee and the other cannot see one — and that
//! difference is a defect rather than a design. Nothing would have noticed. This cell is the
//! notice.
//!
//! WHY A SOURCE CENSUS IS ACCEPTABLE HERE AND WOULD NOT BE FOR A GUARD. This census reads SOURCE,
//! not types. Destructuring is no longer among its blind spots — `let CapturedProcess { tree_kill,
//! .. }` and every nesting of it is decided from the `syn` AST, so the record is recognised
//! wherever a pattern may appear — but a read through a re-export or a trait object still names
//! nothing this file can see. The exact residual is listed at `reads_the_field`. In a CONTAINMENT
//! guard such a gap admits the defect the guard exists to stop. Here the cost of a missed spelling
//! is a LATE revisit of a deferral, not an admitted defect, and the failure direction is the safe
//! one: a new spelling delays this cell, it does not make the code wrong.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Attribute, Expr, ImplItem, Item, Local, Meta, Stmt, TraitItem};

/// The documented upper bound on one source file the census will read and parse.
///
/// The largest `.rs` file in this workspace is well under a tenth of this; the bound exists so an
/// untrusted checkout cannot make the authoritative gate allocate without limit.
const MAX_SOURCE_BYTES: u64 = 4 * 1024 * 1024;

/// The workspace root, from this crate's manifest directory.
fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(Path::parent)
        .expect("adapters/<crate> sits two levels under the workspace root")
        .to_path_buf()
}

/// The workspace's member crates, read from the root manifest.
///
/// NOT a hand-written list of roots. The first version walked `apps/`, `core/` and `adapters/`,
/// which are three of the FOUR directories that hold members: `tools/development-benchmark`
/// depends on `graphhelm-tool-host` directly (`tools/development-benchmark/Cargo.toml:23`), so a
/// consumer added there is a concrete way the deferral expires where nothing was looking (Codex,
/// third finding on this PR).
///
/// Deriving the population from the manifest is the fix that cannot fall behind again: a new
/// member directory arrives already walked, and the guard stops depending on my memory of the
/// tree's shape.
fn member_roots(root: &Path) -> Vec<PathBuf> {
    let manifest = std::fs::read_to_string(root.join("Cargo.toml"))
        .expect("the workspace manifest must be readable");
    let members = manifest
        .split_once("members = [")
        .and_then(|(_, rest)| rest.split_once(']'))
        .map(|(list, _)| list)
        .expect("the workspace manifest must declare `members = [`");
    members
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim().trim_end_matches(',').trim_matches('"');
            (!trimmed.is_empty() && !trimmed.starts_with('#')).then(|| root.join(trimmed))
        })
        .collect()
}

/// Production `.rs` files in the defining crate and direct workspace consumers.
///
/// A field with this spelling in a crate that cannot depend on `graphhelm-tool-host` is not a
/// receiver of `CapturedProcess`. The old all-member walk included `adapters/process-tree`, which
/// made an unrelated `outcome.tree_kill` a false reader. This is a manifest-derived population,
/// not a claim that textual matching proves all Rust type flow: re-exports and indirect consumers
/// remain outside this bounded observer until a syntax-aware/type-aware census exists.
fn sources_under(member_src: &Path) -> Vec<PathBuf> {
    fn walk(directory: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(directory) else {
            return;
        };
        for entry in entries.flatten() {
            // A repository checkout is untrusted input (AGENTS.md, security rules). `path.is_dir()`
            // FOLLOWS a symlink, so a directory link to an ancestor loops this recursion and one
            // pointing outside the member root makes the authoritative gate read arbitrary host
            // paths. Decide on the link itself, never on its target: `file_type()` comes from
            // `symlink_metadata`, so a link is neither descended nor collected.
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() {
                if path.file_name().is_some_and(|name| name == "tests") {
                    continue;
                }
                walk(&path, out);
            } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
                // Bound the input BEFORE reading or parsing it. `production_source` and
                // `lexical_projection` each allocate buffers the size of the file, so an
                // unreferenced multi-megabyte `.rs` file under `src` would let a checkout stall
                // or exhaust the gate. The bound is documented, not silent: an oversized file
                // refuses the census rather than being skipped into a false "no reader".
                let size = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
                assert!(
                    size <= MAX_SOURCE_BYTES,
                    "OBSERVER_MISSING: {} is {size} bytes, above the {MAX_SOURCE_BYTES}-byte census bound; \
                     the source census refuses it rather than reporting an unmeasured absence",
                    path.display()
                );
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(member_src, &mut out);
    out
}

/// Read one census input, or REFUSE.
///
/// A census answers a question about absence, so a file it could not read must never leave the
/// population quietly: `read_to_string(path).ok()` turns an unreadable consumer into part of a
/// zero the cell then reports as "nothing reads the field". The refusal carries the file's own
/// error, so a permission denial and a non-UTF-8 file are distinguishable at the failure.
fn read_census_source(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|error| {
        panic!(
            "OBSERVER_MISSING: {} could not be read for the source census ({error}); \
             an unreadable file must refuse the census rather than shrink its population",
            path.display()
        )
    })
}

/// Production `.rs` files in the defining crate and direct workspace consumers.
fn production_sources(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for member in member_roots(root) {
        let manifest = member.join("Cargo.toml");
        let manifest_text = std::fs::read_to_string(&manifest)
            .expect("every workspace member must have a readable manifest");
        let is_definition = manifest_text.contains("name = \"graphhelm-tool-host\"");
        let is_direct_consumer = manifest_text.contains("graphhelm-tool-host");
        if !is_definition && !is_direct_consumer {
            continue;
        }
        out.extend(sources_under(&member.join("src")));
    }
    out.sort();
    out
}

/// A READ of the field, decided per OCCURRENCE and per CONTEXT.
///
/// THE FIRST VERSION MEASURED THE WRONG PROPERTY, and two lanes proved it by execution rather than
/// by argument. It asked `contains("tree_kill") && !contains("tree_kill:")` -- "no colon-suffixed
/// spelling anywhere on this line" -- which is not "this line reads the field". It therefore both
/// missed and false-positived, and a predicate that does both is not a strict rule, it is the
/// wrong question:
///
///   `tree_kill: captured.tree_kill`          MISSED  -- the ordinary way a first consumer appears
///   `let _ = 1u8; // ... tree_kill ...`      FIRED   -- a read that does not exist (J)
///   `let CapturedProcess { tree_kill: tk, .. }`  MISSED  -- destructuring with a rename (L)
///
/// Nor was the whole defining module fair to exclude: a method on `CapturedProcess` branching on
/// `self.tree_kill` is one of the likeliest first consumers and lived where nothing looked (Codex).
///
/// SO: a trailing comment is removed first -- a mention there reads nothing -- and then an
/// occurrence is a READ when either
///
///   (a) it is preceded by a DOT: `captured.tree_kill`, `self.tree_kill`, and the right-hand side
///       of `tree_kill: captured.tree_kill`; or
///   (b) the line DESTRUCTURES the record: `record_pattern_ranges` asks `syn` for the `Pat` nodes
///       that name `CapturedProcess`, so pattern position is DECIDED by the parser and is not
///       enumerated here. That covers `{ tree_kill, .. }`, `{ tree_kill: tk, .. }` and
///       `{ tree_kill: Some(t), .. }` wherever a pattern is legal, and the colon means RENAME
///       rather than assignment -- which the first version read backwards.
///
///       The four-spelling text comparison (`let`/`if let`/`while let`, or a match arm's `=>`)
///       is what is left of that first version, and it is now a FALLBACK reached only when the
///       line does not parse even as a function body, where there is no pattern to ask about.
///       Read as the rule it is strictly narrower than the parser, and the gap has two shapes
///       rather than one:
///
///       - POSITION. A `for` pattern and a closure parameter are two places a pattern is legal
///         that none of the four spellings names, and `record_pattern_ranges` sees both.
///       - NESTING, and this is the distinction the first wording of this paragraph blurred. The
///         three `let` spellings are matched with `starts_with`, so they recognise only the DIRECT
///         form, where the record is the whole pattern: `let CapturedProcess { tree_kill, .. }`.
///         A record nested inside another pattern -- `let Some(CapturedProcess { tree_kill, .. })`
///         -- begins `let Some(`, matches none of the three, and carries no `=>`, so the fallback
///         misses it although `contains("CapturedProcess {")` is true. `record_pattern_ranges`
///         walks every `Pat` node and sees it at any depth. So "the line destructures the record"
///         is the PARSER's rule; the fallback approximates it only for an unnested pattern in one
///         of four positions, and it is reached only where there is no parse at all.
///
/// Everything else is a write or a same-named local, which is what the defining module holds --
/// `pub tree_kill:`, `let mut tree_kill = None`, `tree_kill = Some(..)`, the `tree_kill,`
/// shorthand, and `tree_kill: None` constructions. **So no file and no line is excused by name.**
///
/// THE RESIDUAL, NAMED EXACTLY. Pattern position is no longer enumerated: `record_pattern_ranges`
/// asks `syn` for `Pat` nodes, so any nesting of the record inside tuple-struct, reference, tuple,
/// or-, and slice patterns is seen wherever a pattern is legal. What remains unseen is everything
/// that needs more than this file's source view, and it is this list and no longer than it:
///
///   - **re-exports and aliases** -- `pub use CapturedProcess as Captured;` then `Captured { .. }`,
///     because the path's last segment no longer spells the record;
///   - **trait objects and generics** -- a read reached through `dyn Sweep` or a type parameter;
///   - **macro expansion** -- a body `syn` keeps as an unparsed `TokenStream`. `matches!` is
///     carried lexically for exactly this reason; other macros are not;
///   - **type flow** -- `receiver_scopes` associates a dotted receiver with a lexical extent, which
///     is a bounded heuristic and not Rust type inference;
///   - **scoping** -- the population is each member's `src`, so `build.rs`, `examples/` and
///     `benches/` are outside it while a `tests/` directory is deliberately excluded;
///   - **`#[path]` modules** -- a compiled module whose file lives outside `src`, and conversely an
///     orphan or generated `.rs` file under `src` that no module graph references.
///
/// Closing any of these means resolving names or types, which is a different instrument from a
/// source census. The failure direction stays the safe one: a missed spelling delays this cell.
fn reads_the_field(line: &str) -> bool {
    let projected = lexical_projection(line);
    if projected.contains('\n') {
        return !reader_lines(&projected).is_empty();
    }
    reads_the_field_in_line(&projected)
}

fn reads_the_field_in_line(line: &str) -> bool {
    let code = match line.find("//") {
        Some(at) => &line[..at],
        None => line,
    };
    if !code.contains("tree_kill") {
        return false;
    }
    // Ask the AST first, for the same reason the multi-line pass does: the four spellings lane S
    // measured all name the record on the pattern line without being any of the shapes the text
    // comparison below enumerates. The comparison survives only for a fragment that does not
    // parse even as a function body, where there is no pattern to ask about.
    let destructures = match record_pattern_ranges(code) {
        Some(ranges) => !ranges.is_empty(),
        None => {
            code.contains("CapturedProcess {") && {
                let trimmed = code.trim_start();
                trimmed.starts_with("let CapturedProcess {")
                    || trimmed.starts_with("if let CapturedProcess {")
                    || trimmed.starts_with("while let CapturedProcess {")
                    || code.contains("=>")
            }
        }
    };
    if destructures {
        return true;
    }
    let bytes = code.as_bytes();
    code.match_indices("tree_kill")
        .any(|(start, _)| start > 0 && bytes[start - 1] == b'.')
}

/// Blank items that are compiled only for tests while preserving source line numbers.
///
/// The source is parsed as Rust before any bytes are projected. A range is blanked only when the
/// AST proves that an item's `cfg` predicate implies `test`; unknown predicates stay visible,
/// while malformed source refuses with `OBSERVER_MISSING`. The span range is taken from the parsed item
/// and its attributes, so lifetimes, raw strings, comments, and nested braces cannot change the
/// boundary.
fn production_source(source: &str) -> String {
    let file = syn::parse_file(source)
        .unwrap_or_else(|_| panic!("OBSERVER_MISSING: Rust parsing failed for production source"));
    let source_offset = parsed_source_offset(source, &file);
    let mut visitor = TestOnlyItems::default();
    visitor.visit_file(&file);
    let mut projected = source.as_bytes().to_vec();
    for mut range in visitor.ranges {
        range.start += source_offset;
        range.end += source_offset;
        blank_range(&mut projected, range);
    }
    String::from_utf8(projected).expect("source projection preserves UTF-8 bytes")
}

/// Keep only bytes that belong to real Rust identifiers, punctuation, or delimiters.
///
/// `syn` supplies the test-item ranges above, while `proc_macro2` supplies the lexical spans
/// here. The projection preserves byte offsets, line breaks, and syntax delimiters, but masks
/// literals and every gap between tokens (comments and whitespace are gaps). A proc-macro token
/// synthesized for a doc comment is accepted only when its span's original bytes equal the token's
/// spelling; this prevents `/// fake.tree_kill` from becoming a fake field read.
fn lexical_projection(source: &str) -> String {
    let offset = syn::parse_file(source)
        .ok()
        .map_or(0, |file| parsed_source_offset(source, &file));
    let body = &source[offset..];
    let Ok(tokens) = TokenStream::from_str(body) else {
        panic!("OBSERVER_MISSING: Rust tokenization failed for the bounded source census");
    };
    let mut keep = vec![false; source.len()];
    collect_code_spans(&tokens, offset, source, &mut keep);
    let mut projected = source.as_bytes().to_vec();
    for (index, byte) in projected.iter_mut().enumerate() {
        if !keep[index] && *byte != b'\n' && *byte != b'\r' {
            *byte = b' ';
        }
    }
    String::from_utf8(projected).expect("lexical projection preserves UTF-8 bytes")
}

fn collect_code_spans(tokens: &TokenStream, offset: usize, source: &str, keep: &mut [bool]) {
    for token in tokens.clone() {
        match token {
            TokenTree::Group(group) => {
                if group.delimiter() != Delimiter::None {
                    let (open, close) = match group.delimiter() {
                        Delimiter::Parenthesis => ('(', ')'),
                        Delimiter::Brace => ('{', '}'),
                        Delimiter::Bracket => ('[', ']'),
                        Delimiter::None => unreachable!(),
                    };
                    mark_exact_span(
                        group.span_open(),
                        open.to_string().as_str(),
                        offset,
                        source,
                        keep,
                    );
                    mark_exact_span(
                        group.span_close(),
                        close.to_string().as_str(),
                        offset,
                        source,
                        keep,
                    );
                }
                collect_code_spans(&group.stream(), offset, source, keep);
            }
            TokenTree::Ident(ident) => {
                mark_exact_span(
                    ident.span(),
                    ident.to_string().as_str(),
                    offset,
                    source,
                    keep,
                );
            }
            TokenTree::Punct(punct) => {
                mark_exact_span(
                    punct.span(),
                    punct.as_char().to_string().as_str(),
                    offset,
                    source,
                    keep,
                );
            }
            TokenTree::Literal(literal) => {
                // Literals are deliberately not copied. Their span is still validated so a
                // synthetic doc-comment literal cannot expose its text as code.
                let _ = exact_span(literal.span(), literal.to_string().as_str(), offset, source);
            }
        }
    }
}

fn exact_span(
    span: proc_macro2::Span,
    expected: &str,
    offset: usize,
    source: &str,
) -> Option<std::ops::Range<usize>> {
    let range = span.byte_range();
    let start = offset.checked_add(range.start)?;
    let end = offset.checked_add(range.end)?;
    (start <= end && end <= source.len() && source.get(start..end) == Some(expected))
        .then_some(start..end)
}

fn mark_exact_span(
    span: proc_macro2::Span,
    expected: &str,
    offset: usize,
    source: &str,
    keep: &mut [bool],
) {
    if let Some(range) = exact_span(span, expected, offset, source) {
        for byte in &mut keep[range] {
            *byte = true;
        }
    }
}

/// `syn::parse_file` removes a leading BOM and a shebang before assigning byte spans. Reapply the
/// exact number of removed bytes before touching the original source, or a test-only item after a
/// shebang would blank the wrong production bytes. Use `File::shebang`, which is populated by
/// syn's own whitespace/comment-aware parser, so a `#! /*comment*/ [..]` inner attribute is not
/// mistaken for a shebang.
fn parsed_source_offset(source: &str, file: &syn::File) -> usize {
    let mut offset = if source.starts_with('\u{feff}') { 3 } else { 0 };
    if let Some(shebang) = &file.shebang {
        offset += shebang.len();
    }
    offset
}

#[derive(Default)]
struct TestOnlyItems {
    ranges: Vec<std::ops::Range<usize>>,
}

impl<'ast> Visit<'ast> for TestOnlyItems {
    fn visit_item(&mut self, item: &'ast Item) {
        if self.record_if_test_only(item_attrs(item), item) {
            return;
        }
        visit::visit_item(self, item);
    }

    fn visit_impl_item(&mut self, item: &'ast ImplItem) {
        if self.record_if_test_only(impl_item_attrs(item), item) {
            return;
        }
        visit::visit_impl_item(self, item);
    }

    fn visit_trait_item(&mut self, item: &'ast TraitItem) {
        if self.record_if_test_only(trait_item_attrs(item), item) {
            return;
        }
        visit::visit_trait_item(self, item);
    }

    fn visit_local(&mut self, local: &'ast Local) {
        if self.record_if_test_only(&local.attrs, local) {
            return;
        }
        visit::visit_local(self, local);
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        if self.record_if_test_only(expr_attrs(expr), expr) {
            return;
        }
        visit::visit_expr(self, expr);
    }

    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        if let Stmt::Macro(mac) = stmt
            && self.record_if_test_only(&mac.attrs, mac)
        {
            return;
        }
        visit::visit_stmt(self, stmt);
    }

    // A match ARM carries its own attributes and rustc removes an attributed arm from a normal
    // build like any other item. `syn` walks an arm's attributes without the arm itself being an
    // `Item`, `Local`, `Expr` or statement macro, so a `#[cfg(test)]` arm whose body read
    // `captured.tree_kill` survived the projection and reported a production consumer that does
    // not exist. The arm's own span covers its pattern, guard and body.
    fn visit_arm(&mut self, arm: &'ast syn::Arm) {
        if self.record_if_test_only(&arm.attrs, arm) {
            return;
        }
        visit::visit_arm(self, arm);
    }

    // The same hole for the remaining attributed child nodes that can CONTAIN an expression:
    // struct-literal fields (`#[cfg(test)] tree_kill: captured.tree_kill`), field definitions,
    // and function parameters. Each is attributed, none is reached by the four visitors above.
    fn visit_field_value(&mut self, field: &'ast syn::FieldValue) {
        if self.record_if_test_only(&field.attrs, field) {
            return;
        }
        visit::visit_field_value(self, field);
    }

    fn visit_field(&mut self, field: &'ast syn::Field) {
        if self.record_if_test_only(&field.attrs, field) {
            return;
        }
        visit::visit_field(self, field);
    }

    fn visit_fn_arg(&mut self, arg: &'ast syn::FnArg) {
        let attrs: &[Attribute] = match arg {
            syn::FnArg::Receiver(receiver) => &receiver.attrs,
            syn::FnArg::Typed(typed) => &typed.attrs,
        };
        if self.record_if_test_only(attrs, arg) {
            return;
        }
        visit::visit_fn_arg(self, arg);
    }
}

impl TestOnlyItems {
    fn record_if_test_only<T: Spanned>(&mut self, attrs: &[Attribute], node: &T) -> bool {
        if !attrs.iter().any(attribute_requires_test) {
            return false;
        }
        let mut range = node.span().byte_range();
        for attr in attrs {
            let attr_range = attr.span().byte_range();
            range.start = range.start.min(attr_range.start);
            range.end = range.end.max(attr_range.end);
        }
        self.ranges.push(range);
        true
    }
}

fn item_attrs(item: &Item) -> &[Attribute] {
    match item {
        Item::Const(item) => &item.attrs,
        Item::Enum(item) => &item.attrs,
        Item::ExternCrate(item) => &item.attrs,
        Item::Fn(item) => &item.attrs,
        Item::ForeignMod(item) => &item.attrs,
        Item::Impl(item) => &item.attrs,
        Item::Macro(item) => &item.attrs,
        Item::Mod(item) => &item.attrs,
        Item::Static(item) => &item.attrs,
        Item::Struct(item) => &item.attrs,
        Item::Trait(item) => &item.attrs,
        Item::TraitAlias(item) => &item.attrs,
        Item::Type(item) => &item.attrs,
        Item::Union(item) => &item.attrs,
        Item::Use(item) => &item.attrs,
        Item::Verbatim(_) => &[],
        _ => &[],
    }
}

fn impl_item_attrs(item: &ImplItem) -> &[Attribute] {
    match item {
        ImplItem::Const(item) => &item.attrs,
        ImplItem::Fn(item) => &item.attrs,
        ImplItem::Type(item) => &item.attrs,
        ImplItem::Macro(item) => &item.attrs,
        ImplItem::Verbatim(_) => &[],
        _ => &[],
    }
}

fn trait_item_attrs(item: &TraitItem) -> &[Attribute] {
    match item {
        TraitItem::Const(item) => &item.attrs,
        TraitItem::Fn(item) => &item.attrs,
        TraitItem::Type(item) => &item.attrs,
        TraitItem::Macro(item) => &item.attrs,
        TraitItem::Verbatim(_) => &[],
        _ => &[],
    }
}

fn expr_attrs(expr: &Expr) -> &[Attribute] {
    match expr {
        Expr::Array(value) => &value.attrs,
        Expr::Assign(value) => &value.attrs,
        Expr::Async(value) => &value.attrs,
        Expr::Await(value) => &value.attrs,
        Expr::Binary(value) => &value.attrs,
        Expr::Block(value) => &value.attrs,
        Expr::Break(value) => &value.attrs,
        Expr::Call(value) => &value.attrs,
        Expr::Cast(value) => &value.attrs,
        Expr::Closure(value) => &value.attrs,
        Expr::Const(value) => &value.attrs,
        Expr::Continue(value) => &value.attrs,
        Expr::Field(value) => &value.attrs,
        Expr::ForLoop(value) => &value.attrs,
        Expr::Group(value) => &value.attrs,
        Expr::If(value) => &value.attrs,
        Expr::Index(value) => &value.attrs,
        Expr::Infer(value) => &value.attrs,
        Expr::Let(value) => &value.attrs,
        Expr::Lit(value) => &value.attrs,
        Expr::Loop(value) => &value.attrs,
        Expr::Macro(value) => &value.attrs,
        Expr::Match(value) => &value.attrs,
        Expr::MethodCall(value) => &value.attrs,
        Expr::Paren(value) => &value.attrs,
        Expr::Path(value) => &value.attrs,
        Expr::Range(value) => &value.attrs,
        Expr::RawAddr(value) => &value.attrs,
        Expr::Reference(value) => &value.attrs,
        Expr::Repeat(value) => &value.attrs,
        Expr::Return(value) => &value.attrs,
        Expr::Struct(value) => &value.attrs,
        Expr::Try(value) => &value.attrs,
        Expr::TryBlock(value) => &value.attrs,
        Expr::Tuple(value) => &value.attrs,
        Expr::Unary(value) => &value.attrs,
        Expr::Unsafe(value) => &value.attrs,
        Expr::While(value) => &value.attrs,
        Expr::Yield(value) => &value.attrs,
        _ => &[],
    }
}

fn attribute_requires_test(attribute: &Attribute) -> bool {
    // The built-in `test` attribute is test-only WITHOUT a `cfg`: rustc omits `#[test] fn` from a
    // normal build exactly as it omits a `#[cfg(test)]` item. Recognising only `cfg` left a
    // standalone `#[test] fn` that reads `captured.tree_kill` inside a `src` file in the
    // production projection, so the tripwire reported a consumer that no production build has.
    if attribute.path().is_ident("test") {
        return true;
    }
    if !attribute.path().is_ident("cfg") {
        return false;
    }
    let Meta::List(list) = &attribute.meta else {
        return false;
    };
    let Ok(predicate) = list.parse_args::<Meta>() else {
        return false;
    };
    cfg_requires_test(&predicate)
}

/// Whether a cfg predicate is provably true only in a test configuration.
///
/// `all` requires one test-only operand; `any` requires every alternative to be test-only.
/// Therefore `all(any(feature = "prod", test))` is retained while `all(test, unix)` is blanked.
fn cfg_requires_test(predicate: &Meta) -> bool {
    match predicate {
        Meta::Path(path) => path.is_ident("test"),
        Meta::List(list) => {
            let Ok(arguments) = list.parse_args_with(
                syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated,
            ) else {
                return false;
            };
            if list.path.is_ident("all") {
                arguments.iter().any(cfg_requires_test)
            } else if list.path.is_ident("any") {
                !arguments.is_empty() && arguments.iter().all(cfg_requires_test)
            } else {
                false
            }
        }
        Meta::NameValue(_) => false,
    }
}

/// A bounded association between one receiver name and one lexical function/impl scope.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ReceiverKind {
    CapturedProcess,
    Other,
    Unknown,
}

#[derive(Clone)]
struct ReceiverScope {
    name: String,
    kind: ReceiverKind,
    start: usize,
    end: usize,
}

/// Associate annotated bindings with their nearest lexical function/impl scope.
///
/// This recognizes direct `name: CapturedProcess` bindings and `self` in an
/// `impl CapturedProcess` block. A dotted receiver without a local association is retained as an
/// UNKNOWN reader, so inferred bindings such as `let add = run_in_workspace(...); add.tree_kill`
/// cannot silently make the tripwire green. The classifier is intentionally not an exhaustive
/// Rust type/data-flow proof: aliases, re-exports, return types, macros, and generated code remain
/// explicit residual limits. Associations use function/impl extents, not local block lifetimes:
/// conflicting associations for the same receiver remain visible rather than choosing a shadow
/// that may already have gone out of scope. This can conservatively retain unrelated inner reads.
fn receiver_scopes(source: &str) -> Vec<ReceiverScope> {
    let projected = lexical_projection(source);
    let lines: Vec<&str> = projected.lines().collect();
    let concrete_types = concrete_types(&projected);
    let mut scopes = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let code = line.find("//").map_or(*line, |at| &line[..at]);
        let Some((name, kind)) = annotated_binding(code, &concrete_types)
            .or_else(|| inferred_binding(code).map(|name| (name, ReceiverKind::Unknown)))
        else {
            if code.contains("impl CapturedProcess") {
                scopes.push(ReceiverScope {
                    name: "self".to_owned(),
                    kind: ReceiverKind::CapturedProcess,
                    start: index,
                    end: lexical_scope_end(&lines, index),
                });
            }
            continue;
        };
        scopes.push(ReceiverScope {
            name,
            kind,
            start: index,
            end: lexical_scope_end(&lines, index),
        });
    }
    scopes
}

fn inferred_binding(line: &str) -> Option<String> {
    let (left, _) = line.split_once('=')?;
    let mut words = line.split_whitespace();
    if words.next()? != "let" {
        return None;
    }
    let mut binding = words.next()?.trim_end_matches('=');
    if binding == "mut" {
        binding = words.next()?.trim_end_matches('=');
    }
    if binding.is_empty()
        || !binding
            .chars()
            .next()
            .is_some_and(|character| character == '_' || character.is_alphabetic())
        || !binding
            .chars()
            .all(|character| character == '_' || character.is_alphanumeric())
        || left.contains(':')
    {
        return None;
    }
    Some(binding.to_owned())
}

fn concrete_types(source: &str) -> std::collections::BTreeSet<String> {
    source
        .lines()
        .filter_map(|line| {
            let code = line.find("//").map_or(line, |at| &line[..at]).trim_start();
            let rest = code
                .strip_prefix("struct ")
                .or_else(|| code.strip_prefix("enum "))
                .or_else(|| code.strip_prefix("union "))?;
            Some(
                rest.chars()
                    .take_while(|character| character.is_alphanumeric() || *character == '_')
                    .collect(),
            )
        })
        .collect()
}

fn annotated_binding(
    line: &str,
    concrete_types: &std::collections::BTreeSet<String>,
) -> Option<(String, ReceiverKind)> {
    let type_at = line.find("CapturedProcess");
    let (binding, kind) = if let Some(type_at) = type_at {
        let prefix = &line[..type_at];
        let colon = prefix.match_indices(':').rev().find_map(|(at, _)| {
            (prefix.as_bytes().get(at.wrapping_sub(1)) != Some(&b':')
                && prefix.as_bytes().get(at + 1) != Some(&b':'))
            .then_some(at)
        })?;
        (&prefix[..colon], ReceiverKind::CapturedProcess)
    } else {
        let colon = line.match_indices(':').rev().find_map(|(at, _)| {
            (line.as_bytes().get(at.wrapping_sub(1)) != Some(&b':')
                && line.as_bytes().get(at + 1) != Some(&b':'))
            .then_some(at)
        })?;
        let after = line[colon + 1..].trim_start();
        let type_name: String = after
            .chars()
            .skip_while(|character| *character == '&' || character.is_whitespace())
            .take_while(|character| character.is_alphanumeric() || *character == '_')
            .collect();
        if type_name.is_empty() {
            return None;
        }
        let kind = if concrete_types.contains(&type_name) {
            ReceiverKind::Other
        } else {
            ReceiverKind::Unknown
        };
        (&line[..colon], kind)
    };
    let binding = binding.trim_end();
    let end = binding.len();
    let start = binding
        .char_indices()
        .rev()
        .find(|(_, character)| !character.is_alphanumeric() && *character != '_')
        .map_or(0, |(at, character)| at + character.len_utf8());
    (start < end
        && binding[start..]
            .chars()
            .next()
            .is_some_and(|character| character == '_' || character.is_alphabetic())
        && binding[start..]
            .chars()
            .all(|character| character == '_' || character.is_alphanumeric()))
    .then(|| (binding[start..].to_owned(), kind))
}

fn lexical_scope_end(lines: &[&str], start: usize) -> usize {
    let scope_start = (0..=start)
        .rev()
        .find(|index| {
            let code = lines[*index]
                .find("//")
                .map_or(lines[*index], |at| &lines[*index][..at]);
            code.contains("fn ") || code.contains("impl ")
        })
        .unwrap_or(start);
    if scope_start == start && !lines[start].contains('{') {
        return start;
    }
    let mut depth = 0_i32;
    let mut opened = false;
    for (index, line) in lines.iter().enumerate().skip(scope_start) {
        let delta = brace_delta(line);
        depth += delta;
        opened |= delta > 0;
        if opened && depth <= 0 {
            return index;
        }
    }
    lines.len().saturating_sub(1)
}

fn reads_the_field_for_receivers(line: &str, line_index: usize, scopes: &[ReceiverScope]) -> bool {
    let code = match line.find("//") {
        Some(at) => &line[..at],
        None => line,
    };
    let bytes = code.as_bytes();
    code.match_indices("tree_kill").any(|(field_start, _)| {
        let Some(dot) = field_start.checked_sub(1) else {
            return false;
        };
        if bytes[dot] != b'.' {
            return false;
        }
        let receiver_end = dot;
        let receiver_start = code[..receiver_end]
            .char_indices()
            .rev()
            .find(|(_, character)| !character.is_alphanumeric() && *character != '_')
            .map_or(0, |(at, character)| at + character.len_utf8());
        let receiver = &code[receiver_start..receiver_end];
        let mut associations = scopes
            .iter()
            .filter(|scope| {
                scope.name == receiver && scope.start <= line_index && line_index <= scope.end
            })
            .peekable();
        // Unknown receiver classification is visible (a safe false positive) rather than a
        // silent negative. Local block lifetimes are not represented by these extents, so a
        // later Other binding cannot erase an earlier CapturedProcess/Unknown association:
        // the shadow may have ended before this read. Exclude only unanimous Other associations.
        associations.peek().is_none() || associations.any(|scope| scope.kind != ReceiverKind::Other)
    })
}

fn blank_range(bytes: &mut [u8], range: std::ops::Range<usize>) {
    for byte in &mut bytes[range] {
        if *byte != b'\n' && *byte != b'\r' {
            *byte = b' ';
        }
    }
}

/// Whether a path's LAST segment is the record's name.
///
/// The last segment, never the whole path: `graphhelm_tool_host::CapturedProcess { .. }` is the
/// same pattern as `CapturedProcess { .. }`, and a lexical rule that compared the text before the
/// brace could not say so.
fn path_names_record(path: &syn::Path) -> bool {
    path.segments
        .last()
        .is_some_and(|segment| segment.ident == "CapturedProcess")
}

/// Every PATTERN that names the record, as byte ranges, decided by `syn` rather than by text.
///
/// WHY THIS REPLACED A TEXT COMPARISON. The previous rule took the text before `CapturedProcess {`
/// and compared it for EQUALITY against `let`, `if let`, `while let` and `for`. That is not "is
/// this a pattern position", it is "is this line shaped like the four spellings I thought of", and
/// lane S measured four consumers it therefore could not see -- each naming the record literally
/// on the pattern line, each silent:
///
///   `let Some(CapturedProcess { tree_kill, .. }) = maybe else { .. }`   nested in a tuple struct
///   `let graphhelm_tool_host::CapturedProcess { tree_kill, .. } = ..`   qualified by module path
///   `let &CapturedProcess { tree_kill, .. } = reference;`               behind a reference pattern
///   `let (_code, CapturedProcess { tree_kill, .. }) = pair;`            inside a tuple pattern
///
/// A `Pat` is a pattern wherever it appears, so asking the AST removes the enumeration entirely:
/// `let`, `let ... else`, `if let`, `while let`, `for`, match arms, function parameters and
/// closure parameters all arrive as `Pat` nodes, and nesting through tuple-struct, reference,
/// tuple, or-, and slice patterns is just recursion. A struct LITERAL is an `Expr`, never a `Pat`,
/// so the construction that the old arrow search misread cannot reach this at all.
///
/// Returns `None` when the text does not parse -- a single fragment row, not a file -- so the
/// caller can fall back rather than treat an unparseable fragment as "no pattern".
fn record_pattern_ranges(source: &str) -> Option<Vec<std::ops::Range<usize>>> {
    #[derive(Default)]
    struct RecordPatterns {
        ranges: Vec<std::ops::Range<usize>>,
    }

    impl<'ast> Visit<'ast> for RecordPatterns {
        fn visit_pat(&mut self, pat: &'ast syn::Pat) {
            let names_record = match pat {
                syn::Pat::Struct(pattern) => path_names_record(&pattern.path),
                syn::Pat::TupleStruct(pattern) => path_names_record(&pattern.path),
                syn::Pat::Path(pattern) => path_names_record(&pattern.path),
                _ => false,
            };
            if names_record {
                self.ranges.push(pat.span().byte_range());
            }
            visit::visit_pat(self, pat);
        }
    }

    // A whole file first; otherwise the same text as the body of one function, which is what makes
    // a bare statement (`for ... { }`, a `let ... else`) parseable without changing its meaning.
    let (file, offset) = match syn::parse_file(source) {
        Ok(file) => {
            let offset = parsed_source_offset(source, &file);
            (file, offset)
        }
        Err(_) => {
            const PREFIX: &str = "fn __census_probe() {\n";
            let wrapped = format!("{PREFIX}{source}\n}}");
            let file = syn::parse_file(&wrapped).ok()?;
            let mut visitor = RecordPatterns::default();
            visitor.visit_file(&file);
            return Some(
                visitor
                    .ranges
                    .into_iter()
                    .map(|range| {
                        let start = range.start.saturating_sub(PREFIX.len()).min(source.len());
                        let end = range.end.saturating_sub(PREFIX.len()).min(source.len());
                        start..end
                    })
                    .collect(),
            );
        }
    };
    let mut visitor = RecordPatterns::default();
    visitor.visit_file(&file);
    Some(
        visitor
            .ranges
            .into_iter()
            .map(|range| (range.start + offset)..(range.end + offset))
            .collect(),
    )
}

/// The byte offset at which each line of `source` starts.
fn line_start_offsets(source: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (index, byte) in source.bytes().enumerate() {
        if byte == b'\n' {
            starts.push(index + 1);
        }
    }
    starts
}

fn reader_lines(source: &str) -> Vec<usize> {
    let projected = lexical_projection(source);
    let lines: Vec<&str> = projected.lines().collect();
    let scopes = receiver_scopes(source);
    let mut readers = std::collections::BTreeSet::new();

    for (index, line) in lines.iter().enumerate() {
        if reads_the_field_for_receivers(line, index, &scopes) {
            readers.insert(index);
        }
    }

    // `lexical_projection` preserves every byte offset and every newline, so a byte range taken
    // from the AST of `source` names the same lines in `projected`.
    let ast_patterns = record_pattern_ranges(source);
    if let Some(ranges) = &ast_patterns {
        let starts = line_start_offsets(source);
        for range in ranges {
            for (index, line) in lines.iter().enumerate() {
                let line_start = starts[index];
                let line_end = line_start + line.len();
                if line_start >= range.end || line_end < range.start {
                    continue;
                }
                if line
                    .find("//")
                    .map_or(*line, |at| &line[..at])
                    .contains("tree_kill")
                {
                    readers.insert(index);
                }
            }
        }
    }

    for (start, line) in lines.iter().enumerate() {
        let line = *line;
        let code = line.find("//").map_or(line, |at| &line[..at]);
        let Some(type_start) = code.find("CapturedProcess {") else {
            continue;
        };

        // Walk to the record's OWN closing brace, remembering where it sits. The byte after it is
        // the only place an arm arrow may appear: `CapturedProcess { .. } => body`. Scanning the
        // whole span for `=>` instead read an arrow NESTED inside the braces -- as in
        // `CapturedProcess { tree_kill: match value { Some(v) => Some(v), _ => None }, .. }` --
        // and called a legitimate construction a destructuring pattern, so its `tree_kill:` write
        // was reported as a read.
        let mut depth = 0i32;
        let mut end = start;
        let mut close: Option<usize> = None;
        let mut cursor = type_start;
        loop {
            let line_code = if end == start {
                code
            } else {
                let next = lines[end];
                next.find("//").map_or(next, |at| &next[..at])
            };
            let mut found = None;
            for (offset, byte) in line_code.as_bytes().iter().enumerate().skip(cursor) {
                match byte {
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            found = Some(offset + 1);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if let Some(offset) = found {
                close = Some(offset);
                break;
            }
            end += 1;
            cursor = 0;
            if end >= lines.len() {
                break;
            }
        }
        let Some(close_at) = close else {
            continue;
        };

        let prefix = code[..type_start].trim();
        // Every Rust position where `CapturedProcess { .. }` is a PATTERN rather than a literal,
        // to the extent a bounded lexical observer can name them. `for` and parameter positions
        // were missing: `for CapturedProcess { tree_kill, .. } in values`, a function parameter,
        // and a closure parameter each destructure the record while carrying neither `let` nor an
        // arrow, so a first consumer written that way left this tripwire green.
        let direct_pattern = matches!(prefix, "let" | "if let" | "while let" | "for");
        let parameter_pattern = {
            let head = code[..type_start].trim_end();
            let opens_parameter = head.ends_with('(') || head.ends_with(',');
            let function_parameter =
                opens_parameter && (code.contains("fn ") || code.starts_with("fn "));
            let closure_parameter =
                head.ends_with('|') || (opens_parameter && head.matches('|').count() % 2 == 1);
            function_parameter || closure_parameter
        };
        let context_start = start.saturating_sub(8);
        let context = lines[context_start..=start].join("\n");
        let record_offset = lines[context_start..start]
            .iter()
            .map(|line| line.len() + 1)
            .sum::<usize>()
            + type_start;
        let macro_pattern = context[..record_offset]
            .rfind("matches!(")
            .is_some_and(|at| {
                let mut depth = 0;
                let mut pattern_argument = false;
                for byte in context[at..record_offset].bytes() {
                    match byte {
                        b'(' => depth += 1,
                        b')' => {
                            depth -= 1;
                            if depth == 0 {
                                return false;
                            }
                        }
                        b',' if depth == 1 => pattern_argument = true,
                        _ => {}
                    }
                }
                depth > 0 && pattern_argument
            });
        let match_arm = {
            let closing = lines[end];
            let closing = closing.find("//").map_or(closing, |at| &closing[..at]);
            closing
                .get(close_at..)
                .is_some_and(|tail| tail.contains("=>"))
        };

        // `matches!(value, CapturedProcess { tree_kill, .. })` stays LEXICAL on purpose: `syn`
        // keeps a macro's body as an unparsed `TokenStream`, so no `Pat` node exists for the AST
        // pass to find. Everything else defers to the AST whenever the text parsed; the text
        // comparisons below survive only as the fallback for a fragment that is not a file and
        // not a function body -- a single truth-table row -- where there is no AST to ask.
        let accepted = if ast_patterns.is_some() {
            macro_pattern
        } else {
            direct_pattern || parameter_pattern || macro_pattern || match_arm
        };
        if !accepted {
            continue;
        }
        for (index, line) in lines.iter().enumerate().take(end + 1).skip(start) {
            let line = *line;
            if line
                .find("//")
                .map_or(line, |at| &line[..at])
                .contains("tree_kill")
            {
                readers.insert(index);
            }
        }
    }

    readers.into_iter().collect()
}

fn brace_delta(code: &str) -> i32 {
    code.bytes().fold(0, |depth, byte| match byte {
        b'{' => depth + 1,
        b'}' => depth - 1,
        _ => depth,
    })
}

/// The predicate's own truth table, from the rows two lanes measured against the first version.
///
/// Driven as TEXT rather than by editing production code: J and L each injected lines into
/// `host.rs`, built the crate and ran the cell, which is what found the defects -- and what a
/// reviewer cannot repeat cheaply. These rows make the property assertable in 0.1s, so the next
/// change to `reads_the_field` cannot quietly re-open any of them.
#[test]
fn the_predicate_answers_the_rows_two_lanes_measured() {
    // READS -- every one of these is a consumer appearing.
    for line in [
        "    if captured.tree_kill.is_some() {}",
        "            tree_kill: captured.tree_kill,",
        "    let CapturedProcess { tree_kill, .. } = captured;",
        "    let CapturedProcess { tree_kill: tk, .. } = captured;",
        "        CapturedProcess { tree_kill: Some(t), .. } => t,",
        "        self.tree_kill.is_some()",
    ] {
        assert!(
            reads_the_field(line),
            "a consumer must be visible, and this one is not: {line}"
        );
    }

    // NOT READS -- writes, declarations, same-named locals, and a mention in a trailing comment.
    for line in [
        "            tree_kill: None,",
        "    pub tree_kill: Option<graphhelm_process_tree::TerminationOutcome>,",
        "    let mut tree_kill = None;",
        "                    tree_kill = Some(graphhelm_process_tree::terminate());",
        "        tree_kill,",
        "    let captured = CapturedProcess { tree_kill: None, ..default };",
        "        let _ = 1u8; // the tree_kill field is unused here",
        "    // tree_kill is dropped by name at both call sites",
    ] {
        assert!(
            !reads_the_field(line),
            "this line reads nothing and must not be reported as a consumer: {line}"
        );
    }
}

#[test]
fn the_predicate_sees_multiline_patterns_and_macro_patterns() {
    let source = r#"
        fn future_consumer(captured: CapturedProcess) {
            let CapturedProcess {
                tree_kill,
                ..
            } = captured;
            let escaped = matches!(
                captured,
                CapturedProcess {
                    tree_kill: Some(_),
                    ..
                }
            );
            let _ = (tree_kill, escaped);
        }
    "#;

    let readers = reader_lines(source);
    let reader_text: Vec<&str> = readers
        .iter()
        .map(|&index| {
            source
                .lines()
                .nth(index)
                .expect("reader line must exist")
                .trim()
        })
        .collect();
    assert_eq!(
        reader_text,
        ["tree_kill,", "tree_kill: Some(_),"],
        "both the multiline destructure and matches! pattern must count as reads"
    );

    assert!(reader_lines(
        "let value = CapturedProcess { tree_kill: None, ..base }; let unrelated = matches!(value, _);"
    ).is_empty(), "a later macro is not context for an earlier construction");
    assert!(reader_lines(
        "let matched = matches!(value, Some(_));\nlet captured = CapturedProcess {\n tree_kill: None,\n ..base\n};"
    ).is_empty(), "an already closed macro is not context for a later construction");
}

#[test]
fn the_projection_excludes_cfg_test_readers_and_unrelated_receivers() {
    let source = r#"
struct Other;

#[cfg(test)]
mod tests {
    fn test_only(captured: CapturedProcess, value: &'static str) {
        let _ = captured.tree_kill;
    }
}

#[cfg(all(any(feature = "prod", test, feature = "other")))]
fn nested_cfg_is_not_test_only(captured: CapturedProcess) {
    let _ = captured.tree_kill;
}

#[cfg(all(test, feature = "test_fixture"))]
fn nested_cfg_is_test_only(captured: CapturedProcess) {
    let _ = captured.tree_kill;
}

fn production(captured: CapturedProcess) {
    let _ = captured.tree_kill;
}

type Alias = CapturedProcess;

fn aliased(captured: Alias) {
    let _ = captured.tree_kill;
}

fn inferred() {
    let add = run_in_workspace();
    let _ = add.tree_kill;
}

fn same_name_captured(captured: CapturedProcess) {
    let _ = captured.tree_kill;
}

fn same_name_unrelated(captured: Other) {
    let _ = captured.tree_kill;
}

fn shadowed_untyped(captured: Other) {
    let captured = run_in_workspace();
    let _ = captured.tree_kill;
}

fn later_inferred_same_name() {
    let captured = make_captured_process();
    let _ = captured.tree_kill;
}

fn unrelated(outcome: Other) {
    let _ = outcome.tree_kill;
}
"#;
    let projected = production_source(source);
    let readers = reader_lines(&projected);
    let reader_text: Vec<&str> = readers
        .iter()
        .map(|&index| {
            source
                .lines()
                .nth(index)
                .expect("reader line must exist")
                .trim()
        })
        .collect();
    assert_eq!(
        reader_text,
        [
            "let _ = captured.tree_kill;",
            "let _ = captured.tree_kill;",
            "let _ = captured.tree_kill;",
            "let _ = add.tree_kill;",
            "let _ = captured.tree_kill;",
            "let _ = captured.tree_kill;",
            "let _ = captured.tree_kill;",
        ],
        "a cfg(test)-only read and an unrelated typed receiver must not enter the production census; an inferred receiver remains visible"
    );
}

#[test]
fn a_nested_other_shadow_cannot_hide_the_outer_captured_receiver() {
    let source = "struct Other;\nfn production(captured: CapturedProcess) {\n    {\n        let captured: Other = Other;\n    }\n    let _ = captured.tree_kill;\n}\n";
    assert_eq!(
        reader_lines(&production_source(source)),
        [5],
        "the inner Other shadow has ended before the outer CapturedProcess read"
    );

    let unrelated =
        "struct Other;\nfn production(captured: Other) {\n    let _ = captured.tree_kill;\n}\n";
    assert!(
        reader_lines(&production_source(unrelated)).is_empty(),
        "an unambiguous unrelated receiver remains excluded"
    );
}

#[test]
fn the_projection_accounts_for_bom_and_shebang_span_offsets() {
    let source = "\u{feff}#!/usr/bin/env rustx\n#[cfg(test)]\nfn test_only(value: &'static str) {\n    let _ = value;\n    let _ = CapturedProcess { tree_kill: None };\n}\nfn production(captured: CapturedProcess) {\n    let _ = captured.tree_kill;\n}\n";
    let file = syn::parse_file(source).expect("BOM and shebang source parses");
    assert_eq!(
        parsed_source_offset(source, &file),
        "\u{feff}#!/usr/bin/env rustx".len()
    );
    let projected = production_source(source);
    assert!(
        !projected
            .lines()
            .nth(4)
            .is_some_and(|line| line.contains("tree_kill")),
        "the cfg(test) item after a BOM and shebang must be blanked"
    );
    assert!(
        projected
            .lines()
            .nth(7)
            .is_some_and(|line| line.contains("captured.tree_kill")),
        "the production item after a BOM and shebang must remain at its original line"
    );
}

#[test]
fn the_projection_does_not_strip_a_commented_inner_attribute() {
    let source = "#! /*comment*/ [allow(dead_code)]\n#[cfg(test)]\nfn test_only(captured: CapturedProcess) {\n    let _ = captured.tree_kill;\n}\nfn production(captured: CapturedProcess) {\n    let _ = captured.tree_kill;\n}\n";
    let file = syn::parse_file(source).expect("commented inner attribute parses");
    assert!(
        file.shebang.is_none(),
        "syn must retain this valid inner attribute"
    );
    let projected = production_source(source);
    assert!(
        !projected
            .lines()
            .nth(3)
            .is_some_and(|line| line.contains("tree_kill")),
        "the test-only item after a commented inner attribute must be blanked"
    );
    assert!(
        projected
            .lines()
            .nth(6)
            .is_some_and(|line| line.contains("captured.tree_kill")),
        "the production item after a commented inner attribute must remain"
    );
}

#[test]
fn the_projection_masks_comments_literals_and_test_only_expression_observers() {
    let source = r#"
fn production(captured: CapturedProcess) {
    let url = "http://example.test/tree_kill";
    let _ = captured.tree_kill;
    /* captured.tree_kill */
    /** captured.tree_kill */
    let _ = "captured.tree_kill";
    #[cfg(test)] let hidden_local = captured.tree_kill;
    #[cfg(test)] { let _ = captured.tree_kill; }
    #[cfg(test)] reads_a_field!(captured);
    #[cfg(any(test, feature = "production"))] { let _ = captured.tree_kill; }
}

macro_rules! reads_a_field {
    ($value:expr) => { $value.tree_kill };
}
"#;
    let projected = production_source(source);
    let readers = reader_lines(&projected);
    let reader_text: Vec<&str> = readers
        .iter()
        .map(|&index| {
            source
                .lines()
                .nth(index)
                .expect("reader line must exist")
                .trim()
        })
        .collect();
    assert_eq!(
        reader_text,
        [
            "let _ = captured.tree_kill;",
            "#[cfg(any(test, feature = \"production\"))] { let _ = captured.tree_kill; }",
            "($value:expr) => { $value.tree_kill };",
        ],
        "URL text, literals, comments, and cfg(test) local/block expressions must not become production reads while a real read and macro token remain visible"
    );
    assert!(reads_the_field(
        "let url = \"http://example.test\"; let _ = captured.tree_kill;"
    ));
    assert!(!reads_the_field(
        "let note = \"captured.tree_kill\"; /* captured.tree_kill */"
    ));
    assert!(!reads_the_field(r##"let note = r#"captured.tree_kill"#;"##));
    assert!(!reads_the_field("/* captured.tree_kill */"));

    let typed_string_initializer = r#"
struct Other;
fn typed(other: Other, captured: CapturedProcess) {
    let other: Other = "http://example.test/tree_kill";
    let _ = other.tree_kill;
    let _ = captured.tree_kill;
}
"#;
    let readers = reader_lines(&production_source(typed_string_initializer));
    assert_eq!(
        readers,
        [5],
        "a typed Other receiver with a string initializer stays excluded while the real CapturedProcess read remains visible"
    );
}

#[test]
#[should_panic(expected = "OBSERVER_MISSING")]
fn lexical_projection_refuses_unparseable_source() {
    let _ = lexical_projection("let value = \\\"unterminated");
}

/// The pattern positions that carry neither `let` nor an arrow, and the arrow that is not an arm.
///
/// RED before the change in this commit: rows 1-3 returned NO reader (`for`, a function parameter
/// and a closure parameter were absent from `direct_pattern`), and row 4 returned a reader (the
/// nested `=>` inside the literal's braces made `match_arm` true at any depth). Each row names the
/// exact spelling a reviewer raised, so re-opening any of them fails here rather than in a census.
#[test]
fn pattern_positions_without_let_and_arrows_that_are_not_arms() {
    let for_loop = "for CapturedProcess { tree_kill, .. } in values {\n    let _ = tree_kill;\n}\n";
    assert!(
        !reader_lines(for_loop).is_empty(),
        "a `for` destructuring pattern must be a reader"
    );

    let parameter = "fn consume(CapturedProcess { tree_kill, .. }: CapturedProcess) {\n    let _ = tree_kill;\n}\n";
    assert!(
        !reader_lines(parameter).is_empty(),
        "a function-parameter destructuring pattern must be a reader"
    );

    let closure = "fn run() {\n    let f = |CapturedProcess { tree_kill, .. }| tree_kill;\n    let _ = f;\n}\n";
    assert!(
        !reader_lines(closure).is_empty(),
        "a closure-parameter destructuring pattern must be a reader"
    );

    let nested_arrow = "fn build(value: Option<u8>) -> CapturedProcess {\n    CapturedProcess {\n        tree_kill: match value {\n            Some(v) => Some(v),\n            _ => None,\n        },\n    }\n}\n";
    assert!(
        reader_lines(nested_arrow).is_empty(),
        "an inline match inside a CONSTRUCTION is not a destructuring pattern, so its `tree_kill:` write is not a read"
    );
}

/// `#[test]` without a `cfg`, and a `#[cfg(test)]` match arm, are both absent from a normal build.
///
/// RED before the change in this commit: the standalone `#[test] fn` survived
/// `attribute_requires_test` (which asked only about `cfg`), and the attributed arm was walked
/// through by `syn` without ever reaching a recorded node. Both then reported a production
/// consumer the compiler never builds.
#[test]
fn the_projection_blanks_standalone_test_functions_and_attributed_arms() {
    let standalone = "#[test]\nfn only_in_tests(captured: CapturedProcess) {\n    assert!(captured.tree_kill.is_some());\n}\n";
    let projected = production_source(standalone);
    assert!(
        !projected.contains("tree_kill"),
        "a standalone #[test] fn is not production code:\n{projected}"
    );
    assert_eq!(
        projected.lines().count(),
        standalone.lines().count(),
        "the projection must preserve line numbers"
    );

    let arm = "fn dispatch(captured: CapturedProcess, which: u8) {\n    match which {\n        #[cfg(test)]\n        0 => {\n            let _ = captured.tree_kill;\n        }\n        _ => {}\n    }\n}\n";
    let projected = production_source(arm);
    assert!(
        !projected.contains("tree_kill"),
        "a #[cfg(test)] match arm is not production code:\n{projected}"
    );
    assert!(
        reader_lines(&projected).is_empty(),
        "a #[cfg(test)] match arm must not be reported as a production reader"
    );
}

/// The walk decides on the LINK, not on its target.
///
/// RED before the change in this commit: `path.is_dir()` follows a directory symlink, so a link
/// pointing at an ancestor re-entered the same tree and one pointing outside the member root made
/// the gate read host paths it was never given. Skipped on a host without symlink privilege
/// rather than asserted falsely.
#[test]
fn the_source_walk_refuses_directory_symlinks() {
    let temporary = tempfile::tempdir().expect("a temporary directory");
    let root = temporary.path();
    let real = root.join("real");
    std::fs::create_dir_all(real.join("nested")).expect("the real tree");
    std::fs::write(real.join("nested").join("inside.rs"), "fn a() {}\n").expect("a real source");

    let outside = root.join("outside");
    std::fs::create_dir_all(&outside).expect("the outside tree");
    std::fs::write(outside.join("escape.rs"), "fn b() {}\n").expect("an outside source");

    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(&outside, real.join("link")).is_ok();
    // On Windows a directory SYMLINK needs `SeCreateSymbolicLinkPrivilege`, which a developer
    // shell usually lacks; a directory JUNCTION needs no privilege and is the same hazard --
    // `is_dir()` follows it, `file_type().is_symlink()` reports it. Falling back to the junction
    // is what makes this cell observe the fix on this host instead of declining on every run.
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_dir(&outside, real.join("link")).is_ok()
        || std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(real.join("link"))
            .arg(&outside)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
    if !linked {
        // Creating a symlink needs a privilege this host may not grant. Absence of the privilege
        // is not evidence about the walk, so the cell declines rather than passing vacuously.
        eprintln!("OBSERVER_MISSING: this host refused to create a directory symlink");
        return;
    }

    let found = sources_under(&real);
    assert!(
        found.iter().any(|path| path.ends_with("inside.rs")),
        "the walk must still collect real files: {found:?}"
    );
    assert!(
        !found.iter().any(|path| path.ends_with("escape.rs")),
        "the walk followed a directory symlink out of the member root: {found:?}"
    );
}

/// The bound is applied BEFORE the file is read, and it refuses rather than skipping.
///
/// RED before the change in this commit: no size check preceded `read_to_string`, so an
/// oversized `.rs` file entered the population and `production_source` allocated several further
/// buffers its size. Skipping such a file silently would be worse than failing: it would report
/// an absence the census never measured.
#[test]
fn the_source_walk_refuses_a_file_above_the_documented_bound() {
    let temporary = tempfile::tempdir().expect("a temporary directory");
    let src = temporary.path().join("src");
    std::fs::create_dir_all(&src).expect("a source directory");
    let mut oversized = String::from("// ");
    oversized.push_str(&"x".repeat(MAX_SOURCE_BYTES as usize + 1));
    std::fs::write(src.join("huge.rs"), &oversized).expect("the oversized source");

    let refusal = std::panic::catch_unwind(|| sources_under(&src))
        .expect_err("an oversized source must refuse the census");
    let message = refusal
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| refusal.downcast_ref::<&str>().map(|text| text.to_string()))
        .unwrap_or_default();
    assert!(
        message.contains("OBSERVER_MISSING"),
        "the refusal must be visible as OBSERVER_MISSING, got: {message}"
    );
}

/// The six spellings that name the record on the pattern line without being shaped like `let X {`.
///
/// Lane S measured the first four against `310d55b3` and every one was SILENT: the rule compared
/// the text before `CapturedProcess {` for equality against four keywords, which is a list of
/// spellings rather than the question "is this a pattern". Each row here is a first consumer a
/// reviewer can read, and each must fire.
#[test]
fn a_pattern_is_recognized_wherever_a_pattern_is_legal() {
    let rows = [
        (
            "let-else nested in a tuple struct",
            "fn consume(maybe: Option<CapturedProcess>) {\n    let Some(CapturedProcess { tree_kill, .. }) = maybe else {\n        return;\n    };\n    let _ = tree_kill;\n}\n",
        ),
        (
            "qualified by a module path",
            "fn consume(captured: graphhelm_tool_host::CapturedProcess) {\n    let graphhelm_tool_host::CapturedProcess { tree_kill, .. } = captured;\n    let _ = tree_kill;\n}\n",
        ),
        (
            "behind a reference pattern",
            "fn consume(reference: &CapturedProcess) {\n    let &CapturedProcess { tree_kill, .. } = reference;\n    let _ = tree_kill;\n}\n",
        ),
        (
            "inside a tuple pattern",
            "fn consume(pair: (i32, CapturedProcess)) {\n    let (_code, CapturedProcess { tree_kill, .. }) = pair;\n    let _ = tree_kill;\n}\n",
        ),
        (
            "nested two tuple structs deep",
            "fn consume(value: Result<Option<CapturedProcess>, ()>) {\n    if let Ok(Some(CapturedProcess { tree_kill, .. })) = value {\n        let _ = tree_kill;\n    }\n}\n",
        ),
        (
            "a match arm that is not a bare record pattern",
            "fn consume(maybe: Option<CapturedProcess>) {\n    match maybe {\n        Some(CapturedProcess { tree_kill, .. }) => {\n            let _ = tree_kill;\n        }\n        None => {}\n    }\n}\n",
        ),
    ];
    let mut silent = Vec::new();
    for (name, source) in rows {
        if reader_lines(source).is_empty() {
            silent.push(name);
        }
    }
    assert!(
        silent.is_empty(),
        "{} of {} pattern spellings were silent: {silent:?}",
        silent.len(),
        rows.len()
    );
}

/// The same question asked of the SINGLE-LINE predicate, which carried the identical shape.
#[test]
fn the_single_line_predicate_recognizes_the_same_pattern_spellings() {
    for line in [
        "    let Some(CapturedProcess { tree_kill, .. }) = maybe else { return; };",
        "    let graphhelm_tool_host::CapturedProcess { tree_kill, .. } = captured;",
        "    let &CapturedProcess { tree_kill, .. } = reference;",
        "    let (_code, CapturedProcess { tree_kill, .. }) = pair;",
    ] {
        assert!(
            reads_the_field(line),
            "a destructuring consumer was silent: {line}"
        );
    }
}

/// THE FALSE-POSITIVE CONTROL, kept silent by the same change.
///
/// A struct LITERAL is an `Expr` and never a `Pat`, so the AST cannot mistake a construction for a
/// destructuring; comments, string literals and doc comments are masked before any of this. If a
/// future rule buys pattern coverage by widening the text match, these rows go red.
#[test]
fn constructions_comments_and_literals_stay_silent_under_the_ast_pass() {
    for (name, source) in [
        (
            "a construction whose field is computed by an inline match",
            "fn build(value: Option<u8>) -> CapturedProcess {\n    CapturedProcess {\n        tree_kill: match value {\n            Some(v) => Some(v),\n            _ => None,\n        },\n    }\n}\n",
        ),
        (
            "a plain construction nested in a call",
            "fn build() {\n    let _ = wrap(CapturedProcess { tree_kill: None });\n}\n",
        ),
        (
            "the field named inside a string literal",
            "fn note() {\n    let note = \"CapturedProcess { tree_kill, .. }\";\n    let _ = note;\n}\n",
        ),
        (
            "the field named inside a raw string literal",
            "fn note() {\n    let note = r#\"let CapturedProcess { tree_kill, .. } = captured;\"#;\n    let _ = note;\n}\n",
        ),
        (
            "the field named inside an ordinary comment",
            "fn note() {\n    // let CapturedProcess { tree_kill, .. } = captured;\n    let _ = 1u8;\n}\n",
        ),
        (
            "the field named inside a doc comment",
            "/// let CapturedProcess { tree_kill, .. } = captured;\nfn note() {\n    let _ = 1u8;\n}\n",
        ),
    ] {
        assert!(
            reader_lines(source).is_empty(),
            "a false reader was reported for {name}:\n{source}"
        );
    }
}

/// An unreadable file REFUSES the census; it does not leave the population.
///
/// `read_to_string(path).ok()` silently dropped such a file, and the cell then reported the
/// resulting zero as "no production code reads the field" -- a false absence produced by not
/// looking. Lane R raised it; the refusal names the file and carries the OS error.
#[test]
fn an_unreadable_census_input_refuses_instead_of_shrinking_the_population() {
    let temporary = tempfile::tempdir().expect("a temporary directory");
    let path = temporary.path().join("not-utf8.rs");
    // Invalid UTF-8 is the portable unreadable file: `read_to_string` refuses it on every host,
    // where a permission bit would not reproduce under every runner identity.
    std::fs::write(&path, [0x66, 0x6e, 0x20, 0xff, 0xfe, 0x28, 0x29]).expect("the invalid source");

    let refusal = std::panic::catch_unwind(|| read_census_source(&path))
        .expect_err("an unreadable census input must refuse");
    let message = refusal
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| refusal.downcast_ref::<&str>().map(|text| text.to_string()))
        .unwrap_or_default();
    assert!(
        message.contains("OBSERVER_MISSING"),
        "the refusal must be visible as OBSERVER_MISSING, got: {message}"
    );
    assert!(
        message.contains("not-utf8.rs"),
        "the refusal must name the file it could not read, got: {message}"
    );
}

#[test]
fn the_population_is_the_definition_and_direct_consumers() {
    let root = workspace_root();
    let files = production_sources(&root);
    let has_member = |name: &str| {
        files
            .iter()
            .any(|path| path.components().any(|part| part.as_os_str() == name))
    };
    assert!(has_member("tool-host"), "the defining crate is not scanned");
    assert!(
        has_member("codebase-memory-mcp"),
        "the direct adapter consumer is not scanned"
    );
    assert!(has_member("cli"), "the direct CLI consumer is not scanned");
    assert!(
        has_member("development-benchmark"),
        "the direct tools consumer is not scanned"
    );
    assert!(
        !files.iter().any(|path| {
            path.components()
                .any(|part| part.as_os_str() == "process-tree")
        }),
        "an unrelated process-tree crate entered the consumer population"
    );
}

#[test]
fn no_production_code_reads_the_tool_call_sweep_outcome() {
    let root = workspace_root();
    let files = production_sources(&root);

    // POSITIVE CONTROL, both halves. A zero below must mean absence in the tree, not a walk that
    // opened nothing or a field that has been renamed out from under this cell.
    assert!(
        files.len() > 50,
        "the walk found only {} production sources, so it is measuring itself and not the tree",
        files.len()
    );
    // AND IT REACHES THE MEMBER THAT MADE THIS NECESSARY. `tools/development-benchmark` depends on
    // `graphhelm-tool-host`, so it can read the field; the first version of this walk never looked
    // there. Asserting the ROOT is covered, rather than trusting that deriving from the manifest
    // covered it, is the difference between a fix and a belief about a fix.
    assert!(
        files.iter().any(|path| path
            .components()
            .any(|part| part.as_os_str() == "development-benchmark")),
        "the walk never reached tools/development-benchmark, a member that depends on this crate and could hold the first consumer"
    );
    let mentions = files
        .iter()
        .filter(|path| production_source(&read_census_source(path)).contains("tree_kill"))
        .count();
    assert!(
        mentions > 0,
        "no production file mentions `tree_kill` at all, so this cell is guarding a field that no longer exists under that name"
    );

    let mut readers: Vec<String> = Vec::new();
    for path in &files {
        let text = production_source(&read_census_source(path));
        let relative = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        for index in reader_lines(&text) {
            let line = text.lines().nth(index).expect("reader line must exist");
            readers.push(format!("{relative}:{}  {}", index + 1, line.trim()));
        }
    }

    assert!(
        readers.is_empty(),
        "production code now READS `tree_kill`, which is the premise #868's deferral rests on:\n  {}\n\n\
         Provisioning's cancellation still drops its own sweep answer (`run_supervised`, the two `terminate` calls). While NOTHING read the tool-call boundary's record, the two boundaries were equally blind and the asymmetry was a design. A consumer makes it a defect: one path can act on an escapee, the other cannot see one. Revisit #868 -- either carry the outcome through `SupervisedOutcome::Cancelled` and `HostError::Cancelled`, or record why the provisioning escapee still does not need to reach this reader.",
        readers.join("\n  ")
    );
}
