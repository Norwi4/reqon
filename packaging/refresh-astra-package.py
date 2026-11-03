"""Refresh an Astra package's Python 3.11 code/assets and replace its Qt runtime.

Use only with an already built, compatible Linux amd64 runtime. A Python 3.11
interpreter is required to compile bytecode; it may run on another OS. This does
not replace a Linux launch check. Other native dependencies remain unchanged.
"""
import argparse
import hashlib
import io
import json
import lzma
import re
import struct
import subprocess
import tarfile
import time
import zlib
import zipfile
from pathlib import Path

from PyInstaller.archive.readers import CArchiveReader
from PyInstaller.archive.writers import CArchiveWriter

ROOT = Path(__file__).resolve().parents[1]
WORK = ROOT / 'build/linux-refresh'
VERSION = '0.19.0'
QT_VERSION = '6.11.3'


def runner_payload():
    data = (ROOT / 'prototypes/rust-runner/dist/reqon').read_bytes()
    record = json.loads((ROOT / 'prototypes/rust-runner/dist/reqon-linux-build.json').read_text())
    expected = re.search(r"RUNNER_VERSION = '([^']+)'", (ROOT / 'runner_controller.py').read_text()).group(1)
    if not data.startswith(b'\x7fELF') or record['version'] != expected or record['sha256'] != hashlib.sha256(data).hexdigest():
        raise ValueError('Build the matching Linux Reqon CLI before packaging the application')
    return data


def is_qt_runtime(relative):
    first = relative.split('/')[0].lower()
    return first.startswith(('pyside6', 'shiboken6', 'libqt6', 'libpyside6', 'libshiboken6', 'libicu'))


def load_qt_runtime(wheel_directory):
    """Replace bindings, Qt plugins/libraries and Python support code as one set."""
    files, links, wheels = {}, {}, []
    overlay = WORK / ('qt-python-' + QT_VERSION)
    modules = {'QtCore', 'QtGui', 'QtWidgets', 'QtSvg', 'QtNetwork', 'QtDBus', 'QtOpenGL'}
    plugins = {'platforms', 'imageformats', 'iconengines', 'platforminputcontexts', 'platformthemes',
               'xcbglintegrations', 'wayland-shell-integration', 'wayland-graphics-integration-client',
               'wayland-decoration-client', 'egldeviceintegrations', 'networkinformation', 'tls', 'generic'}
    # Essentials also ships optional plugins that need libraries from Addons.
    # Reqon does not use PDF rendering, a virtual keyboard or embedded EGLFS/KMS.
    excluded_plugins = {'libqpdf.so', 'libqtvirtualkeyboardplugin.so', 'libqeglfs-kms-integration.so'}
    for package in ('pyside6_essentials', 'shiboken6'):
        matches = list(wheel_directory.glob(f'{package}-{QT_VERSION}-*-manylinux_2_34_x86_64.whl'))
        if len(matches) != 1:
            raise ValueError(f'Expected one Linux x86-64 {package} {QT_VERSION} wheel in {wheel_directory}')
        wheel = matches[0]
        wheels.append({'file': wheel.name, 'sha256': sha(wheel.read_bytes())})
        with zipfile.ZipFile(wheel) as archive:
            for name in archive.namelist():
                if name.endswith('/'):
                    continue
                path = Path(name)
                python_source = name.endswith('.py') and (len(path.parts) == 2 or name.startswith('PySide6/support/'))
                binding = name.endswith('.abi3.so') and path.stem.split('.')[0] in modules | {'Shiboken'}
                library = '.so' in path.name and (name.startswith('PySide6/Qt/lib/') or path.name.startswith(('libpyside6.', 'libshiboken6.')))
                plugin = name.startswith('PySide6/Qt/plugins/') and path.parts[3] in plugins and path.name not in excluded_plugins
                translation = name.startswith('PySide6/Qt/translations/qtbase_') and name.endswith('.qm')
                metadata = '.dist-info/' in name
                if not (python_source or binding or library or plugin or translation or metadata):
                    continue
                data = archive.read(name)
                files[name] = (data, 0o755 if '.so' in path.name else 0o644)
                if python_source:
                    destination = overlay / name
                    destination.parent.mkdir(parents=True, exist_ok=True)
                    destination.write_bytes(data)
                if library:
                    links[path.name] = name
    return files, links, overlay, wheels


def sha(data):
    return hashlib.sha256(data).hexdigest()


