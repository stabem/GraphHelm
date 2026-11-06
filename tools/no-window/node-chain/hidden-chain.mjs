// #2573: a Node parent starts a console child through the shared hidden helper. Must show no window.
import { hiddenNodeChildren } from '../../hidden-node-children.mjs';
const child = hiddenNodeChildren.spawn('cmd.exe', ['/c', 'echo hidden-chain'], { stdio: 'ignore' });
child.on('exit', () => console.log('hidden-chain done'));
