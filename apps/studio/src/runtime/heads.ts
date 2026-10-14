/** #1275: two recorded heads are one commit when one is a prefix of the other and both are at
 * least 7 hex: a `pr_opened` spelled with 8 characters and a verdict with 40 name the same head.
 * The Runtime's task fold uses the same rule (`same_head` in `core/execution/src/tasks.rs`, #1262);
 * every view that compares heads goes through here so the views cannot disagree with the fold. */
export function sameHead(a: string | null | undefined, b: string | null | undefined): boolean {
  if (!a || !b) return false;
  const x = a.toLowerCase();
  const y = b.toLowerCase();
  if (x === y) return true;
  return x.length >= 7 && y.length >= 7 && (x.startsWith(y) || y.startsWith(x));
}
