"""lanes.py <project> <token-file>: print one JSON line with the open task board and the build slots.

The Runtime URL and the execution come from the project, never from this file: the environment
(GRAPHHELM_RUNTIME_URL, GRAPHHELM_EXECUTION), then `<project>/.graphhelm/team.json`
(`runtimeUrl`, `executionId`), then the local defaults. Only a loopback URL is called, because the
token goes with the request. The token file is read here only; the token is never printed."""
import json, os, sys, urllib.parse, urllib.request
from pathlib import Path

DEFAULT_URL, DEFAULT_EXECUTION = 'http://127.0.0.1:8793', 'gh-team'


def team(project):
    for d in (project, *project.parents):
        p = d / '.graphhelm' / 'team.json'
        if p.is_file():
            try:
                data = json.loads(p.read_bytes()[:4096])
            except (OSError, ValueError):
                return {}
            return data if isinstance(data, dict) else {}
    return {}


def main(project_arg, tokfile):
    t = team(Path(project_arg))
    url = (os.environ.get('GRAPHHELM_RUNTIME_URL') or str(t.get('runtimeUrl') or DEFAULT_URL)).rstrip('/')
    ex = os.environ.get('GRAPHHELM_EXECUTION') or str(t.get('executionId') or DEFAULT_EXECUTION)
    parsed = urllib.parse.urlparse(url)
    if parsed.scheme != 'http' or parsed.hostname not in ('127.0.0.1', 'localhost', '::1'):
        sys.exit('lanes: the Runtime URL must be a loopback http URL')
    if not Path(tokfile).is_file() and t.get('tokenFile'):
        tokfile = str(t['tokenFile'])
    with open(tokfile, encoding='utf-8') as f:
        tok = f.readline().strip()

    def get(path):
        r = urllib.request.Request(url + path, headers={'Authorization': 'Bearer ' + tok})
        return json.load(urllib.request.urlopen(r, timeout=30))

    d = get('/v1/executions/%s/tasks' % urllib.parse.quote(ex, safe='')).get('data', {})
    try:
        slots = get('/v1/workspaces/slots').get('data', {}).get('slots', [])
    except Exception:
        slots = []
    keep = ('issue', 'title', 'step', 'lane', 'pr', 'prTitle', 'clock', 'lastHeartbeatAt', 'claimedAt',
            'blockedBy', 'releasedBy', 'key')
    tasks = [{k: x.get(k) for k in keep} for x in d.get('tasks', [])]
    print(json.dumps({
        'now': d.get('now'), 'tasks': tasks, 'slots': slots,
        'stalls': [s.get('key') for s in d.get('stalls', [])],
        'silent': [l['lane'] for l in d.get('liveness', {}).get('lanes', []) if l.get('silent')],
    }))


main(sys.argv[1], sys.argv[2])
