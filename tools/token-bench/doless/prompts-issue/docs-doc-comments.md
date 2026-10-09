**rustdoc shows the wrong docs on two CLI functions**

In the CLI crate, `repository_scope` (in `commands/execution/mod.rs`) is documented with the text
meant for `addressable_scope`, and `assign_schema` (in `commands/mcp/tools.rs`) carries the
`synthesize_schema` explanation. Looks like a merge put the functions between the doc comments and
the items they belong to. Expected: every doc comment documents its own function again.
