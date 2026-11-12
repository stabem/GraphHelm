import { journeyName, type JourneyFlow } from "../runtime/client";

/** #2845: the longest query the Proof column's search field takes. */
export const JOURNEY_SEARCH_MAX = 80;

/** The group a journey is listed under in the Proof column. */
function groupName(flow: JourneyFlow): string {
  return flow.feature || "Other";
}

/** The Proof column's groups, in the order their first journey appears. */
export function groupJourneys(journeys: readonly JourneyFlow[]): [string, JourneyFlow[]][] {
  const groups = new Map<string, JourneyFlow[]>();
  for (const flow of journeys) {
    const group = groupName(flow);
    groups.set(group, [...(groups.get(group) ?? []), flow]);
  }
  return [...groups];
}

/** #2845: the journeys the search shows. A journey is shown when the trimmed, lower-cased query is a
 * plain substring of its display name, its id or its group's name; an empty query shows them all. No
 * pattern or fuzzy matching: the query is text. */
export function filterJourneys(journeys: readonly JourneyFlow[], query: string): readonly JourneyFlow[] {
  const needle = query.trim().toLowerCase();
  if (needle === "") return journeys;
  return journeys.filter((flow) => [journeyName(flow), flow.id, groupName(flow)].some((field) => field.toLowerCase().includes(needle)));
}
