// The disposable fixture project carries the files its journey flows name in `scope` (#1041).
//
//   node tools/studio-journey-fixture/copy_scope.mjs <repo> <project>
//
// fixture.sh copies `.graphhelm/journeys` into a disposable project so no document save reaches
// the repository (#585). The Runtime resolves every screen `scope` path against that project and
// refuses a flow whose scope file is absent (`flow.scope_path_missing`), so a fixture preview was
// refused before it ran. This copies each named file from the repository into the project as a
// read-only snapshot: nothing is ever written to <repo>, and a path that is absolute or climbs out
// with `..` is skipped, the same paths the Runtime itself refuses as outside the project.
import fs from 'node:fs';
import path from 'node:path';

const [repo, project] = process.argv.slice(2);
if (!repo || !project) {
  console.error('usage: copy_scope.mjs <repo> <project>');
  process.exit(2);
}
const journeys = path.join(project, '.graphhelm', 'journeys');
const scoped = new Set();
for (const name of fs.readdirSync(journeys)) {
  if (!name.endsWith('.journey.yaml')) continue;
  const text = fs.readFileSync(path.join(journeys, name), 'utf8');
  // Canonical flows write each screen's scope as one inline list: `scope: [a, b]`.
  for (const match of text.matchAll(/^\s*scope:\s*\[([^\]]*)\]\s*$/gm)) {
    for (const entry of match[1].split(',')) {
      const value = entry.trim().replace(/^["']|["']$/g, '');
      if (value) scoped.add(value);
    }
  }
}
let copied = 0;
for (const entry of [...scoped].sort()) {
  const parts = entry.split('/');
  if (path.isAbsolute(entry) || /[\\:]/.test(entry) || parts.some((part) => part === '..')) {
    console.error(`skipped (outside the project): ${entry}`);
    continue;
  }
  const source = path.join(repo, ...parts);
  if (!fs.existsSync(source) || !fs.statSync(source).isFile()) continue;
  const target = path.join(project, ...parts);
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.copyFileSync(source, target);
  copied += 1;
}
console.log(`copied ${copied} scope files`);
