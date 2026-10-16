/** Two Runtime answers with the same content (#503): poll replies are fresh objects every tick. */
export function sameValue(left: unknown, right: unknown): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}
