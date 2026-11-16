"""Collect attribution from the exact locally installed Cargo.lock dependencies."""
from pathlib import Path
import json, shutil, tomllib, urllib.request

root = Path(__file__).resolve().parents[1]
dest = root / 'assets/licenses'
upstream = {
    'assertion-error-LICENSE': 'https://raw.githubusercontent.com/chaijs/assertion-error/master/LICENSE',
    'egui-LICENSE-MIT': 'https://raw.githubusercontent.com/emilk/egui/main/LICENSE-MIT',
    'egui-LICENSE-APACHE': 'https://raw.githubusercontent.com/emilk/egui/main/LICENSE-APACHE',
    'node-ntlm-client-LICENSE': 'https://raw.githubusercontent.com/elasticio/node-ntlm-client/master/LICENSE',
    'buffer-LICENSE': 'https://raw.githubusercontent.com/feross/buffer/master/LICENSE',
    'tabler-icons-LICENSE': 'https://raw.githubusercontent.com/tabler/tabler-icons/main/LICENSE',
}
for name, url in upstream.items():
    with urllib.request.urlopen(url, timeout=30) as response:
        (dest / name).write_bytes(response.read())
for name in ['MIT', 'APACHE']:
    shutil.copyfile(dest / f'egui-LICENSE-{name}', root / f'vendor/epaint/LICENSE-{name}')
registry = root / '.cargo-home/registry/src'
packages = tomllib.loads((root / 'Cargo.lock').read_text(encoding='utf-8'))['package']
inventory = []
for package in packages:
    candidates = list(registry.glob(f"*/{package['name']}-{package['version']}"))
    if package['name'] == 'epaint':
        candidates = [root / 'vendor/epaint']
    if not candidates:
        continue
    source = candidates[0]
    manifest = tomllib.loads((source / 'Cargo.toml').read_text(encoding='utf-8'))['package']
    entry = {key: manifest.get(key, '') for key in ['name', 'version', 'license', 'repository']}
    entry['notices'] = []
    for path in source.rglob('*'):
        if not path.is_file() or not (path.name.upper().startswith(('LICENSE', 'LICENCE', 'COPYING', 'NOTICE', 'OFL')) or 'LICENSES' in [part.upper() for part in path.relative_to(source).parts]) or path.stat().st_size > 300_000:
            continue
        target = dest / 'rust' / f"{package['name']}-{package['version']}" / path.relative_to(source)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, target)
        entry['notices'].append(target.relative_to(root).as_posix())
    inventory.append(entry)
(root / 'docs/dependencies.json').write_text(json.dumps(inventory, indent=2), encoding='utf-8')
print(f'Collected license inventory for {len(inventory)} locked Rust packages.')
