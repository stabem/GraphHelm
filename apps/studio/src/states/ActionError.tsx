import { useId } from "react";

import { RuntimeError } from "../runtime/client";

export interface ActionErrorDetails {
  title: string;
  code: string | null;
  cause: string;
  nextStep: string;
  command?: string;
}

/** Keep Runtime codes when supplied, and use caller-owned text for failures without safe detail. */
export function actionErrorDetails(title: string, failure: unknown, cause: string, nextStep: string, command?: string): ActionErrorDetails {
  const runtimeFailure = failure instanceof RuntimeError ? failure : null;
  return {
    title,
    code: runtimeFailure?.code ?? null,
    cause: runtimeFailure?.message ?? cause,
    nextStep,
    ...(command === undefined ? {} : { command }),
  };
}

export function ActionError({ error, onDismiss, onRetry }: {
  error: ActionErrorDetails;
  onDismiss: () => void;
  onRetry?: () => void;
}) {
  const titleId = useId();
  return (
    <div role="alert" aria-labelledby={titleId} className="action-error">
      <div className="action-error-head">
        <strong id={titleId} className="action-error-title">{error.title}</strong>
        <button type="button" className="b ghost action-error-dismiss" onClick={onDismiss}>Dismiss</button>
      </div>
      <span className="mono action-error-code">{error.code ?? "Code not supplied"}</span>
      <p className="action-error-cause">{error.cause}</p>
      <p className="action-error-next">{error.nextStep}{error.command !== undefined && <> <code>{error.command}</code></>}</p>
      {onRetry !== undefined && <button type="button" className="b ghost action-error-retry" onClick={onRetry}>Retry</button>}
    </div>
  );
}
