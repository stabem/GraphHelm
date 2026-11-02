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

export function spawn(command, args, options) {
  return Array.isArray(args)
    ? original.spawn(command, args, hidden(options))
    : original.spawn(command, hidden(args));
}

export function spawnSync(command, args, options) {
  return Array.isArray(args)
    ? original.spawnSync(command, args, hidden(options))
    : original.spawnSync(command, hidden(args));
}

export function exec(command, options, callback) {
  return typeof options === 'function'
    ? original.exec(command, hidden(undefined), options)
    : original.exec(command, hidden(options), callback);
}

export function execFile(file, args, options, callback) {
  if (Array.isArray(args)) {
    return typeof options === 'function'
      ? original.execFile(file, args, hidden(undefined), options)
      : original.execFile(file, args, hidden(options), callback);
  }
  return typeof args === 'function'
    ? original.execFile(file, hidden(undefined), args)
    : original.execFile(file, hidden(args), options);
}

export function fork(modulePath, args, options) {
  return Array.isArray(args)
    ? original.fork(modulePath, args, hidden(options))
    : original.fork(modulePath, hidden(args));
}

// Playwright obtains childProcess.spawn from the module object at launch time.
childProcess.spawn = spawn;
childProcess.spawnSync = spawnSync;
childProcess.exec = exec;
childProcess.execFile = execFile;
childProcess.fork = fork;
