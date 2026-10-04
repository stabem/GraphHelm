// Bounded GraphHelm bridge for the Desktop app-tools named pipe.
// This file is embedded by the Rust adapter and is also exercised directly by the offline tests.
import fs from 'node:fs';
import net from 'node:net';

const MAX_FRAME = 8 * 1024 * 1024;
const MAX_MESSAGE = 2000;
const MAX_PATH = 4096;
const DEADLINE_MS = 1_800_000;
const MARKER_PREFIX = '[GraphHelm request: ';

const validUuid = (value) => typeof value === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(value);
const validRequestId = (value) => typeof value === 'string' && /^[A-Za-z0-9_-]{1,64}$/.test(value);
const markerFor = (requestId) => `${MARKER_PREFIX}${requestId}]`;

function fail(message, errorKind = 'after_dispatch') { const error = new Error(message); error.errorKind = errorKind; throw error; }
function itemText(item) {
  if (typeof item?.text === 'string') return item.text;
  if (Array.isArray(item?.content)) return item.content.map((part) => itemText(part)).join('');
  return '';
}
function turnsOf(read) { return Array.isArray(read?.turns) ? read.turns : []; }
function canonicalPath(value) {
  const path = fs.realpathSync.native(value);
  if (process.platform !== 'win32') return path;
  return (path.startsWith('\\\\?\\UNC\\') ? '\\\\' + path.slice(8) : path.replace(/^\\\\\?\\/, '')).toLowerCase();
}
function validateRead(read, input, expectedCwd, errorKind = 'before_dispatch') {
  const meta = read?.thread;
  let actualCwd = '';
  try { if (typeof meta?.cwd === 'string' && meta.cwd.length <= MAX_PATH) actualCwd = canonicalPath(meta.cwd); } catch { fail('target native chat metadata does not match', errorKind); }
  if (read?.schemaVersion !== 1 || meta?.id !== input.threadId || meta?.kind !== 'codex' || meta?.hostId !== 'local' || actualCwd !== expectedCwd || typeof meta?.status?.type !== 'string') fail('target native chat metadata does not match', errorKind);
  return meta;
}
function findTurn(read, input) {
  const expected = `<codex_delegation>\n  <source_thread_id>${input.callerThreadId}</source_thread_id>\n  <input>${input.message}</input>\n</codex_delegation>`;
  for (const turn of turnsOf(read)) {
    if (!validUuid(turn.id) || !Array.isArray(turn.items)) continue;
    for (const item of turn.items) {
      if (item.type === 'functionCallOutput' && item.namespace === 'codex_app' &&
          item.name === 'send_message_to_thread' && item.output?.truncated === false &&
          item.output?.text === expected) return turn;
    }
  }
  return null;
}
function matchingTurns(read, input) {
  return turnsOf(read).filter((turn) => findTurn({ turns: [turn] }, input));
}
const isNewCorrelatedTurn = (read, input, baselineIds) => {
  const turn = matchingTurns(read, input).find((candidate) => !baselineIds.has(candidate.id));
  return turn && !baselineIds.has(turn.id) ? turn : null;
};
function finalText(turn) {
  let text = '';
  for (const value of turn.items || []) {
    if (value.type === 'agentMessage' && (value.phase === undefined || value.phase === 'final_answer')) text = itemText(value) || text;
  }
  return [...text].slice(0, MAX_MESSAGE).join('');
}

function validateInput(input) {
  if (!input || !validUuid(input.threadId) || !validRequestId(input.requestId) || !validUuid(input.callerThreadId) || !validUuid(input.callerTurnId)) fail('desktop identity is invalid', 'before_dispatch');
  if (typeof input.message !== 'string' || input.message.length === 0 || [...input.message].length > MAX_MESSAGE) fail('message is invalid', 'before_dispatch');
  if (typeof input.sourceDirectory !== 'string' || input.sourceDirectory.length === 0 || input.sourceDirectory.length > MAX_PATH) fail('source directory is invalid', 'before_dispatch');
  if (typeof input.pipePath !== 'string' || !input.pipePath.startsWith('\\\\.\\pipe\\') || input.pipePath.length > MAX_PATH) fail('app-tools pipe is invalid', 'before_dispatch');
  if (!fs.existsSync(input.sourceDirectory) || !fs.statSync(input.sourceDirectory).isDirectory()) fail('source directory is unavailable', 'before_dispatch');
}

