#!/usr/bin/env bash
#
# census.sh <pr> — read EVERY comment surface of a pull request, emit EVERY row, and fail loudly.
#
# WHY THIS EXISTS. A verdict census taken from one surface reports a confident wrong answer, not an
# incomplete one: J told the board #986 had zero non-author passes after reading
# `issues/<n>/comments` and the GraphQL review threads, while C's APPROVE-WITH-RISK and E's APPROVE
# sat on `pulls/<n>/reviews` — the surface never read.
#
# CONTRACT
#   * reads all three: issues/<n>/comments, pulls/<n>/reviews, pulls/<n>/comments, plus reviewThreads
#   * EMITS EVERY ROW. A row it cannot parse prints with `lane=?` and `verdict=-` and its id, never
#     silently. The first published version dropped unparsed rows: on #986 it printed
#     "inline — entries: 5" and showed TWO, discarding three bot findings that carried no `Lane:`
#     token — the count said 5, the reader saw 2, and there was no id to retrieve the rest.
#     THE FAILURE HAD MOVED, NOT GONE: a tool that reads the right surface and filters its contents
#     away is the same defect one layer in. (L on #1020.)
#   * PAGINATES every surface (`--paginate`). The checklist's own reviews line lacks it and returned
#     30 of 37 on #1005 — the NEWEST dropped, so the most recent pass is the one a reader loses (K).
#   * pages reviewThreads to exhaustion. `first:100` beside a `totalCount` of more is a count that
#     does not cover what it claims — the same shape again. (L on #1020.)
#   * shows `edited=<updated_at>` when a body was edited after posting. The exhaustion clause picks
#     the presser by EARLIEST timestamp, so a comment edited into an approval must not read as an
#     early pass. (L on #1020.)
#   * NO error suppression. A failed read prints `!! READ FAILED ... NOT an empty surface` and the
#     script exits non-zero. An earlier draft wrapped a `jq` call in `2>/dev/null`; `jq` is absent on
#     this shell (gh bundles its own, which is why `gh api --jq` works), and the suppression turned a
#     missing binary into clean counts with no verdicts — an anti-empty tool producing a confident
#     empty.
#   * uses `gh api --jq`. A BARE `jq` IS NOT ON THIS SHELL; do not add one.
#
# WHAT IT DOES NOT DO. It matches the verdict WORD, not the role: a body quoting another lane's
# verdict, or an author's own comment, reads as a verdict here. That is declared, not a defect — the
# reader decides verdicts from the bodies it points at, and the author from the PR's identity line.
# A MISSING ROW would be a defect; there are none.
#
# SLOW. Three paginated surfaces plus GraphQL: over two minutes on a busy PR. Use it to form a
# verdict, not at the instant of pressing.
#
set -u
ROWS=0; SURFACES=0
PR="${1:?usage: census.sh <pr>}"; REPO=stabem/GraphHelm; FAIL=0
US=$'\x1f'
lane_of() { # accept every identity form the protocol uses. NOTE: $f is FLATTENED (newlines became
  # spaces), so `^` no longer means "start of line" — it means start of the whole body. The third
  # form is therefore anchored on start-or-space, not on `^` alone; anchoring it on `^` made every
  # `E | Session: ...` body read as lane=? because the identity line is rarely the first thing.
  local f=" $1" l
  l=$(printf '%s' "$f" | grep -oE 'Lane:[[:space:]]*[A-Za-z0-9]+([[:space:]][0-9]+)?' | head -1 | sed 's/Lane:[[:space:]]*//')
  [ -n "$l" ] && { printf '%s' "$l"; return; }
  l=$(printf '%s' "$f" | grep -oE 'Session:[[:space:]]*[A-Za-z0-9]{1,12}[[:space:]]*·' | head -1 | sed 's/Session:[[:space:]]*//' | sed 's/[[:space:]]*·//')
  [ -n "$l" ] && { printf '%s' "$l"; return; }
  printf '%s' "$f" | grep -oE '[[:space:]][A-Z]{1,8}[[:space:]]*\|[[:space:]]*Session:' | head -1 | sed 's/^[[:space:]]*//' | sed 's/[[:space:]]*|.*$//'
}
surface() { # label, path, timefield, has-edit-instant(yes|no)
  echo "  --- $1 ---"
  local out rc n=0 hasedit="${4:-yes}"
  out=$(gh api --paginate "repos/$REPO/$2" \
    --jq ".[] | [(.$3 // \"?\"), (.updated_at // \"-\"), (.id|tostring), (.user.login // \"?\"), (.body // \"\" | gsub(\"\n\";\"$US\"))] | @tsv"); rc=$?
  if [ $rc -ne 0 ]; then echo "      !! READ FAILED rc=$rc — NOT an empty surface"; FAIL=1; return; fi
  while IFS=$'\t' read -r ts upd id who body; do
    [ -z "${ts:-}" ] && continue
    n=$((n+1))
    # C0 AND ESC ARE STRIPPED BEFORE ANY PRINTING. A body is untrusted text, and this tool exists
    # to help choose who merges: a comment carrying an ESC-bracket-2K-CR sequence` clears the line it is printed on
    # and rewrites the row above it, so a body could hide its own row -- or the completion marker
    # that is this tool's only proof it finished. UTF-8 is untouched (0x80-0xFF), because the lane
    # forms are separated by `·`. (L on #1020.)
    local flat bare lane sha v edited="" lc
    flat=$(printf '%s' "${body//$US/ }" | tr -d '[:cntrl:]')
    lane=$(lane_of "$flat")
    # A DATE IS EIGHT HEX CHARACTERS. `manifest 20260908T014704` yielded ~sha=20260908 (A on #1020).
    # Prefer the sha a body NAMES; fall back to a hex run that contains at least one a-f, which a
    # decimal timestamp cannot.
    sha=$(printf '%s' "$flat" | grep -oiE '(head[^a-z0-9]{0,14}|re-read at[^a-z0-9]{0,3})`?[0-9a-f]{8}' | grep -oiE '[0-9a-f]{8}$' | head -1)
    [ -z "$sha" ] && sha=$(printf '%s' "$flat" | grep -oiE '[0-9a-f]{8}' | grep -iE '[a-f]' | head -1)
    # EVERY verdict word present, never a choice between them. Checking APPROVE first made a body
    # holding BOTH read as a pass: A's own BLOCK on #1002 was classified APPROVE because it also said
    # "I would flip to APPROVE on sight of them" — a refusal reported as consent. Reordering only moves
    # the misclassification, so the ambiguity is PRINTED instead of resolved. (A on #1020.)
    v=""
    # APPROVE-WITH-RISK contains the word APPROVE, so testing the raw body would print `AWR+APPROVE`
    # on every risk verdict — noise, not ambiguity. Remove the compound before asking about the bare
    # word; what must stay visible is APPROVE beside BLOCK.
    # CASE-INSENSITIVE, because a verdict is a word and not a spelling: `Approve`, `approve` and
    # `block` were all read as NO VERDICT, so a lane that wrote its pass in prose scored zero and
    # waited behind everything (L on #1020). Matching happens on a lowercased copy; what is PRINTED
    # is still the canonical upper-case word, so the column stays a closed vocabulary.
    lc=$(printf '%s' "$flat" | tr '[:upper:]' '[:lower:]')
    bare="${lc//approve-with-risk/}"
    printf '%s' "$lc"   | grep -q 'approve-with-risk' && v="AWR"
    printf '%s' "$bare" | grep -q 'approve' && v="${v:+$v+}APPROVE"
    printf '%s' "$lc"   | grep -q 'block'   && v="${v:+$v+}BLOCK"
    # A NEGATED VERDICT WORD IS NOT THAT VERDICT. "I do not APPROVE this" was reported as APPROVE --
    # a refusal read as consent, the same defect this tool already carries a comment about for the
    # both-words case. Consistent with that comment, the ambiguity is MARKED rather than resolved:
    # `~neg` says a negator sits within a few words of a verdict word, and the row must be read
    # before it is counted. Resolving it here would be a second guess about English.
    printf '%s' "$lc" | grep -qE "(do not|don'?t|cannot|can'?t|will not|won'?t|never|not)[a-z0-9 ,'-]{0,18}(approve|block)"       && v="${v}~neg"
    [ -z "$v" ] && v="-"
    # AN EMPTY-BODIED REVIEW IS A CONTAINER, not a verdict: GitHub creates one to hold inline
    # comments and bot reviews. The runner counted these as "reviews" when ordering the queue, which
    # is why a PR with 18 containers and 0 verdicts outranked one with two passes (L on #1020).
    [ -z "$(printf "%s" "$flat" | tr -d "[:space:]")" ] && v="container"
    # THE REVIEW SURFACE HAS NO EDIT INSTANT. `pulls/N/reviews` carries `submitted_at` and no
    # `updated_at` at all, so silence there means UNKNOWABLE, never "not edited" -- and the
    # earliest-pass rule in .factory/MERGE-CHECKLIST.md turns on exactly that difference. An absent
    # column read as "clean" is this tool reporting an unknown as a reassuring negative. (L on #1020.)
    if [ "$hasedit" = "no" ]; then edited="  edited=UNKNOWABLE(surface has no edit instant)"
    elif [ -n "${upd:-}" ] && [ "$upd" != "-" ] && [ "$upd" != "$ts" ]; then edited="  edited=$upd"; fi
    # EVERY ROW PRINTS. An unparsed one carries its id so it can be fetched.
    echo "      $ts  lane=${lane:-?}  ~sha=${sha:-none}  verdict=$v  by=$who  id=$id$edited"
    echo "          | $(printf '%s' "$flat" | cut -c1-96)"
  done <<< "$out"
  ROWS=$((ROWS+n)); SURFACES=$((SURFACES+1))
  echo "      entries: $n"
}
echo "=== PR #$PR census, $(date -u +%H:%M:%SZ) ==="
surface "issue-comments" "issues/$PR/comments" "created_at"   "yes"
surface "reviews"        "pulls/$PR/reviews"   "submitted_at" "no"
surface "inline"         "pulls/$PR/comments"  "created_at"   "yes"
# reviewThreads, PAGED to exhaustion
Q='query($n:Int!,$c:String){repository(owner:"stabem",name:"GraphHelm"){pullRequest(number:$n){reviewThreads(first:100,after:$c){totalCount pageInfo{hasNextPage endCursor} nodes{isResolved}}}}}'
cur=""; total=""; seen=0; unres=0
while :; do
  if [ -z "$cur" ]; then page=$(gh api graphql -f query="$Q" -F n="$PR" --jq '[.data.repository.pullRequest.reviewThreads.totalCount, ([.data.repository.pullRequest.reviewThreads.nodes[]]|length), ([.data.repository.pullRequest.reviewThreads.nodes[]|select(.isResolved==false)]|length), .data.repository.pullRequest.reviewThreads.pageInfo.hasNextPage, (.data.repository.pullRequest.reviewThreads.pageInfo.endCursor // "")] | @tsv')
  else page=$(gh api graphql -f query="$Q" -F n="$PR" -f c="$cur" --jq '[.data.repository.pullRequest.reviewThreads.totalCount, ([.data.repository.pullRequest.reviewThreads.nodes[]]|length), ([.data.repository.pullRequest.reviewThreads.nodes[]|select(.isResolved==false)]|length), .data.repository.pullRequest.reviewThreads.pageInfo.hasNextPage, (.data.repository.pullRequest.reviewThreads.pageInfo.endCursor // "")] | @tsv'); fi
  if [ $? -ne 0 ] || [ -z "$page" ]; then echo "  !! threads READ FAILED — not zero"; FAIL=1; break; fi
  IFS=$'\t' read -r total pn pu more cur <<< "$page"
  seen=$((seen+pn)); unres=$((unres+pu))
  [ "$more" = "true" ] || break
