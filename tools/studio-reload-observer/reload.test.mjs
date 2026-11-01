// Unit tests for the receipt arithmetic and classification of the board reload observer (#1703).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { percentile, summarize, classify, routeOf, parseBuilds, countInvocations, parseArgs, psToLines, snapshotFrom } from './reload.mjs';

test('nearest-rank percentiles over 20 samples', () => {
  const v = Array.from({ length: 20 }, (_, i) => (i + 1) * 100); // 100..2000, shuffled order must not matter
  const shuffled = [...v].reverse();
  assert.equal(percentile(shuffled, 50), 1000);
  assert.equal(percentile(shuffled, 95), 1900);
  assert.equal(percentile([], 50), null);
});

test('stats use success samples only and every outcome is counted', () => {
  const attempts = [
    { outcome: 'success', usableMs: 100 }, { outcome: 'success', usableMs: 300 },
    { outcome: 'timeout', usableMs: null }, { outcome: 'error', usableMs: null }, { outcome: 'excluded', usableMs: null },
  ];
  const s = summarize(attempts);
  assert.deepEqual(s.counts, { success: 2, timeout: 1, error: 1, excluded: 1 });
  assert.equal(s.attempts, 5);
  assert.equal(s.validSamples, 2);
  assert.equal(s.maxMs, 300);
  assert.equal(s.p95Ms, 300);
  assert.equal(summarize([{ outcome: 'timeout', usableMs: null }]).p50Ms, null);
});

test('classification precedence: excluded, thrown, runtime >= 400, read failure, timeout', () => {
  const base = { threw: null, ready: true, readFailure: false, badResponses: [], excludedReason: null };
  assert.equal(classify(base).outcome, 'success');
  assert.equal(classify({ ...base, ready: false }).outcome, 'timeout');
  assert.equal(classify({ ...base, readFailure: true }).outcome, 'error');
  assert.equal(classify({ ...base, badResponses: [{ status: 503, route: '/v1/executions' }] }).outcome, 'error');
  assert.equal(classify({ ...base, threw: 'TimeoutError' }).outcome, 'error');
  assert.equal(classify({ ...base, threw: 'x', excludedReason: 'browser crashed' }).outcome, 'excluded');
});

test('a route never carries its query (where the session nonce rides)', () => {
  assert.equal(routeOf('http://127.0.0.1:5191/v1/executions/demo/tasks?session=secret-nonce'), '/v1/executions/demo/tasks');
});

test('build snapshot keeps crate and subcommand names, never paths', () => {
  const win = (...parts) => parts.join(String.fromCharCode(92)); // Windows paths without escapes
  const b = parseBuilds([
    `rustc.exe\t${win('C:', 'Users', 'x', '.rustup', 'rustc.exe')} --crate-name graphhelm_cli --edition=2024 ${win('D:', 'gh', 'w', 'src', 'main.rs')}`,
    `cargo.exe\t"${win('C:', 'Users', 'x', '.cargo', 'bin', 'cargo.exe')}" +1.97.1 build --locked -p graphhelm-cli`,
    'rustc.exe\trustc -vV',
  ]);
  assert.deepEqual(b, [{ tool: 'cargo', what: 'build' }, { tool: 'rustc', what: 'graphhelm_cli' }]);
  assert.ok(!JSON.stringify(b).includes('Users'));
});

test('the live Studio and Runtime ports are refused', () => {
  const ok = ['--runtime', 'http://127.0.0.1:8811', '--session', 's', '--token', 't', '--condition', 'idle', '--playwright', '.', '--out', 'o.json'];
  assert.throws(() => parseArgs(['--studio', 'http://127.0.0.1:5183', ...ok]), /live/);
  assert.throws(() => parseArgs(['--studio', 'http://127.0.0.1:5191', ...ok.map((x) => x.replace('8811', '8793'))]), /live/);
  assert.throws(() => parseArgs(['--studio', 'http://example.com:5191', ...ok]), /loopback/);
  assert.equal(parseArgs(['--studio', 'http://127.0.0.1:5191', ...ok]).attempts, 20);
});

test('a rustup-proxied cargo run is one build invocation, not two', () => {
  const win = (...parts) => parts.join(String.fromCharCode(92));
  const proxy = (pid) => `cargo.exe\t${pid}\t1\t"${win('C:', 'x', '.cargo', 'bin', 'cargo.exe')}" +1.97.1 build --locked -p a`;
  const real = (pid, ppid) => `cargo.exe\t${pid}\t${ppid}\t"${win('C:', 'x', 'bin', 'cargo.exe')}" build --locked -p a`;
  // two builds are four cargo processes, which a process count would have called "four builds"
  assert.equal(countInvocations([proxy(10), real(11, 10), proxy(20), real(21, 20), 'rustc.exe\t30\t11\trustc --crate-name a']), 2);
  assert.equal(countInvocations([proxy(10), real(11, 10), proxy(20), real(21, 20), proxy(30), real(31, 30), proxy(40), real(41, 40)]), 4);
  assert.equal(countInvocations([]), 0);
  assert.equal(countInvocations(['rustc.exe\t1\t2\trustc -vV']), 0);
});

test('a Linux ps listing of two proxied builds is two invocations, with names and no paths (#2402)', () => {
  // `ps -eo comm=,pid=,ppid=,args=` as Linux prints it: padded columns, a rustup proxy cargo (pid 100, 200)
  // with its real cargo child (101, 201), rustc under each, and unrelated processes that must be ignored.
  const ps = [
    '    bash  50     1 -bash',
    '   cargo 100    50 cargo +1.97.1 build --locked -p graphhelm-cli',
    '   cargo 101   100 /home/u/.rustup/toolchains/1.97.1-x86_64-unknown-linux-gnu/bin/cargo build --locked -p graphhelm-cli',
    '   rustc 102   101 /home/u/.rustup/toolchains/1.97.1-x86_64-unknown-linux-gnu/bin/rustc --crate-name graphhelm_cli --edition=2024 apps/cli/src/main.rs',
    '   cargo 200    50 cargo +1.97.1 test --locked -p tool-host',
    '   cargo 201   200 /home/u/.rustup/toolchains/1.97.1-x86_64-unknown-linux-gnu/bin/cargo test --locked -p tool-host',
    '   rustc 202   201 /home/u/.rustup/toolchains/1.97.1-x86_64-unknown-linux-gnu/bin/rustc --crate-name tool_host --edition=2024 src/lib.rs',
    '   rustc 300     1 rustc -vV',
    '',
  ].join('\n');
  const lines = psToLines(ps);
  assert.equal(lines.split('\n').length, 8, 'every listed process becomes a four-field line');
  const snap = snapshotFrom(lines);
  assert.equal(snap.invocations, 2, 'four cargo processes are two builds');
  assert.deepEqual(snap.builds, [
    { tool: 'cargo', what: 'build' }, { tool: 'cargo', what: 'build' }, { tool: 'cargo', what: 'test' }, { tool: 'cargo', what: 'test' },
    { tool: 'rustc', what: 'graphhelm_cli' }, { tool: 'rustc', what: 'tool_host' },
  ]);
  assert.ok(!JSON.stringify(snap).includes('/home/'), 'no path reaches the snapshot');
  // an empty listing and a listing with no build are zero, not an error
  assert.equal(snapshotFrom(psToLines('')).invocations, 0);
  assert.equal(snapshotFrom(psToLines('    bash  50     1 -bash\n')).invocations, 0);
});
