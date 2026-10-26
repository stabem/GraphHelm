# Sourced by fixture.sh. #1905, #2077: a fixture-played flow runs at its `base`. Every studio2 flow
# whose base is the default fixture Studio (127.0.0.1:5184) is rewritten to name this fixture's
# Studio port, so an isolated fixture (any other port) does not end watch.app_down. Other bases
# (a Runtime, a fixture-only server) are left as written.
#   rebase_flows <journeys dir> <studio port>
rebase_flows() {
  local f
  for f in "$1"/studio2-*.journey.yaml; do
    [ -f "$f" ] || continue
    sed "s#^base: http://127.0.0.1:5184\$#base: http://127.0.0.1:$2#" "$f" > "$f.tmp" && mv "$f.tmp" "$f"
  done
}
