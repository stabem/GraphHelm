# M08 re-judge run 6 — transcript, not a store

Transcript, not a store: `journal.jsonl` here is an exported event log — no `format.json`, no
`blobs/` — so it cannot be opened or replayed, and the evidence its batches reference (tool
records, stdout/stderr, the judge's judgment) was never committed.

Its content is intact and checksummed in `SHA256SUMS`: the batch checksums, the event hash chain
and the request digests all verify. What is absent is the store shape and the evidence blobs, and
neither can be reconstructed from what is here — unlike the byte-archives below, no `mkdir`
repairs this one.

`read-audit.jsonl` beside it is a different kind of record: the bytes the surface actually served
during the run. It is a flat log, not a store either, and it is the source `docs/acceptance/m08-judge-coverage.md`
measures.

For a committed store that does open, see `docs/acceptance/m05-run-2026-08-16/` and
`docs/acceptance/m06-run-2026-08-17/` — byte-archives of real stores; restore the empty
directories first, per their `README.md`.