def read_ar(path):
    data = path.read_bytes()
    assert data[:8] == b'!<arch>\n'
    members = {}
    offset = 8
    while offset < len(data):
        header = data[offset:offset + 60]
        assert header[58:60] == b'`\n'
        size = int(header[48:58])
        members[header[:16].decode().strip().rstrip('/')] = data[offset + 60:offset + 60 + size]
        offset += 60 + size + size % 2
    assert offset == len(data)
    return members


COMPILER = r'''
import importlib.util, json, marshal, struct, sys, zlib
from pathlib import Path
root, work = map(Path, sys.argv[1:3])
verify = '--skip-verification' not in sys.argv[3:]
assert sys.version_info[:2] == (3, 11)
base = (work / 'base-PYZ.pyz').read_bytes()
assert base[4:8] == importlib.util.MAGIC_NUMBER
old = dict(marshal.loads(base[struct.unpack('!i', base[8:12])[0]:]))
sources = {p.stem: p for p in root.glob('*.py') if p.stem != 'main'}
packages = set()
overlays = [arg.split('=', 1)[1] for arg in sys.argv[3:] if arg.startswith('--overlay=')]
if overlays:
    old = {name: value for name, value in old.items() if name.split('.')[0] not in ('PySide6', 'shiboken6')}
    for directory in overlays:
        directory = Path(directory)
        for path in directory.rglob('*.py'):
            parts = list(path.relative_to(directory).with_suffix('').parts)
            package = parts[-1] == '__init__'
            if package:parts.pop()
            name = '.'.join(parts)
            sources[name] = path
            if package:packages.add(name)
data, toc = bytearray(base[:17]), []
for name in sorted(old.keys() | sources.keys()):
    if name in sources:
        code = compile(sources[name].read_bytes(), '/src/' + sources[name].name, 'exec', optimize=0)
        blob, kind = zlib.compress(marshal.dumps(code), 6), int(name in packages)
    else:
        kind, offset, length = old[name]
        blob = base[offset:offset + length]
    toc.append((name, (kind, len(data), len(blob))))
    data.extend(blob)
toc_offset = len(data)
data.extend(marshal.dumps(toc))
data[8:12] = struct.pack('!i', toc_offset)
(work / 'updated-PYZ.pyz').write_bytes(data)
(work / 'main.marshal').write_bytes(marshal.dumps(compile((root / 'main.py').read_bytes(), '/src/main.py', 'exec', optimize=0)))
# Verify every changed module can be read by the target bytecode interpreter.
if verify:
    for name, (kind, offset, length) in toc:
        if name in sources:
            code = marshal.loads(zlib.decompress(data[offset:offset + length]))
            assert code.co_filename == '/src/' + sources[name].name
print('Compiled', len(sources) + 1, 'application modules for Python', sys.version.split()[0], flush=True)
'''


def refresh_executable(executable, python311, verify=True, python_overlay=None):
    path = WORK / 'base-Getman'
    path.write_bytes(executable)
    archive = CArchiveReader(str(path))
    assert executable[:4] == b'\x7fELF' and struct.unpack_from('<H', executable, 18)[0] == 62
    cookie_offset = executable.rfind(CArchiveWriter._COOKIE_MAGIC_PATTERN)
    cookie = struct.unpack(CArchiveWriter._COOKIE_FORMAT, executable[cookie_offset:cookie_offset + CArchiveWriter._COOKIE_LENGTH])
    assert cookie[4] == 311 and cookie[5].rstrip(b'\0') == b'libpython3.11.so.1.0'
    (WORK / 'base-PYZ.pyz').write_bytes(archive.extract('PYZ.pyz'))
    compiler = WORK / 'compile_payload.py'
    compiler.write_text(COMPILER, encoding='utf-8')
    command = [str(python311), str(compiler), str(ROOT), str(WORK)]
    if not verify:
        command.append('--skip-verification')
    if python_overlay is not None:
        command.append('--overlay=' + str(python_overlay))
    subprocess.run(command, check=True)
    runner = runner_payload()
    replacements = {
        './opt/reqon/_internal/runner/reqon': (runner, 0o755),'main': (WORK / 'main.marshal').read_bytes(), 'PYZ.pyz': (WORK / 'updated-PYZ.pyz').read_bytes()}
    data, toc = bytearray(), []
    for name, (_, _, _, compressed, kind) in archive.toc.items():
        raw = replacements.get(name)
        if raw is None:
            raw = archive.extract(name)
        blob = zlib.compress(raw, 9) if compressed else raw
        toc.append((len(data), len(blob), len(raw), compressed, kind, name))
        data.extend(blob)
    for option in archive.options:
        toc.append((0, 0, 0, 0, 'o', option))
    toc_offset = len(data)
    toc_data = CArchiveWriter._serialize_toc(toc)
    data.extend(toc_data)
    data.extend(struct.pack(CArchiveWriter._COOKIE_FORMAT, cookie[0], len(data) + CArchiveWriter._COOKIE_LENGTH,
                            toc_offset, len(toc_data), cookie[4], cookie[5]))
    # The ELF has a section table after its original pydata section. Keep that
    # ELF intact and append the replacement CArchive. The PyInstaller bootloader
    # locates the last archive cookie from EOF, as does CArchiveReader.
    result = executable + data
    target = WORK / 'Reqon'
    target.write_bytes(result)
    if verify:
        check = CArchiveReader(str(target))
        assert check.options == archive.options
        for name in archive.toc:
            assert check.extract(name) == replacements.get(name, archive.extract(name))
        assert result[:len(executable)] == executable
    return result


