// graphhelm:intentional-visible-window — negative control for test_node_chain.ps1
// #2573 negative control: the same chain started detached, the bug. The observer must count a window.
import { spawn } from 'node:child_process';
const child = spawn(process.execPath, ['-e', "require('child_process').spawn('cmd.exe',['/c','echo detached'],{stdio:'ignore'}).on('exit',()=>{})"], { stdio: 'ignore', detached: true });
child.unref();
console.log('detached-control started');
