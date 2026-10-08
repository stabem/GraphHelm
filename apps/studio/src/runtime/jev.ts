/** #396 (journey-first spec §8): a Runtime with no judge route is a state, not an error. The chat
 * renders no Jev card for it; run details carries `JEV_ABSENT_NOTE` instead. */
export const JEV_ABSENT_NOTE = "No Jev model: suggested replies are off (Models → Add model)";

/** `judgeRouteCount` is null until the Runtime's routes have been read. */
export function jevAvailability(judgeRouteCount: number | null): "checking" | "absent" | "configured" {
  if (judgeRouteCount === null) return "checking";
  return judgeRouteCount === 0 ? "absent" : "configured";
}
