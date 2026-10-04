// Protects the real framed host boundary from false receipts, duplicate sends and setting overrides.
// Existing standalone RPC tests cannot exercise the Desktop protocol. Cost: local mocked I/O and
// one Node subprocess, about two seconds; no Desktop, provider, credentials or network required.
import test from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { run } from '../src/desktop_chats.mjs';

const ids = {
  threadId: '019fdfe7-b5fa-7ca1-89c8-9651ad856819',
  callerThreadId: '01a0fc64-953e-7cc2-a27f-53a91c425a15',
  callerTurnId: '01a106fc-8eb2-7ca3-9029-ec2799b09acb',
  requestId: 'request-01_test',
};
const input = { ...ids, message: 'continue the bounded task', sourceDirectory: process.cwd(), pipePath: '\\\\.\\pipe\\codex-test' };
const expectedPrompt = 'continue the bounded task\n\n[GraphHelm request: request-01_test]';
const oldId = '019fdfe7-b5fa-7ca1-89c8-9651ad856820';
const newId = '019fdfe7-b5fa-7ca1-89c8-9651ad856821';
const payload = (value) => ({ success: true, contentItems: [{ type: 'inputText', text: JSON.stringify(value) }] });
const metadata = () => ({ id: ids.threadId, kind: 'codex', hostId: 'local', cwd: process.cwd(), title: 'target', status: { type: 'idle' } });
function turn(id, status, prompt = expectedPrompt) {
  return {
    id, status,
    items: [
      { type: 'functionCallOutput', namespace: 'codex_app', name: 'send_message_to_thread', output: {
        text: `<codex_delegation>\n  <source_thread_id>${ids.callerThreadId}</source_thread_id>\n  <input>${prompt}</input>\n</codex_delegation>`, truncated: false,
      } },
      ...(status === 'completed' ? [{ type: 'agentMessage', phase: 'final_answer', text: 'done' }] : []),
    ],
  };
}
class HostSocket extends EventEmitter {
  constructor(mode = 'normal') { super(); this.mode = mode; this.reads = 0; this.sends = []; this.requests = []; }
  write(bytes) {
    const request = JSON.parse(bytes.subarray(4).toString());
    this.requests.push(request);
    setImmediate(() => {
      let result;
      if (request.method === 'tools/list') {
        result = { tools: this.mode === 'missing' ? [] : [
          { namespace: 'codex_app', name: 'read_thread' }, { namespace: 'codex_app', name: 'send_message_to_thread' },
        ] };
      } else if (request.params.tool === 'send_message_to_thread') {
        this.sends.push(request.params.arguments);
        if (this.mode === 'ambiguous') {
          const malformed = Buffer.from('{not JSON'); const frame = Buffer.alloc(4 + malformed.length);
          frame.writeUInt32LE(malformed.length); malformed.copy(frame, 4); this.emit('data', frame); return;
        }
        result = payload({ threadId: ids.threadId });
      } else {
        this.reads += 1;
        if (this.mode === 'history' && request.params.arguments.turnLimit !== 1) {
          const oversized = Buffer.alloc(8 * 1024 * 1024 + 1);
          oversized.writeUInt32LE(8 * 1024 * 1024 + 1, 0);
          this.emit('data', oversized);
          return;
        }
        const meta = metadata();
        if (this.mode === 'cwd') meta.cwd = fileURLToPath(new URL('../src', import.meta.url));
        const turns = this.reads === 1 ? (this.mode === 'old' ? [turn(oldId, 'completed')] : []) : [turn(this.mode === 'old' ? oldId : newId, this.mode === 'running' && this.reads === 2 ? 'inProgress' : 'completed')];
        if (this.reads > 1 && this.mode === 'wrong-caller') turns[0].items[0].output.text = turns[0].items[0].output.text.replace(ids.callerThreadId, ids.threadId);
        if (this.reads > 1 && this.mode === 'truncated') turns[0].items[0].output.truncated = true;
        if (this.reads > 1 && this.mode === 'wrong-target') meta.id = ids.callerThreadId;
        result = payload({ schemaVersion: 1, thread: meta, turns });
      }
      const encoded = Buffer.from(JSON.stringify({ jsonrpc: '2.0', id: request.id, result }));
      const frame = Buffer.alloc(4 + encoded.length); frame.writeUInt32LE(encoded.length); encoded.copy(frame, 4);
      this.emit('data', frame.subarray(0, 3)); setImmediate(() => this.emit('data', frame.subarray(3)));
    });
    return true;
  }
  destroy() { this.emit('close'); }
}

