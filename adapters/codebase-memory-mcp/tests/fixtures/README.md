# Fixture provenance

These fixtures pin the machine-readable result shapes consumed by the pure decoder. They are not
invented GraphHelm wire contracts.

- `search_graph.json` was captured from a live `search_graph(query="ToolCallRecord",
  format="json")` call on 2026-08-28. The result rows were reduced while preserving the provider's
  envelope, keys, column order, and JSON value types.
- `search_graph_grouped.json` was captured from a live
  `search_graph(name_pattern=".*ToolCallRecord.*", format="json")` call on 2026-08-28. It preserves
  the provider's required `total`, `count`, `cols`, `groups`, `qn_prefix`, `file`, `rows`, and
  `has_more` fields.
- `check_index_coverage.json` was captured from a live `check_index_coverage` call on 2026-08-28.
  Its non-empty `coverage` entry follows `coverage_add_row_json` in upstream
  `DeusData/codebase-memory-mcp` commit
  `e65ace1096222580963ebc7ca62d9357269d7485`; quantities and paths were reduced while preserving
  field names and types.

Future fixture changes must name the upstream revision or live capture that changed the shape.