done
if [ $FAIL -eq 0 ]; then
  echo "  --- threads --- total=$total seen=$seen unresolved=$unres"
  [ "${total:-0}" != "$seen" ] && { echo "      !! seen ($seen) != totalCount ($total) — the unresolved count does NOT cover them all"; FAIL=1; }
fi
gh pr view "$PR" --json headRefOid,mergeable,mergeStateStatus,closingIssuesReferences \
  --jq '"  --- state --- head=\(.headRefOid[0:8]) \(.mergeable)/\(.mergeStateStatus) closes=[\([.closingIssuesReferences[].number]|join(","))]"' || FAIL=1
[ $FAIL -ne 0 ] && echo "  ** CENSUS INCOMPLETE — draw no conclusion from it **"
# THE LAST LINE IS THE ONLY PROOF THE RUN FINISHED. The INCOMPLETE banner fires on a failed READ; it
# cannot fire on a run CUT SHORT — piped into `head`, killed by a timeout, a closed terminal — and at
# minutes per census that is the likelier ending. L read this tool mid-write on #1014, 56 of an
# eventual 144 lines, every line correct, and saw `containers=0`; the number was wrong because the
# READING stopped, not because the API lied — this tool's own defect class, aimed at itself.
# If this line is absent, you hold half a census. Do not draw a conclusion from it.
echo "=== census complete: $ROWS rows across $SURFACES surfaces ==="
exit $FAIL
