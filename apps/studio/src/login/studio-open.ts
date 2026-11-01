/** `graphhelm studio open`'s default `--port` (apps/cli/src/args.rs, `StudioOpenArgs::port`). The
 * one place the Studio names it. */
export const STUDIO_OPEN_DEFAULT_PORT = 5183;

/** The `<meta>` the dev server writes into the page: the port the Studio SERVER listens on. */
export const STUDIO_PORT_META = "graphhelm-studio-port";

/** The port the Studio server listens on, as the server told this page; `null` when it did not.
 * Never the address bar's port: behind a published container port, a tunnel or a proxy that is the
 * browser's port, which `graphhelm studio open` refuses (GHCLI032). */
function pagePort(): string | null {
  // Through globalThis: this file is also compiled with the dev server's config, which has no DOM.
  const document = (globalThis as { document?: { querySelector(selector: string): { getAttribute(name: string): string | null } | null } }).document;
  const port = document?.querySelector(`meta[name="${STUDIO_PORT_META}"]`)?.getAttribute("content");
  return port === undefined || port === null || port === "" ? null : port;
}

/** The `graphhelm studio open` command this page can hand the user. Off the CLI's default port it
 * names the server's port, or the CLI would refuse it (GHCLI032); on the default port it stays as is.
 * `extra` are the arguments after it, e.g. `--project`. */
export function studioOpenCommand(extra = "", port: string | null = pagePort()): string {
  const onDefault = port === null || Number(port) === STUDIO_OPEN_DEFAULT_PORT;
  return ["graphhelm studio open", onDefault ? "" : `--port ${port}`, extra].filter(Boolean).join(" ");
}