function frame(value) {
  const body = Buffer.from(JSON.stringify(value));
  if (body.length > MAX_FRAME) fail('app-tools frame exceeded limit');
  const out = Buffer.allocUnsafe(4 + body.length); out.writeUInt32LE(body.length, 0); body.copy(out, 4); return out;
}
function connect(pipePath, deadline) {
  return new Promise((resolve, reject) => {
    const socket = net.createConnection(pipePath);
    const timer = setTimeout(() => { socket.destroy(); reject(new Error('app-tools pipe timed out')); }, Math.min(10_000, Math.max(1, deadline - Date.now())));
    socket.once('connect', () => { clearTimeout(timer); socket.on('error', () => {}); resolve(socket); });
    socket.once('error', (error) => { clearTimeout(timer); reject(error); });
  });
}
function rpc(socket, id, method, params, deadline) {
  return new Promise((resolve, reject) => {
    let buffer = Buffer.alloc(0);
    let settled = false;
    const finish = (callback, value) => { if (settled) return; settled = true; clearTimeout(timer); socket.off('data', onData); socket.off('error', onError); socket.off('close', onClose); callback(value); };
    const onError = () => finish(reject, new Error('app-tools pipe failed'));
    const onClose = () => finish(reject, new Error('app-tools pipe closed'));
    const timer = setTimeout(() => { socket.destroy(); finish(reject, new Error('app-tools call timed out')); }, Math.min(10_000, Math.max(1, deadline - Date.now())));
    const onData = (chunk) => {
      buffer = Buffer.concat([buffer, chunk]);
      if (buffer.length > MAX_FRAME + 4) { socket.destroy(); finish(reject, new Error('app-tools response exceeded limit')); return; }
      while (buffer.length >= 4) {
        const length = buffer.readUInt32LE(0); if (length > MAX_FRAME) { socket.destroy(); finish(reject, new Error('app-tools frame exceeded limit')); return; }
        if (buffer.length < length + 4) return;
        const body = buffer.subarray(4, length + 4); buffer = buffer.subarray(length + 4);
        let value; try { value = JSON.parse(body.toString('utf8')); } catch { socket.destroy(); finish(reject, new Error('app-tools returned malformed JSON')); return; }
        if (value.jsonrpc !== '2.0') { finish(reject, new Error('app-tools protocol is unsupported')); return; }
        if (value.id === id) { if (value.error) finish(reject, new Error('app-tools call failed')); else finish(resolve, value.result); return; }
      }
    };
    socket.on('data', onData); socket.once('error', onError); socket.once('close', onClose);
    try { socket.write(frame({ jsonrpc: '2.0', id, method, params })); } catch { finish(reject, new Error('app-tools request could not be written')); }
  });
}
async function callTool(socket, sequence, input, tool, args, deadline) {
  const result = await rpc(socket, sequence, 'tools/call', { callerSource: 'codex', hostId: 'local', namespace: 'codex_app', threadId: input.callerThreadId, turnId: input.callerTurnId, callId: `${input.requestId}-${sequence}`, tool, arguments: args }, deadline);
  if (result?.success !== true) fail(`Desktop app-tool ${tool} failed`);
  return result;
}
function payload(result) {
  const text = result?.contentItems?.find((item) => item.type === 'inputText')?.text;
  if (typeof text !== 'string') fail('app-tools returned no bounded payload');
  try { return JSON.parse(text); } catch { fail('app-tools returned malformed payload'); }
}

