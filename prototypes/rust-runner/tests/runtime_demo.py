"""Exercise environment/secret handling using disposable synthetic credentials."""
import json
import os
import secrets
import subprocess

from fixture import WorkflowServer
from package_fixture import build_package, write_json
from test_e2e import BINARY, ROOT


if __name__ == '__main__':
    directory = ROOT / 'artifacts' / 'runtime-demo'
    package = directory / 'package'
    files = build_package(package)
    files['cases/chain.json']['steps'] = [
        {'type': 'grpc', 'id': 'echo-secret', 'method': 'poc.Workflow/Echo',
         'request': {'payload': '${token}'}, 'expect': [{'path': '/payload', 'equals': '${token}'}]},
        {'type': 'javascript', 'id': 'copy-secret',
         'script': 'assert(response.payload === vars.token); return {fromFirst: Array.from(vars.token).reverse().join("")};'},
    ]
    files['cases/isolation.json']['steps'][0]['script'] = 'assert(typeof vars.fromFirst === "undefined"); assert(vars.token.length > 0); return {isolated:true};'
    for name in ('cases/chain.json', 'cases/isolation.json'):
        write_json(package / name, files[name])
    token = secrets.token_urlsafe(24)
    server = WorkflowServer()
    try:
        environment = directory / 'environment.json'
        write_json(environment, {
            'schema_version': 'reqon-environment/v1', 'id': 'synthetic-local',
            'endpoint': server.endpoint, 'variables': {'label': 'demo'},
            'metadata': {'authorization': '${token}'},
            'secrets': {'token': {'env': 'REQON_DEMO_SECRET'}},
        })
        child_env = os.environ.copy()
        child_env['REQON_DEMO_SECRET'] = token
        result = subprocess.run(
            [str(BINARY), 'run', str(package), '--env', str(environment),
             '--json', str(directory / 'result.json'), '--junit', str(directory / 'junit.xml')],
            capture_output=True, text=True, encoding='utf-8', env=child_env, timeout=15, check=True,
        )
        (directory / 'events.ndjson').write_text(result.stdout, encoding='utf-8')
        report = json.loads((directory / 'result.json').read_text(encoding='utf-8'))
        assert server.calls[0][2]['authorization'] == token
        for name in ('result.json', 'junit.xml', 'events.ndjson'):
            text = (directory / name).read_text(encoding='utf-8')
            assert token not in text and token[::-1] not in text, 'secret leaked into an artifact'
        assert report['redaction']['dynamic_values_withheld']
        print(json.dumps({'outcome': report['outcome'], 'cases': [c['outcome'] for c in report['cases']], 'redaction': report['redaction']}, indent=2))
        print(f'Artifacts: {directory}')
    finally:
        server.close()
