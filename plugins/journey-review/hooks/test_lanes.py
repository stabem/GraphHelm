"""lanes.py sends the Bearer token to the loopback Runtime only: no proxy, no redirect (#1646).
Runs against local http.server instances; no network."""
import http.server
import json
import os
import subprocess
import sys
import tempfile
import threading
import unittest
from pathlib import Path

LANES = Path(__file__).with_name('lanes.py')
TOKEN = 'secret-token-1646'
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0


class Server:
    """A loopback server; `routes` maps a path to (status, headers, body); `seen` logs requests."""

    def __init__(self, routes):
        seen = self.seen = []

        class H(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                seen.append((self.path, self.headers.get('Authorization')))
                status, headers, body = routes.get(self.path, (404, {}, b'{}'))
                self.send_response(status)
                for k, v in headers.items():
                    self.send_header(k, v)
                self.send_header('Content-Length', str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, *a):
                pass

        self.httpd = http.server.ThreadingHTTPServer(('127.0.0.1', 0), H)
        self.url = 'http://127.0.0.1:%d' % self.httpd.server_address[1]
        threading.Thread(target=self.httpd.serve_forever, daemon=True).start()

    def close(self):
        self.httpd.shutdown()
        self.httpd.server_close()


def ok(data):
    return 200, {'Content-Type': 'application/json'}, json.dumps({'data': data}).encode()


TASKS = '/v1/executions/gh-team/tasks'
SLOTS = '/v1/workspaces/slots'


class LanesTokenStaysOnLoopback(unittest.TestCase):
    def setUp(self):
        self.servers = []
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.tok = Path(self.tmp.name, 'events.token')
        self.tok.write_text(TOKEN + '\n', encoding='utf-8')

    def serve(self, routes):
        s = Server(routes)
        self.servers.append(s)
        self.addCleanup(s.close)
        return s

    def run_lanes(self, url, **env):
        e = {k: v for k, v in os.environ.items()
             if k.upper() not in ('HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY', 'NO_PROXY')}
        e.update({'GRAPHHELM_RUNTIME_URL': url, 'GRAPHHELM_EXECUTION': 'gh-team'})
        e.update(env)
        return subprocess.run([sys.executable, '-I', str(LANES), self.tmp.name, str(self.tok)],
                              capture_output=True, text=True, timeout=60, env=e,
                              creationflags=NO_WINDOW)

    def run_lanes_from_project(self, project, **env):
        e = {k: v for k, v in os.environ.items()
             if k.upper() not in ('GRAPHHELM_RUNTIME_URL', 'GRAPHHELM_EXECUTION',
                                  'HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY', 'NO_PROXY')}
        e.update(env)
        return subprocess.run([sys.executable, '-I', str(LANES), str(project), str(self.tok)],
                              capture_output=True, text=True, timeout=60, env=e,
                              creationflags=NO_WINDOW)

    def test_reads_the_board_with_the_bearer_token(self):
        rt = self.serve({TASKS: ok({'now': 'n', 'tasks': [{'issue': 7, 'step': 'plan'}]}),
                         SLOTS: ok({'slots': [{'name': 's'}]})})
        r = self.run_lanes(rt.url)
        self.assertEqual(r.returncode, 0, r.stderr)
        out = json.loads(r.stdout)
        self.assertEqual(out['tasks'][0]['issue'], 7)
        self.assertEqual(out['slots'], [{'name': 's'}])
        self.assertEqual({a for _, a in rt.seen}, {'Bearer ' + TOKEN})
        self.assertNotIn(TOKEN, r.stdout + r.stderr)

    def test_http_proxy_is_ignored(self):
        proxy = self.serve({})
        rt = self.serve({TASKS: ok({'tasks': []}), SLOTS: ok({'slots': []})})
        r = self.run_lanes(rt.url, HTTP_PROXY=proxy.url, http_proxy=proxy.url, ALL_PROXY=proxy.url)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(proxy.seen, [], 'the token went to the proxy')
        self.assertEqual(len(rt.seen), 2)

    def test_redirect_on_tasks_is_an_error_and_not_followed(self):
        elsewhere = self.serve({TASKS: ok({'tasks': []})})
        rt = self.serve({TASKS: (302, {'Location': elsewhere.url + TASKS}, b'')})
        r = self.run_lanes(rt.url)
        self.assertNotEqual(r.returncode, 0)
        self.assertEqual(elsewhere.seen, [], 'the token followed a redirect off the listener')
        self.assertNotIn(TOKEN, r.stdout + r.stderr)

    def test_redirect_on_slots_is_not_followed(self):
        elsewhere = self.serve({SLOTS: ok({'slots': [{'name': 'evil'}]})})
        rt = self.serve({TASKS: ok({'tasks': []}),
                         SLOTS: (307, {'Location': elsewhere.url + SLOTS}, b'')})
        r = self.run_lanes(rt.url)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(json.loads(r.stdout)['slots'], [])
        self.assertEqual(elsewhere.seen, [])

    def test_non_loopback_url_is_refused_before_any_request(self):
        for url in ('http://example.com', 'http://192.0.2.1:8793', 'https://127.0.0.1:8793'):
            r = self.run_lanes(url)
            self.assertNotEqual(r.returncode, 0, url)
            self.assertIn('loopback http URL', r.stderr, url)

    def test_project_binding_selects_configured_execution_path(self):
        rt = self.serve({'/v1/executions/studio-v2/tasks': ok({'tasks': []}),
                         SLOTS: ok({'slots': []})})
        project = Path(self.tmp.name, 'nested')
        (project / '.graphhelm').mkdir(parents=True)
        (project / '.graphhelm' / 'team.json').write_text(
            json.dumps({'runtimeUrl': rt.url, 'executionId': 'studio-v2'}), encoding='utf-8')
        r = self.run_lanes_from_project(project)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual([path for path, _ in rt.seen],
                         ['/v1/executions/studio-v2/tasks', SLOTS])

    def test_explicit_execution_keeps_configured_runtime_url(self):
        rt = self.serve({'/v1/executions/override-run/tasks': ok({'tasks': []}),
                         SLOTS: ok({'slots': []})})
        project = Path(self.tmp.name, 'override')
        (project / '.graphhelm').mkdir(parents=True)
        (project / '.graphhelm' / 'team.json').write_text(
            json.dumps({'runtimeUrl': rt.url, 'executionId': 'configured-run'}), encoding='utf-8')
        r = self.run_lanes_from_project(project, GRAPHHELM_EXECUTION='override-run')
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual([path for path, _ in rt.seen],
                         ['/v1/executions/override-run/tasks', SLOTS])

    def test_broken_project_binding_refuses_before_any_request(self):
        rt = self.serve({})
        project = Path(self.tmp.name, 'broken')
        (project / '.graphhelm').mkdir(parents=True)
        (project / '.graphhelm' / 'team.json').write_text('{"executionId": "bad/id"}', encoding='utf-8')
        r = self.run_lanes_from_project(project, GRAPHHELM_RUNTIME_URL=rt.url)
        self.assertNotEqual(r.returncode, 0)
        self.assertIn('valid executionId', r.stderr)
        self.assertEqual(rt.seen, [])


if __name__ == '__main__':
    unittest.main()
