import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const driver = fileURLToPath(new URL('./driver.mjs', import.meta.url));
const protocol = 'graphhelm-journey-driver/1';
// Contract: closed bounded frames reject invalid input before loading a browser.
// Defect/gap: no phase-1 observer consumes a driver protocol. Real child I/O, no seams;
// cost: <3 seconds, Node only, no Playwright, credentials, or network.
test('closed protocol refuses malformed, secret, sequence and oversized frames', async () => {
  const cases = [
    [{ protocol, requestId: 1, op: 'unknown' }, 'driver.protocol_invalid'],
    [{ protocol, requestId: 2, op: 'close' }, 'driver.protocol_invalid'],
    [{ protocol: 'old', requestId: 1, op: 'close' }, 'driver.protocol_invalid'],
    [{ protocol, requestId: 1, op: 'close', surprise: true }, 'driver.protocol_invalid'],
    [{ protocol, requestId: 1, op: 'act', kind: 'enter_text', role: 'textbox', name: 'Password', text: 'canary', secretEnv: 'GRAPHHELM_SECRET_PASSWORD' }, 'driver.protocol_invalid'],
    [{ protocol, requestId: 1, op: 'act', kind: 'activate', role: 'button', name: 'Save', text: 'canary' }, 'driver.protocol_invalid'],
    [{ protocol, requestId: 1, op: 'act', kind: 'enter_text', role: 'textbox', name: 'Password', text: 'canary' }, 'driver.secret_literal'],
    [{ protocol, requestId: 1, op: 'act', kind: 'enter_text', role: 'textbox', name: 'Password', secretEnv: 'GRAPHHELM_SECRET_MISSING' }, 'driver.secret_missing'],
    [{ protocol, requestId: 1, op: 'act', kind: 'select', role: 'combobox', name: 'Country' }, 'driver.unsupported_act'],
    [{ protocol, requestId: 1, op: 'open', base: 'http://localhost.evil.test.example/cart', viewport: {width:1280,height:720}, allowOrigins: [] }, 'driver.host_refused'],
    [{ protocol, requestId: 1, op: 'open', base: 'http://user:password@localhost/cart', viewport: {width:1280,height:720}, allowOrigins: [] }, 'driver.host_refused'],
    // #398: `headed` is a boolean open field; true passes validation and reaches the browser
    // load (no Playwright in this cwd), a non-boolean is refused before it.
    [{ protocol, requestId: 1, op: 'open', base: 'http://localhost/cart', viewport: {width:1280,height:720}, allowOrigins: [], headed: true }, 'driver.observer_missing'],
    [{ protocol, requestId: 1, op: 'open', base: 'http://localhost/cart', viewport: {width:1280,height:720}, allowOrigins: [], headed: 'yes' }, 'driver.protocol_invalid'],
    // #356: `survive` (a healing replay keeps its session at the broken edge) is boolean too.
    [{ protocol, requestId: 1, op: 'open', base: 'http://localhost/cart', viewport: {width:1280,height:720}, allowOrigins: [], survive: true }, 'driver.observer_missing'],
    [{ protocol, requestId: 1, op: 'open', base: 'http://localhost/cart', viewport: {width:1280,height:720}, allowOrigins: [], survive: 1 }, 'driver.protocol_invalid'],
    ['x'.repeat(65537), 'driver.frame_too_large'],
    ['{"protocol":', 'driver.protocol_invalid'],
  ];
  for (const [request, code] of cases) {
    const child = spawn(process.execPath, [driver, '--project', process.cwd(), '--output-dir', process.cwd()], {
      env: { PATH: process.env.PATH, SystemRoot: process.env.SystemRoot, GRAPHHELM_SECRET_PASSWORD: 'canary' },
      stdio: ['pipe', 'pipe', 'pipe'],
    });
    let out = '', err = '';
    child.stdout.on('data', b => { out += b; });
    child.stderr.on('data', b => { err += b; });
    const completion = new Promise((resolve, reject) => { child.on('error', reject); child.on('exit', resolve); });
    child.stdin.end(typeof request === 'string' ? request : JSON.stringify(request) + '\n');
    const timer = setTimeout(() => child.kill(), 3000);
    await completion;
    clearTimeout(timer);
    assert.equal(err, '', 'no exception/input leaks on stderr');
    const reply = JSON.parse(out.trim());
    assert.equal(reply.ok, false);
    assert.equal(reply.code, code);
    assert.equal(reply.protocol, protocol);
    assert.ok(!out.includes('canary'), 'secret never serialized');
  }
});
