"""Build a relocatable Linux binary archive from the existing native Astra runtime.

Application code/assets are refreshed; Qt is replaced with matching official wheels.
No tests, artifact read-back verification or launch checks run in this fast builder.
"""
import argparse
import copy
import importlib.util
import io
import json
import tarfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('refresh_linux', ROOT / 'packaging/refresh-astra-package.py')
refresh = importlib.util.module_from_spec(spec)
spec.loader.exec_module(refresh)

LAUNCHER = b'''#!/bin/sh
set -eu
reqon_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
exec "$reqon_dir/Reqon" "$@"
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base', type=Path, required=True)
    parser.add_argument('--python311', type=Path, required=True)
    parser.add_argument('--qt-wheels', type=Path, required=True)
    args = parser.parse_args()
    refresh.WORK.mkdir(parents=True, exist_ok=True)
    qt_files, qt_links, qt_python, qt_wheels = refresh.load_qt_runtime(args.qt_wheels)
    base = refresh.read_ar(args.base)
    with tarfile.open(fileobj=io.BytesIO(base['data.tar.xz'])) as source:
        executable = source.extractfile('./opt/getman/Getman').read()
    updated = refresh.refresh_executable(executable, args.python311, verify=False, python_overlay=qt_python)
    replacements = {
        'Reqon/Reqon': (updated, 0o755),
        'Reqon/reqon': (LAUNCHER, 0o755),
        'Reqon/README.txt': ((ROOT / 'packaging/portable-linux-readme.txt').read_bytes(), 0o644),
        'Reqon/reqon.png': ((ROOT / 'Reqon_Icon_Pack_Approved/linux/reqon.png').read_bytes(), 0o644),
    }
    for name, entry in qt_files.items():
        replacements['Reqon/_internal/' + name] = entry
    assets = list((ROOT / 'Reqon_Icon_Pack_Approved/png/transparent').glob('*.png'))
    assets += [ROOT / 'Reqon_Icon_Pack_Approved/linux/reqon.png', ROOT / 'reqon-ui-states v2/manifest.json']
    assets += list((ROOT / 'reqon-ui-states v2/dark').glob('*.png'))
    for path in assets:
        replacements['Reqon/_internal/' + path.relative_to(ROOT).as_posix()] = (path.read_bytes(), 0o644)
    # Keep a provenance record alongside the binary; do not label it runtime-tested.
    report = {
        'product': 'Reqon', 'version': refresh.VERSION, 'target': 'ALT Linux 11 (p11), x86-64',
        'format': 'portable native Linux tar.gz', 'minimum_glibc': '2.36',
        'build_mode': 'updated code/resources and replaced complete PySide6/Qt/Shiboken runtime',
        'base_package': args.base.name, 'python_bytecode': '3.11',
        'native_runtime': 'Base Linux ELF/Python/gRPC; Qt/PySide6/Shiboken replaced with official 6.11.3 wheels',
        'qt_version': refresh.QT_VERSION, 'qt_wheels': qt_wheels,
        'tests_run': False, 'artifact_verification_run': False, 'linux_launch_verified': False,
        'validation_note': 'Tests and verification omitted at user request.',
        'data_directory': '${XDG_DATA_HOME:-~/.local/share}/Reqon',
    }
    replacements['Reqon/build-info.json'] = ((json.dumps(report, indent=2) + '\n').encode(), 0o644)
    package = ROOT / f'dist/Reqon-{refresh.VERSION}-linux-x86_64.tar.gz'
    package.parent.mkdir(parents=True, exist_ok=True)
    seen = set()
    with tarfile.open(fileobj=io.BytesIO(base['data.tar.xz'])) as source, tarfile.open(package, 'w:gz', compresslevel=1, format=tarfile.GNU_FORMAT) as target:
        for original in source:
            if not original.name.startswith('./opt/getman/') and original.name != './opt/getman':
                continue
            if original.name.startswith('./opt/getman/_internal/') and refresh.is_qt_runtime(original.name[len('./opt/getman/_internal/'):]):
                continue
            info = copy.copy(original)
            info.name = info.name.replace('./opt/getman', 'Reqon', 1)
            if info.name == 'Reqon/Getman':
                info.name = 'Reqon/Reqon'
            if info.name in replacements:
                continue
            if info.islnk():
                info.linkname = info.linkname.replace('./opt/getman', 'Reqon', 1)
            # Archive only the app payload; retain native permissions and relative symlinks.
            target.addfile(info, source.extractfile(original) if original.isfile() else None)
            seen.add(info.name)
        for name, (data, mode) in replacements.items():
            parts = name.split('/')
            for i in range(1, len(parts)):
                directory = '/'.join(parts[:i])
                if directory not in seen:
                    info = tarfile.TarInfo(directory)
                    info.type, info.mode = tarfile.DIRTYPE, 0o755
                    target.addfile(info)
                    seen.add(directory)
            info = tarfile.TarInfo(name)
            info.size, info.mode, info.mtime = len(data), mode, int(time.time())
            target.addfile(info, io.BytesIO(data))
        for name, destination in qt_links.items():
            info = tarfile.TarInfo('Reqon/_internal/' + name)
            info.type, info.mode, info.linkname = tarfile.SYMTYPE, 0o777, destination
            target.addfile(info)
    report.update(artifact=package.name, bytes=package.stat().st_size)
    (ROOT / f'dist/Reqon-{refresh.VERSION}-linux-portable-build-info.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print('Created:', package, '\nBytes:', report['bytes'], '\nTests and verification skipped.', flush=True)


if __name__ == '__main__':
    main()
