A merge inserted `repository_scope` between `addressable_scope`'s doc block and
`addressable_scope` in `apps/cli/src/commands/execution/mod.rs`, and `assign_schema` between
`synthesize_schema`'s doc block and `synthesize_schema` in `apps/cli/src/commands/mcp/tools.rs`.
rustdoc now attaches those docs to the wrong items. Move each displaced doc block back directly above
its own function. Change comments only: no code, no reformatting, no other files.
