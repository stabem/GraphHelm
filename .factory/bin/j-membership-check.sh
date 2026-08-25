#!/usr/bin/env bash
# Merge membership check (J). Usage: membership.sh <main-ref> <result-ref> [expected-new...]
#
# FOUR tests, because each answers a question the others cannot:
#   LOST      comm -23  -- nothing main had may be missing        (the merge-loss question)
#   GAINED    comm -13  -- what appeared must be what was declared (guards a third-branch import)
#   DUPS      uniq -d   -- a duplicate in a closed vocabulary makes the equality guard's
#                          subject ambiguous; it is not cosmetic
#   PREFIX              -- main's list must be an unbroken head of the result, IN ORDER.
#                          The set tests above are BLIND to a reorder; append-only is a claim
#                          about ORDER, so a set test guarding it prices the wrong unit.
#
# READ THIS BEFORE SIMPLIFYING THE PREFIX TEST. "Prefix" names TWO different properties and only
# one of them is safe:
#   SAFE (implemented here) diff main's FULL list against head -n <len(main)> of the result.
#                           A hole anywhere inside the head breaks it.
#   BLIND (do not adopt)    "the entries of main that ARE present appear in order".
#                           Measured on real data: 6 of main's 43 entries missing -> this
#                           formulation returns OK.
# The second reading keeps the name and the green and loses the property. LOST also catches the
# missing entries, so the pair covers it twice -- but do not let that be the only thing holding
# it. (Fifth failure form raised by N on #295; measured against this script before being written
# down here.)
# Run on BOTH halves independently. The Rust<->schema equality guard is NOT used as a bridge:
# it goes green in the exact failure mode this exists to catch (both halves lose together).
set -u
MAIN="$1"; RES="$2"; shift 2; EXPECTED="$*"
D=$(mktemp -d); RC=0
RS=core/protocols/src/development.rs
SC=extensions/builtin/graphhelm-development-contracts/schemas/development-envelope.schema.json

rust_ordered() { git show "$1:$RS" | sed -n '/DevelopmentRefusalCode {/,/^    }/p' \
  | grep -oE '=> "[a-z_]+"' | sed 's/=> //;s/"//g' | tr -d '\r'; }
schema_ordered() { git show "$1:$SC" > "$D/x.json" 2>/dev/null && python -c "
import sys,json
print('\n'.join(json.load(open(sys.argv[1],encoding='utf-8'))['\$defs']['refusalCode']['enum']))
" "$D/x.json" 2>/dev/null | tr -d '\r'; }

echo "  main=$MAIN  result=$RES"
for half in rust schema; do
  ${half}_ordered "$MAIN" > "$D/mo.txt"; ${half}_ordered "$RES" > "$D/ro.txt"
  if [ ! -s "$D/mo.txt" ] || [ ! -s "$D/ro.txt" ]; then
    echo "  $half: *** EXTRACTION FAILED (empty list) - the comparison proves nothing ***"; RC=1; continue
  fi
  sort "$D/mo.txt" > "$D/m.txt"; sort "$D/ro.txt" > "$D/r.txt"
  LOST=$(comm -23 "$D/m.txt" "$D/r.txt" | tr '\n' ' ' | sed 's/ *$//')
  GAIN=$(comm -13 "$D/m.txt" "$D/r.txt" | tr '\n' ' ' | sed 's/ *$//')
  DUP=$(uniq -d "$D/r.txt" | tr '\n' ' ' | sed 's/ *$//')
  head -n "$(wc -l < "$D/mo.txt")" "$D/ro.txt" > "$D/pfx.txt"
  if diff -q "$D/mo.txt" "$D/pfx.txt" >/dev/null; then PFX="OK (strict prefix, in order)"
  else PFX="*** NOT A PREFIX - reorder or insertion inside main's list ***"; RC=1; fi
  printf "  %-6s  main=%s result=%s\n" "$half" "$(wc -l < "$D/mo.txt")" "$(wc -l < "$D/ro.txt")"
  echo "          LOST from main : ${LOST:-(none)}"; [ -n "$LOST" ] && RC=1
  echo "          GAINED         : ${GAIN:-(none)}"
  echo "          DUPLICATES     : ${DUP:-(none)}";   [ -n "$DUP" ]  && RC=1
  echo "          ORDER (prefix) : $PFX"
  if [ -n "$EXPECTED" ] && [ "$GAIN" != "$EXPECTED" ]; then
    echo "          *** GAINED != expected ($EXPECTED) ***"; RC=1; fi
done
rm -rf "$D"; echo "  verdict: $([ $RC -eq 0 ] && echo PASS || echo FAIL)"; exit $RC