def tar_bytes(files):
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode='w') as tf:
        for name, data in files.items():
            info = tarfile.TarInfo(name)
            info.size, info.mode, info.mtime = len(data), 0o644, int(time.time())
            tf.addfile(info, io.BytesIO(data))
    return lzma.compress(buf.getvalue(), preset=1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base', type=Path, required=True)
    parser.add_argument('--python311', type=Path, required=True)
    parser.add_argument('--qt-wheels', type=Path, required=True)
    args = parser.parse_args()
    WORK.mkdir(parents=True, exist_ok=True)
    qt_files, qt_links, qt_python, qt_wheels = load_qt_runtime(args.qt_wheels)
    base = read_ar(args.base)
    assert base['debian-binary'] == b'2.0\n'
    with tarfile.open(fileobj=io.BytesIO(base['control.tar.xz'])) as tf:
        control = tf.extractfile('./control').read().decode()
    assert 'Architecture: amd64' in control and 'libc6 (>= 2.36)' in control
    with tarfile.open(fileobj=io.BytesIO(base['data.tar.xz'])) as tf:
        executable = tf.extractfile('./opt/getman/Getman').read()
        desktop = tf.extractfile('./usr/share/applications/getman.desktop').read().decode()
    replacements = {
        './opt/reqon/Reqon': (refresh_executable(executable, args.python311, python_overlay=qt_python), 0o755),
        './usr/share/applications/reqon.desktop': (desktop.replace('Name=Getman\n', 'Name=Reqon\n').replace('Icon=getman\n', 'Icon=reqon\n').replace('Exec=getman\n', 'Exec=reqon\n').replace('StartupWMClass=Getman\n', 'StartupWMClass=Reqon\n').encode(), 0o644),
        './usr/share/doc/reqon/README.md': ((ROOT / 'README.md').read_bytes(), 0o644),
        './usr/bin/reqon': (b'#!/bin/sh\nexec /opt/reqon/Reqon "$@"\n', 0o755),
        './usr/bin/getman': (b'#!/bin/sh\nexec /opt/reqon/Reqon "$@"\n', 0o755),
    }
    for name, entry in qt_files.items():
        replacements['./opt/reqon/_internal/' + name] = entry
    assets = list((ROOT / 'Reqon_Icon_Pack_Approved/png/transparent').glob('*.png'))
    assets += [ROOT / 'Reqon_Icon_Pack_Approved/linux/reqon.png', ROOT / 'reqon-ui-states v2/manifest.json']
    assets += list((ROOT / 'reqon-ui-states v2/dark').glob('*.png'))
    assets += list((ROOT / 'reqon-ui-states v2/light').glob('*.png'))
    for path in assets:
        replacements['./opt/reqon/_internal/' + path.relative_to(ROOT).as_posix()] = (path.read_bytes(), 0o644)
    for size in (16, 24, 32, 48, 64, 128, 256, 512):
        replacements[f'./usr/share/icons/hicolor/{size}x{size}/apps/reqon.png'] = ((ROOT / f'Reqon_Icon_Pack_Approved/png/transparent/reqon-{size}.png').read_bytes(), 0o644)
    output_tar = WORK / 'data.tar.xz'
    seen, expected, native = set(), {}, {}
    installed_size = 0
    with tarfile.open(fileobj=io.BytesIO(base['data.tar.xz'])) as source, lzma.open(output_tar, 'wb', preset=1) as compressed, tarfile.open(fileobj=compressed, mode='w|', format=tarfile.GNU_FORMAT) as target:
        for info in source:
            assert not info.name.startswith('/') and '..' not in Path(info.name).parts
            if info.name.startswith('./opt/getman/_internal/') and is_qt_runtime(info.name[len('./opt/getman/_internal/'):]):
                continue
            info.name = info.name.replace('./opt/getman', './opt/reqon').replace('/reqon/Getman', '/reqon/Reqon').replace('/applications/getman.desktop', '/applications/reqon.desktop').replace('/doc/getman', '/doc/reqon')
            if info.name.endswith('/getman.svg') or info.name in replacements:
                continue
            data = source.extractfile(info).read() if info.isfile() else None
            target.addfile(info, io.BytesIO(data) if data is not None else None)
            seen.add(info.name)
            if data is not None:
                installed_size += len(data)
                if '/opt/reqon/_internal/' in info.name:
                    native[info.name] = sha(data)
        for name, (data, mode) in replacements.items():
            parts = name.split('/')
            for i in range(2, len(parts)):
                directory = '/'.join(parts[:i])
                if directory in seen:
                    continue
                info = tarfile.TarInfo(directory)
                info.type, info.mode = tarfile.DIRTYPE, 0o755
                target.addfile(info)
                seen.add(directory)
            info = tarfile.TarInfo(name)
            info.size, info.mode, info.mtime = len(data), mode, int(time.time())
            target.addfile(info, io.BytesIO(data))
            installed_size += len(data)
            expected[name] = (sha(data), mode)
        for name, destination in qt_links.items():
            info = tarfile.TarInfo('./opt/reqon/_internal/' + name)
            info.type, info.mode, info.linkname = tarfile.SYMTYPE, 0o777, destination
            target.addfile(info)
    control = re.sub(r'^Version:.*$', 'Version: ' + VERSION, control, flags=re.M)
    control = re.sub(r'^Installed-Size:.*$', 'Installed-Size: ' + str((installed_size + 1023) // 1024), control, flags=re.M)
    control = control.replace('Package: getman', 'Package: reqon\nConflicts: getman\nReplaces: getman\nProvides: getman').replace('Maintainer: Getman', 'Maintainer: Reqon').replace('Description: Local native', 'Description: Reqon local native')
    package = ROOT / f'dist/reqon_{VERSION}_astra1.8_amd64.deb'
    payloads = {'debian-binary': b'2.0\n', 'control.tar.xz': tar_bytes({'./control': control.encode()}), 'data.tar.xz': output_tar.read_bytes()}
    with package.open('wb') as fp:
        fp.write(b'!<arch>\n')
        for name, data in payloads.items():
            header = f'{name + "/":<16}{int(time.time()):<12}{0:<6}{0:<6}{"100644":<8}{len(data):<10}`\n'.encode()
            assert len(header) == 60
            fp.write(header); fp.write(data)
            if len(data) % 2:
                fp.write(b'\n')
    # Read back the actual deliverable, including unchanged native runtime files.
    verify = read_ar(package)
    with tarfile.open(fileobj=io.BytesIO(verify['data.tar.xz'])) as tf:
        for info in tf:
            if info.name in expected:
                digest, mode = expected.pop(info.name)
                assert info.mode == mode and sha(tf.extractfile(info).read()) == digest, info.name
            if info.name in native:
                assert sha(tf.extractfile(info).read()) == native.pop(info.name), info.name
    assert not expected and not native
    digest = sha(package.read_bytes())
    package.with_suffix('.deb.sha256').write_text(digest + '  ' + package.name + '\n', encoding='ascii')
    report = {'version': VERSION, 'platform': 'Astra Linux 1.8 amd64', 'build_mode': 'incremental native package refresh',
              'base_package': args.base.name, 'base_sha256': sha(args.base.read_bytes()), 'package': package.name,
              'sha256': digest, 'bytes': package.stat().st_size, 'python_bytecode': '3.11',
              'native_runtime': 'Base Linux ELF/Python/gRPC with matching Qt/PySide6/Shiboken 6.11.3 wheels',
              'qt_version': QT_VERSION, 'qt_wheels': qt_wheels,
              'validation': ['All application modules compiled/read back with Python 3.11', 'Embedded archive and native runtime hashes verified', 'DEB archive, icons and permissions verified'],
              'linux_launch_verified': False, 'linux_launch_limitation': 'Docker cannot start: virtualization unavailable on build host',
              'source_sha256': {p.name: sha(p.read_bytes()) for p in sorted(ROOT.glob('*.py'))}}
    (ROOT / f'dist/Reqon-{VERSION}-linux-build-info.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print('Created:', package, '\nSHA256:', digest, flush=True)


if __name__ == '__main__':
    main()
