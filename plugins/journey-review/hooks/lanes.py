"""lanes.py <project> <token-file>: print one JSON line with the open task board and the build slots.

The Runtime URL and the execution come from the project, never from this file: the environment
(GRAPHHELM_RUNTIME_URL, GRAPHHELM_EXECUTION), then `<project>/.graphhelm/team.json`
(`runtimeUrl`, `executionId`). Only the Runtime URL has a local default; an execution binding is
required. Only a loopback URL is called, because the token goes with the request. The token file is
read here only; the token is never printed.

The request goes through its own opener: no proxy (HTTP_PROXY and friends are ignored) and no
redirect handler, so a 3xx is an error and the Bearer token never leaves the loopback listener."""
import json, os, sys, urllib.parse, urllib.request
from pathlib import Path

DEFAULT_URL = 'http://127.0.0.1:8793'
MAX_TEAM_CONFIG_BYTES = 4096

# Not build_opener(): that adds the environment proxies and HTTPRedirectHandler.
OPENER = urllib.request.OpenerDirector()
for _h in (urllib.request.ProxyHandler({}), urllib.request.HTTPHandler(),
           urllib.request.HTTPDefaultErrorHandler(), urllib.request.HTTPErrorProcessor()):
    OPENER.add_handler(_h)


def team(project, execution_override=None):
    project = Path(project).absolute()
    for d in (project, *project.parents):
        p = d / '.graphhelm' / 'team.json'
        try:
            os.lstat(p)
        except FileNotFoundError:
            if (d / '.git').exists():
                break
            continue
        except OSError:
            raise SystemExit('lanes: configured team.json cannot be inspected')
        else:
            try:
                with p.open('rb') as stream:
                    raw = stream.read(MAX_TEAM_CONFIG_BYTES + 1)
            except OSError:
                raise SystemExit('lanes: configured team.json cannot be read')
            if len(raw) > MAX_TEAM_CONFIG_BYTES:
                raise SystemExit('lanes: configured team.json exceeds its byte bound')
            try:
                data = json.loads(raw.decode('utf-8'))
            except (UnicodeDecodeError, json.JSONDecodeError, RecursionError, ValueError):
                raise SystemExit('lanes: configured team.json is not valid UTF-8 JSON')
            if not isinstance(data, dict):
                raise SystemExit('lanes: configured team.json must be an object')
            if execution_override is None and not valid_execution_id(data.get('executionId')):
                raise SystemExit('lanes: configured team.json has no valid executionId')
            return data
        if (d / '.git').exists():
            break
    return {}


def valid_execution_id(value):
    """Match the Runtime's bounded opaque execution identifier."""
    if not isinstance(value, str) or not 0 < len(value) <= 128:
        return False
    try:
        encoded = value.encode('utf-8')
    except UnicodeEncodeError:
        return False
    return all(0x21 <= byte <= 0x2e or 0x30 <= byte <= 0x39 or
               0x3b <= byte <= 0x5b or 0x5d <= byte <= 0x7e
               for byte in encoded)


def main(project_arg, tokfile):
    explicit_execution = os.environ.get('GRAPHHELM_EXECUTION')
    explicit_url = os.environ.get('GRAPHHELM_RUNTIME_URL')
    explicit_token = Path(tokfile).is_file()
    fully_explicit = explicit_execution and explicit_url and explicit_token
    t = {} if fully_explicit else team(Path(project_arg), explicit_execution)
    url = (explicit_url or str(t.get('runtimeUrl') or DEFAULT_URL)).rstrip('/')
    ex = explicit_execution or t.get('executionId')
    if not valid_execution_id(ex):
        sys.exit('lanes: no valid execution binding; set GRAPHHELM_EXECUTION or configure team.json')
    parsed = urllib.parse.urlparse(url)
    if parsed.scheme != 'http' or parsed.hostname not in ('127.0.0.1', 'localhost', '::1'):
        sys.exit('lanes: the Runtime URL must be a loopback http URL')
    if not Path(tokfile).is_file() and t.get('tokenFile'):
        tokfile = str(t['tokenFile'])
    with open(tokfile, encoding='utf-8') as f:
        tok = f.readline().strip()

    def get(path):
        r = urllib.request.Request(url + path, headers={'Authorization': 'Bearer ' + tok})
        return json.load(OPENER.open(r, timeout=30))

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


if __name__ == '__main__':
    main(sys.argv[1], sys.argv[2])
