Keel's proportionality table puts "config values" on the lightest row (nothing beyond the summary),
but a one-line change to a Runtime timeout or a permission setting changes behaviour. In the table in
`docs/process/DELIVERY.md` section 2 and the matching table in the Keel section of `AGENTS.md`:
limit the light row to *inert* config values and say that runtime- or security-affecting config
needs a full card and behavioural evidence; add runtime-affecting config to the expanded
(persistence, permissions, ...) row. Docs only: no checker, no `keel.yaml` rule.