test('refuses another existing directory before dispatch', async () => {
  const socket = new HostSocket('cwd');
  await assert.rejects(run(input, { socket }), (error) => error.errorKind === 'before_dispatch');
  assert.equal(socket.sends.length, 0);
});
test('refuses missing host capabilities before dispatch', async () => {
  const socket = new HostSocket('missing');
  await assert.rejects(run(input, { socket }), (error) => error.errorKind === 'before_dispatch');
  assert.equal(socket.sends.length, 0);
});
test('never promotes an old matching completed turn into a new receipt', async () => {
  const socket = new HostSocket('old'); const events = [];
  await assert.rejects(run(input, { socket, deadlineMs: 40, observe: (event) => events.push(event) }), (error) => error.errorKind === 'after_dispatch');
  assert.equal(socket.sends.length, 1); assert.deepEqual(events, []);
});
test('receives an active turn then completes that exact turn without setting overrides', async () => {
  const socket = new HostSocket('running'); const events = [];
  const result = await run(input, { socket, observe: (event) => events.push(event) });
  assert.deepEqual(events.map((event) => [event.phase, event.turnId]), [['received', newId]]);
  assert.equal(result.phase, 'completed'); assert.equal(result.turnId, newId); assert.equal(result.finalText, 'done');
  assert.deepEqual(socket.sends, [{ threadId: ids.threadId, prompt: expectedPrompt }]);
  assert.ok(socket.requests.filter((request) => request.method === 'tools/call').every((request) => request.params.threadId === ids.callerThreadId && request.params.turnId === ids.callerTurnId));
});
test('uses one latest turn when the native chat has oversized history', async () => {
  const socket = new HostSocket('history');
  const events = [];
  const result = await run(input, { socket, observe: (event) => events.push(event) });
  assert.deepEqual(events.map((event) => [event.phase, event.turnId]), [['received', newId]]);
  assert.equal(result.turnId, newId);
  assert.equal(result.finalText, 'done');
  assert.equal(socket.sends.length, 1);
});
test('does not replay an ambiguous dispatch', async () => {
  const socket = new HostSocket('ambiguous');
  await assert.rejects(run(input, { socket }), (error) => error.errorKind === 'after_dispatch');
  assert.equal(socket.sends.length, 1);
});
test('rejects invented caller identities before any host request', async () => {
  const socket = new HostSocket();
  await assert.rejects(run({ ...input, callerTurnId: 'bad' }, { socket }), (error) => error.errorKind === 'before_dispatch');
  assert.deepEqual(socket.requests, []);
});
for (const mode of ['wrong-caller', 'truncated', 'wrong-target']) {
  test(`does not mint receipts from ${mode} host evidence`, async () => {
    const socket = new HostSocket(mode); const events = [];
    await assert.rejects(run(input, { socket, deadlineMs: 40, observe: (event) => events.push(event) }), (error) => error.errorKind === 'after_dispatch');
    assert.equal(socket.sends.length, 1); assert.deepEqual(events, []);
  });
}
test('embedded module entry parses bounded stdin and refuses an invalid caller', () => {
  const source = readFileSync(new URL('../src/desktop_chats.mjs', import.meta.url), 'utf8');
  const result = spawnSync(process.execPath, ['--input-type=module', '-e', source, '--', '--graphhelm-desktop-main'], {
    input: JSON.stringify({ ...input, callerThreadId: 'invalid' }) + '\n', encoding: 'utf8', timeout: 2000,
  });
  assert.equal(result.status, 1); assert.equal(JSON.parse(result.stdout).errorKind, 'before_dispatch');
});
