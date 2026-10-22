// #1808: the lanes studio2-lanes-needs-action acts on, one Needs action item of each kind.
//
//   node seed_needs.mjs silent <fixture dir> <runtime url> <token file>
//   node seed_needs.mjs fresh  <fixture dir> <runtime url> <token file>
//
// `silent` records two lanes that will owe work and say nothing: `lane-quiet` holds a slice
// (Reassign) and `lane-gone` only reviews one (Archive). A lane turns silent 30 minutes after its
// last record by the Runtime's own clock, which no record can backdate, so `fresh` first waits
// until that much time has passed since `silent`, then records what must stay recent: a PR with no
// reviewer (Assign), an approved PR (Merge), and `lane-free`, a live lane that owes nothing and so
// can take reassigned work. Every record goes through the Runtime's public signal route, signed by
// the lane it names; a refusal stops the fixture.
import fs from 'node:fs';
import { randomUUID } from 'node:crypto';

const [phase, dir, runtimeUrl, tokenFile] = process.argv.slice(2);
const runtime = new URL(runtimeUrl ?? '');
if (!['silent', 'fresh'].includes(phase) || !dir || runtime.protocol !== 'http:' || runtime.hostname !== '127.0.0.1' || !runtime.port || runtime.port === '8793') {
  console.error('seed_needs: usage: node seed_needs.mjs silent|fresh <fixture dir> <loopback runtime url> <token file>');
  process.exit(2);
}
const token = fs.readFileSync(tokenFile, 'utf8').trim();
const stamp = `${dir}/.graphhelm/needs-silent.at`;
const SILENT_MS = 30 * 60 * 1000 + 30 * 1000;

async function record(actor, type, fields) {
  const at = new Date().toISOString().replace(/\.\d+Z$/, 'Z');
  const id = `fixture-needs-${randomUUID()}`;
  const signal = { id, type, source: { type: 'user', id: actor }, severity: 'low', emittedAt: at, evidence: ['demo'],
    description: JSON.stringify({ schema: 'graphhelm-task-event-v1', revision: 1, at, ...fields }) };
  const response = await fetch(new URL('/v1/executions/demo/signal', runtime), {
    method: 'POST', headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json',
      'Idempotency-Key': id, 'X-GraphHelm-Actor': actor, 'X-GraphHelm-Actor-Type': 'agent' },
    body: JSON.stringify({ signal }), signal: AbortSignal.timeout(10000),
  });
  const result = await response.json();
  if (!response.ok || result.ok !== true) throw new Error(`seed_needs: ${type} by ${actor} refused`);
}

const claim = (lane, issue, title) => record(lane, 'task.claimed', { taskId: `issue-${issue}-needs`, issue, lane, branch: `issue-${issue}-needs-fixture`, journeys: [], title });
const pr = (lane, issue, number) => record(lane, 'task.pr_opened', { taskId: `issue-${issue}-needs`, pr: number, lane, headSha: `${number}abcde`, journeys: [], title: `Fixture PR ${number}` });

if (phase === 'silent') {
  await claim('lane-quiet', 803, 'Quiet lane holds this slice');
  await claim('lane-author', 804, 'Reviewed by a lane that went quiet');
  await pr('lane-author', 804, 804);
  await record('lane-author', 'task.review_assigned', { taskId: 'issue-804-needs', pr: 804, headSha: '804abcde', reviewer: 'lane-gone' });
  fs.writeFileSync(stamp, String(Date.now()));
} else {
  const since = Number(fs.readFileSync(stamp, 'utf8'));
  const wait = since + SILENT_MS - Date.now();
  if (wait > 0) {
    console.error(`seed_needs: waiting ${Math.ceil(wait / 60000)} min for the quiet lanes to turn silent`);
    await new Promise((done) => setTimeout(done, wait));
  }
  // lane-author's new claims below are its newest records, so it never reads silent itself.
  await claim('lane-author', 801, 'PR with no reviewer');
  await pr('lane-author', 801, 801);
  await claim('lane-author', 802, 'Approved PR');
  await pr('lane-author', 802, 802);
  await record('lane-review', 'task.review_verdict', { taskId: 'issue-802-needs', pr: 802, headSha: '802abcde', reviewer: 'lane-review', verdict: 'APPROVE', commentUrl: 'https://github.com/stabem/GraphHelm/pull/802#review' });
  await claim('lane-author', 805, 'Merged by the free lane');
  await pr('lane-author', 805, 805);
  await record('lane-free', 'task.merged', { taskId: 'issue-805-needs', pr: 805, mergeSha: '805fedcb', closes: [], merger: 'lane-free' });
}
