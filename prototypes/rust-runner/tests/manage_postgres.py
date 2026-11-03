"""An isolated local PostgreSQL cluster; never reconfigures the installed service."""
import os
import shutil
import shlex
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / 'artifacts' / 'postgres-fixture'
PORT = 55489


def binaries():
    if os.name == 'nt':
        roots = sorted(Path('C:/Program Files/PostgreSQL').glob('*/bin/pg_ctl.exe'), key=lambda p: int(p.parents[1].name), reverse=True)
        if roots:
            return roots[0].parent
    config = shutil.which('pg_config')
    if config:
        return Path(subprocess.check_output([config, '--bindir'], text=True).strip())
    raise RuntimeError('PostgreSQL tools are missing; install PostgreSQL or set REQON_TEST_PG_DSN')


def ensure_postgres():
    if os.environ.get('REQON_TEST_PG_DSN'):
        return os.environ['REQON_TEST_PG_DSN']
    bin_dir = binaries()
    suffix = '.exe' if os.name == 'nt' else ''
    ctl = str(bin_dir / ('pg_ctl' + suffix))
    creationflags = subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0
    def call(args, check=True):
        # A Windows postgres child can inherit a captured pipe even with pg_ctl
        # -l. Waiting for pipe EOF then hangs after the server is already ready.
        # A file preserves diagnostics while waiting only for pg_ctl to exit.
        with (DATA.parent / 'postgres-fixture-control.log').open('ab') as log:
            return subprocess.run(args, stdout=log, stderr=subprocess.STDOUT, check=check, creationflags=creationflags, timeout=90)
    DATA.parent.mkdir(parents=True, exist_ok=True)
    if not (DATA / 'PG_VERSION').exists():
        call([str(bin_dir / ('initdb' + suffix)), '-D', str(DATA), '-U', 'reqon_fixture', '-A', 'trust', '--encoding=UTF8', '--locale=C'])
    if call([ctl, '-D', str(DATA), 'status'], check=False).returncode:
        options = f'-h 127.0.0.1 -p {PORT}'
        if os.name != 'nt':
            options += ' -k ' + shlex.quote(str(DATA))
        call([ctl, '-D', str(DATA), '-l', str(DATA.parent / 'postgres-fixture.log'), '-o', options, '-w', 'start'])
    return f'host=127.0.0.1 port={PORT} user=reqon_fixture dbname=postgres connect_timeout=5'


if __name__ == '__main__':
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument('action', choices=['start', 'stop'], default='start', nargs='?')
    args = parser.parse_args()
    if args.action == 'start':
        ensure_postgres()
        print(f'Isolated PostgreSQL ready on loopback port {PORT}')
    elif (DATA / 'PG_VERSION').exists():
        subprocess.run([str(binaries() / ('pg_ctl.exe' if os.name == 'nt' else 'pg_ctl')), '-D', str(DATA), '-m', 'fast', '-w', 'stop'], check=True)
