// #1808: gives one studio2 flow a finished journey run in the fixture project, the way the Runtime
// keeps one after a preview: a current run (state.json bound to the flow's digest and commit) and
// one earlier run in history/, each with a web and a mobile frame for every screen. The Proof
// compare, the decision bar and the Graph review's Before read exactly these files.
//
//   node seed_run.mjs <project dir> <flow id> <digest> <commit>
//
// The frames are a fixed 1x1 PNG: the flows judge the controls around a frame, not its pixels.
import fs from 'node:fs';
import path from 'node:path';

const [project, id, digest, commit] = process.argv.slice(2);
if (!project || !/^[a-z0-9][a-z0-9._-]{0,127}$/.test(id ?? '') || !/^sha256:[0-9a-f]{64}$/.test(digest ?? '') || !/^[0-9a-f]{40}$/.test(commit ?? '')) {
  console.error('seed_run: usage: node seed_run.mjs <project dir> <flow id> <digest> <commit>');
  process.exit(2);
}
const PNG = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==', 'base64');
const flow = fs.readFileSync(path.join(project, '.graphhelm/journeys', `${id}.journey.yaml`), 'utf8').replace(/\r\n/g, '\n');
const block = flow.slice(flow.indexOf('\nscreens:\n'), flow.indexOf('\nedges:\n'));
const screens = [...block.matchAll(/^  - id: ([A-Za-z0-9][A-Za-z0-9._-]*)$/gm)].map((m) => m[1]);
if (screens.length === 0) { console.error(`seed_run: ${id} names no screen`); process.exit(1); }

function run(dir, startedAt, current) {
  fs.mkdirSync(path.join(dir, 'frames', 'mobile'), { recursive: true });
  const view = (width, height) => Object.fromEntries(screens.map((s) => [s, { result: 'pass', frame: true, file: `${s}.png`, width, height }]));
  for (const s of screens) {
    fs.writeFileSync(path.join(dir, 'frames', `${s}.png`), PNG);
    fs.writeFileSync(path.join(dir, 'frames', 'mobile', `${s}.png`), PNG);
  }
  const ranAt = new Date(Date.parse(startedAt) + 30000).toISOString();
  const state = {
    commit: current ? commit : commit, current: null, digest, edges: {}, kind: 'preview', preview: true, recorded: false,
    ranAt, result: 'pass', screens: view(1280, 800), startedAt, state: 'ready', viewport: 'mobile',
    viewports: { web: { edges: {}, result: 'pass', screens: view(1280, 800) }, mobile: { edges: {}, result: 'pass', screens: view(390, 844) } },
    viewportsPlanned: ['web', 'mobile'],
  };
  fs.writeFileSync(path.join(dir, 'state.json'), `${JSON.stringify(state, null, 2)}\n`);
}

const root = path.join(project, '.graphhelm/journey-previews', id);
const now = Date.now();
const earlier = new Date(now - 3600000).toISOString();
run(path.join(root, 'history', String(Date.parse(earlier)).padStart(20, '0')), earlier, false);
run(root, new Date(now - 60000).toISOString(), true);
console.log(screens.join(' '));
