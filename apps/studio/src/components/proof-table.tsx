import type { Mission } from "../runtime/mission";
import { STATUS_LABEL } from "./mission-graph";

export function ProofTable({ mission, onOpenTest }: { mission: Mission; onOpenTest(stepId: string): void }) {
  return (
    <table className="proof">
      <caption>{`Can I trust “${mission.title}”?`}</caption>
      <thead><tr><th>Step</th><th>Promise</th><th>Status</th><th>Your call</th></tr></thead>
      <tbody>
        {mission.steps.map((s) => (
          <tr key={s.stepId} data-status={s.status}>
            <td>{s.index + 1}</td>
            <td>{s.title}</td>
            <td>{STATUS_LABEL[s.status]}{s.reason ? ` · ${s.reason}` : ""}</td>
            <td><button type="button" aria-label={`Open test for step ${s.index + 1}`} onClick={() => onOpenTest(s.stepId)}>Open test</button></td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
