/** #2341: what the Graph says when no request has work to draw. A run that waits for its owner adds
 * that it waits, so an empty graph is not read as "nothing to do". Only the single run: the
 * project-wide text (many runs) keeps its words, since one run's wait is not the project's. */
export function noRequestsText(many: boolean, runs: number, requests: number, waiting: boolean): string {
  if (requests !== 0) return "No request has work in this journey.";
  if (many) return `No owner request is recorded in the ${runs} runs of this project yet.`;
  const text = "No owner request is recorded on this run yet.";
  return waiting ? `${text} The run waits for you; see Status above.` : text;
}
