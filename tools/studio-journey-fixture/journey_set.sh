#!/usr/bin/env bash
# The journeys the fixture Studio lists (#2863): the ids in journeys.list, no more. fixture.sh used to
# copy every flow in .graphhelm/journeys, so the Proof view's total moved whenever a journey was
# added to or removed from the repository, and flows that pin that total went red for it.
#
#   journey_set.sh check <journeys dir> <list file>
#   journey_set.sh copy  <journeys dir> <list file> <destination dir>
#
# A listed id without a `<id>.journey.yaml` is an error (exit 3) that names the id; the list never
# shrinks silently. `copy` takes every file of a listed id (`<id>.*`: the flow and its generated
# contracts) and nothing else.
set -euo pipefail
mode=${1:-}; src=${2:-}; list=${3:-}; dest=${4:-}
[ -d "$src" ] && [ -f "$list" ] || { echo "fixture: journey_set.sh needs a journeys directory and a list file" >&2; exit 3; }
ids=$(sed -e 's/#.*//' -e 's/[[:space:]]*$//' -e '/^[[:space:]]*$/d' "$list")
seen=" "; bad=0
for id in $ids; do
  case "$id" in
    *[!a-z0-9-]*|-*) echo "fixture: journeys.list holds '$id', which is not a journey id" >&2; bad=1; continue ;;
  esac
  case "$seen" in *" $id "*) echo "fixture: journeys.list names '$id' twice" >&2; bad=1; continue ;; esac
  seen="$seen$id "
  [ -f "$src/$id.journey.yaml" ] || { echo "fixture: journeys.list names the journey '$id' but .graphhelm/journeys/$id.journey.yaml does not exist; restore the flow or remove the id from journeys.list" >&2; bad=1; }
done
[ "$bad" = 0 ] || exit 3
case "$mode" in
  check) ;;
  copy)
    [ -n "$dest" ] || { echo "fixture: journey_set.sh copy needs a destination" >&2; exit 3; }
    mkdir -p "$dest"
    for id in $ids; do cp "$src/$id".* "$dest/"; done ;;
  *) echo "usage: journey_set.sh check|copy <journeys dir> <list file> [destination]" >&2; exit 3 ;;
esac
