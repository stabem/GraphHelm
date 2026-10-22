/**
 * #1775: the Run bar while Run provisions the browser observer (#1763). The Runtime reports the
 * command it is running ("npx playwright install chromium") or a phase line; the owner reads the
 * phase in plain words instead, plus a percentage only when the installer printed one.
 */
const PHASES: ReadonlyArray<readonly [RegExp, string]> = [
  [/playwright install|chromium/i, "Downloading Chromium"],
  [/npm install|@playwright\/test/i, "Installing test tools"],
  [/writing the current browser observer|driver/i, "Writing the driver"],
];

export function observerSetupStatus(message: string): string {
  const phase = PHASES.find(([pattern]) => pattern.test(message))?.[1];
  const percent = /\b(\d{1,3})\s?%/.exec(message)?.[1];
  const shown = percent !== undefined && Number(percent) <= 100 ? ` ${Number(percent)}%` : "";
  return phase ? `Preparing the browser · ${phase}${shown}` : `Preparing the browser${shown}`;
}
