// #1808: gives one studio2 flow a finished journey run in the fixture project, the way the Runtime
// keeps one after a preview: a current run (state.json bound to the flow's digest and commit) and
// one earlier run in history/, each with a web and a mobile frame for every screen. The Proof
// compare, the decision bar and the Graph review's Before read exactly these files.
//
//   node seed_run.mjs <project dir> <flow id> <digest> <commit> [--earlier-differs]
//
// With --earlier-differs (studio2-proof-compare) the earlier run's frames are the same real frames
// with their top-left quarter painted blue, so Changes has exactly 25% of the screen to show.
//
// #2085: the frames are real captured Studio frames (frames/before*.png, studio2-open's bad-ticket
// screen at web and mobile), so a review or compare shows a screen the owner can read. A missing,
// blank or tiny frame stops the seed: a 1x1 placeholder once passed every img check unseen.
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';

const [project, id, digest, commit, option] = process.argv.slice(2);
if (!project || !/^[a-z0-9][a-z0-9._-]{0,127}$/.test(id ?? '') || !/^sha256:[0-9a-f]{64}$/.test(digest ?? '') || !/^[0-9a-f]{40}$/.test(commit ?? '') || ![undefined, '--earlier-differs'].includes(option)) {
  console.error('seed_run: usage: node seed_run.mjs <project dir> <flow id> <digest> <commit> [--earlier-differs]');
  process.exit(2);
}
function frame(name, minWidth, minHeight) {
  const bytes = fs.readFileSync(new URL(`./frames/${name}`, import.meta.url));
  const png = bytes.length > 24 && bytes.toString('latin1', 1, 4) === 'PNG';
  if (!png || bytes.readUInt32BE(16) < minWidth || bytes.readUInt32BE(20) < minHeight || bytes.length < 4096) {
    console.error(`seed_run: frames/${name} is not a real captured frame`);
    process.exit(1);
  }
  return bytes;
}
const WEB = frame('before.png', 1280, 800), MOBILE = frame('before-mobile.png', 390, 844);

function crc32(bytes) {
  let c = ~0;
  for (const b of bytes) { c ^= b; for (let k = 0; k < 8; k += 1) c = (c >>> 1) ^ (0xedb88320 & -(c & 1)); }
  return ~c >>> 0;
}
function chunk(type, data) {
  const out = Buffer.alloc(12 + data.length);
  out.writeUInt32BE(data.length, 0); out.write(type, 4, 'ascii'); data.copy(out, 8);
  out.writeUInt32BE(crc32(out.subarray(4, 8 + data.length)), 8 + data.length);
  return out;
}
/** The frame (8-bit RGB, non-interlaced, as both fixture frames are) with its top-left quarter blue. */
function quarterBlue(png) {
  const width = png.readUInt32BE(16), height = png.readUInt32BE(20);
  if (png[24] !== 8 || png[25] !== 2 || png[28] !== 0) { console.error('seed_run: --earlier-differs needs an 8-bit RGB frame'); process.exit(1); }
  const parts = [];
  for (let at = 8; at < png.length;) {
    const length = png.readUInt32BE(at), type = png.toString('latin1', at + 4, at + 8);
    if (type === 'IDAT') parts.push(png.subarray(at + 8, at + 8 + length));
    at += 12 + length;
  }
  const raw = zlib.inflateSync(Buffer.concat(parts)), stride = width * 3, out = Buffer.alloc(height * (stride + 1));
  let prior = Buffer.alloc(stride);
  for (let y = 0; y < height; y += 1) {
    const filter = raw[y * (stride + 1)], line = Buffer.from(raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1)));
    for (let x = 0; x < stride; x += 1) {
      const a = x >= 3 ? line[x - 3] : 0, b = prior[x], c = x >= 3 ? prior[x - 3] : 0;
      const p = a + b - c, pa = Math.abs(p - a), pb = Math.abs(p - b), pc = Math.abs(p - c);
      const add = filter === 1 ? a : filter === 2 ? b : filter === 3 ? (a + b) >> 1 : filter === 4 ? (pa <= pb && pa <= pc ? a : pb <= pc ? b : c) : 0;
      line[x] = (line[x] + add) & 255;
    }
    prior = line;
    const painted = Buffer.from(line);
    if (y < height / 2) for (let x = 0; x < width / 2; x += 1) { painted[x * 3] = 0; painted[x * 3 + 1] = 0; painted[x * 3 + 2] = 255; }
    painted.copy(out, y * (stride + 1) + 1);
  }
  return Buffer.concat([png.subarray(0, 8), chunk('IHDR', png.subarray(16, 29)), chunk('IDAT', zlib.deflateSync(out)), chunk('IEND', Buffer.alloc(0))]);
}
const differs = option === '--earlier-differs';
const EARLIER = differs ? { web: quarterBlue(WEB), mobile: quarterBlue(MOBILE) } : { web: WEB, mobile: MOBILE };
const flow = fs.readFileSync(path.join(project, '.graphhelm/journeys', `${id}.journey.yaml`), 'utf8').replace(/\r\n/g, '\n');
const block = flow.slice(flow.indexOf('\nscreens:\n'), flow.indexOf('\nedges:\n'));
const screens = [...block.matchAll(/^  - id: ([A-Za-z0-9][A-Za-z0-9._-]*)$/gm)].map((m) => m[1]);
if (screens.length === 0) { console.error(`seed_run: ${id} names no screen`); process.exit(1); }

function run(dir, startedAt, current) {
  fs.mkdirSync(path.join(dir, 'frames', 'mobile'), { recursive: true });
  const view = (width, height) => Object.fromEntries(screens.map((s) => [s, { result: 'pass', frame: true, file: `${s}.png`, width, height }]));
  for (const s of screens) {
    fs.writeFileSync(path.join(dir, 'frames', `${s}.png`), current ? WEB : EARLIER.web);
    fs.writeFileSync(path.join(dir, 'frames', 'mobile', `${s}.png`), current ? MOBILE : EARLIER.mobile);
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
