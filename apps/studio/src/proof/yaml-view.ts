/** #1914: a command's stdout that parses as JSON, shown YAML-like: one line per key or array item,
 * indented by depth. Pure data; the view paints it with text nodes only. */
export type ScalarKind = "string" | "number" | "boolean" | "null";

export interface YamlLine {
  indent: number;
  /** The line starts an array item (`- `). */
  bullet: boolean;
  key: string | null;
  scalar: { kind: ScalarKind; text: string } | null;
}

function scalarOf(value: unknown): { kind: ScalarKind; text: string } | null {
  if (value === null) return { kind: "null", text: "null" };
  if (typeof value === "string") return { kind: "string", text: value.includes("\n") ? JSON.stringify(value) : value };
  if (typeof value === "number") return { kind: "number", text: String(value) };
  if (typeof value === "boolean") return { kind: "boolean", text: String(value) };
  return null;
}

function walk(value: unknown, indent: number, key: string | null, bullet: boolean, out: YamlLine[]): void {
  const scalar = scalarOf(value);
  if (scalar !== null) { out.push({ indent, bullet, key, scalar }); return; }
  const isArray = Array.isArray(value);
  const entries: [string | null, unknown][] = isArray ? (value as unknown[]).map((item) => [null, item]) : Object.entries(value as object);
  if (entries.length === 0) { out.push({ indent, bullet, key, scalar: { kind: "null", text: isArray ? "[]" : "{}" } }); return; }
  if (key !== null) {
    out.push({ indent, bullet, key, scalar: null });
    entries.forEach(([entryKey, entry]) => walk(entry, indent + (bullet ? 2 : 1), entryKey, isArray, out));
    return;
  }
  // An object item of an array: its first key shares the `- ` line, the others align under it.
  if (bullet && !isArray) {
    entries.forEach(([entryKey, entry], index) => walk(entry, index === 0 ? indent : indent + 1, entryKey, index === 0, out));
    return;
  }
  entries.forEach(([entryKey, entry]) => walk(entry, bullet ? indent + 1 : indent, entryKey, isArray, out));
}

/** The YAML-like lines of `text` when it is a JSON object or array, else `null` (show it as text). */
export function yamlLines(text: string): YamlLine[] | null {
  const trimmed = text.trim();
  if (!(trimmed.startsWith("{") || trimmed.startsWith("["))) return null;
  let parsed: unknown;
  try { parsed = JSON.parse(trimmed); } catch { return null; }
  if (typeof parsed !== "object" || parsed === null) return null;
  const out: YamlLine[] = [];
  walk(parsed, 0, null, false, out);
  return out;
}

/** The plain text of one line, for tests. */
export function yamlText(line: YamlLine): string {
  const key = line.key !== null ? `${line.key}:` : "";
  const value = line.scalar !== null ? `${line.key !== null ? " " : ""}${line.scalar.text}` : "";
  return `${"  ".repeat(line.indent)}${line.bullet ? "- " : ""}${key}${value}`;
}
