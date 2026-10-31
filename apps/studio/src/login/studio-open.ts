/** `graphhelm studio open`'s default `--port` (apps/cli/src/args.rs, `StudioOpenArgs::port`). The
 * one place the Studio names it. */
export const STUDIO_OPEN_DEFAULT_PORT = 5183;

/** The port this page is served on; `null` outside a browser or on the scheme's default port. */
function pagePort(): string | null {
  return typeof location === "undefined" || location.port === "" ? null : location.port;
}

/** The `graphhelm studio open` command this page can hand the user. Off the CLI's default port it
 * names this page's port, or the CLI would refuse it (GHCLI032); on the default port it stays as is.
 * `extra` are the arguments after it, e.g. `--project`. */
export function studioOpenCommand(extra = "", port: string | null = pagePort()): string {
  const onDefault = port === null || Number(port) === STUDIO_OPEN_DEFAULT_PORT;
  return ["graphhelm studio open", onDefault ? "" : `--port ${port}`, extra].filter(Boolean).join(" ");
}