export async function run(input, io = {}) {
  validateInput(input);
  const marker = markerFor(input.requestId); const prompt = `${input.message}\n\n${marker}`; const deadline = Date.now() + (io.deadlineMs ?? DEADLINE_MS);
  let socket;
  let dispatched = false;
  try {
    socket = io.socket || await connect(input.pipePath, deadline);
    const listed = await rpc(socket, 1, 'tools/list', { threadStartKind: 'all' }, deadline);
    const names = new Set((Array.isArray(listed?.tools) ? listed.tools : []).map((tool) => `${tool.namespace}/${tool.name}`));
    if (!names.has('codex_app/read_thread') || !names.has('codex_app/send_message_to_thread')) fail('required Desktop app-tools are unavailable', 'before_dispatch');
    const before = payload(await callTool(socket, 2, input, 'read_thread', { threadId: input.threadId, includeOutputs: true, maxOutputCharsPerItem: 8192, turnLimit: 1 }, deadline));
    const expectedCwd = canonicalPath(input.sourceDirectory);
    const meta = validateRead(before, input, expectedCwd);
    if (!['idle', 'notLoaded'].includes(meta.status.type)) fail('target native chat is already running', 'before_dispatch');
    const baseline = new Set(turnsOf(before).map((turn) => turn.id));
    dispatched = true;
    const accepted = payload(await callTool(socket, 3, input, 'send_message_to_thread', { threadId: input.threadId, prompt }, deadline));
    if (accepted?.threadId !== input.threadId) fail('Desktop send returned a mismatched thread');
    let readSequence = 4;
    let received = null;
    while (Date.now() < deadline) {
      const read = payload(await callTool(socket, readSequence++, input, 'read_thread', { threadId: input.threadId, includeOutputs: true, maxOutputCharsPerItem: 8192, turnLimit: 1 }, deadline));
      validateRead(read, input, expectedCwd, 'after_dispatch');
      if (matchingTurns(read, { ...input, message: prompt }).length > 1) fail('multiple native turns match this request');
      const turn = isNewCorrelatedTurn(read, { ...input, message: prompt }, baseline);
      if (turn && !baseline.has(turn.id)) {
        if (!received) {
          received = turn;
          io.observe?.({ phase: 'received', threadId: input.threadId, sourceId: meta.id, sourceCwd: meta.cwd, sourceDirectory: meta.cwd, title: meta.title || meta.name || input.threadId, turnId: turn.id });
        } else if (received.id !== turn.id) fail('multiple native turns match this request');
        else received = turn;
        if (received.status === 'completed') break;
        if (['failed', 'interrupted'].includes(received.status)) fail('native turn stopped before completion');
      }
      await new Promise((resolve) => setTimeout(resolve, Math.min(1000, Math.max(0, deadline - Date.now()))));
    }
    if (!received) fail('dispatched message was not observed in the target chat');
    if (received.status !== 'completed') fail('target chat turn did not complete');
    const reply = finalText(received);
    if (!reply) fail('target chat completed without a final answer');
    return { phase: 'completed', completed: true, threadId: input.threadId, sourceId: meta.id, sourceCwd: meta.cwd, sourceDirectory: meta.cwd, title: meta.title || meta.name || input.threadId, turnId: received.id, finalText: reply };
  } catch (error) { const wrapped = new Error(dispatched ? (error.message || 'Desktop bridge failed') : 'Desktop preflight failed; no instruction was dispatched'); wrapped.errorKind = dispatched ? 'after_dispatch' : 'before_dispatch'; throw wrapped; }
  finally { if (!io.socket) socket?.destroy(); }
}

async function main() {
  try {
    const line = await new Promise((resolve, reject) => {
      let buffer = Buffer.alloc(0);
      const timer = setTimeout(() => finish(reject, new Error('Desktop input timed out')), 10_000);
      const finish = (callback, value) => {
        clearTimeout(timer); process.stdin.off('data', onData); process.stdin.off('end', onEnd);
        process.stdin.off('error', onEnd); process.stdin.destroy(); callback(value);
      };
      const onEnd = () => finish(reject, new Error('Desktop input was incomplete'));
      const onData = (chunk) => {
        if (buffer.length + chunk.length > 64 * 1024) return finish(reject, new Error('Desktop input exceeded limit'));
        buffer = Buffer.concat([buffer, chunk]);
        const end = buffer.indexOf(10);
        if (end >= 0) finish(resolve, buffer.subarray(0, end).toString('utf8'));
      };
      process.stdin.on('data', onData); process.stdin.once('end', onEnd); process.stdin.once('error', onEnd);
    });
    let input;
    try { input = JSON.parse(line); } catch { fail('Desktop input was malformed', 'before_dispatch'); }
    const result = await run(input, { observe: (event) => process.stdout.write(`${JSON.stringify(event)}\n`) });
    process.stdout.write(`${JSON.stringify(result)}\n`);
  } catch (error) {
    process.stdout.write(`${JSON.stringify({ error: error.errorKind ? error.message : 'Desktop input was invalid', errorKind: error.errorKind || 'before_dispatch' })}\n`);
    process.exitCode = 1;
  }
}
if (process.argv.includes('--graphhelm-desktop-main')) main();
