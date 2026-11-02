// Hide every Node child process, including browsers launched by Playwright, on Windows.
import childProcess from 'node:child_process';
const original = {
  spawn: childProcess.spawn,
  spawnSync: childProcess.spawnSync,
  exec: childProcess.exec,
  execFile: childProcess.execFile,
  fork: childProcess.fork,
};

const hidden = (options) => process.platform === 'win32'
  ? { ...options, windowsHide: true }
  : options;

function withOptions(method, target, args, options) {
  if (Array.isArray(args)) return method(target, args, hidden(options));
  if (args === undefined && options !== undefined) {
    return method(target, undefined, hidden(options));
  }
  return method(target, hidden(args));
}

function spawn(command, args, options) {
  return withOptions(original.spawn, command, args, options);
}

function spawnSync(command, args, options) {
  return withOptions(original.spawnSync, command, args, options);
}

function exec(command, options, callback) {
  return typeof options === 'function'
    ? original.exec(command, hidden(undefined), options)
    : original.exec(command, hidden(options), callback);
}

function execFile(file, args, options, callback) {
  if (Array.isArray(args)) {
    return typeof options === 'function'
      ? original.execFile(file, args, hidden(undefined), options)
      : original.execFile(file, args, hidden(options), callback);
  }
  if (typeof args === 'function') return original.execFile(file, hidden(undefined), args);
  if (args === undefined) {
    if (typeof options === 'function' && callback === undefined) {
      return original.execFile(file, hidden(undefined), options);
    }
    if (options !== undefined || callback !== undefined) {
      return original.execFile(file, undefined, hidden(options), callback);
    }
    return original.execFile(file, hidden(undefined));
  }
  return original.execFile(file, hidden(args), options);
}

function fork(modulePath, args, options) {
  return withOptions(original.fork, modulePath, args, options);
}

export const hiddenNodeChildren = { spawn, spawnSync, exec, execFile, fork };

// Playwright obtains childProcess.spawn from the module object at launch time.
Object.assign(childProcess, hiddenNodeChildren);
