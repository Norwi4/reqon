"""Collect workspace-level license files omitted from published crate archives."""
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from urllib.parse import urlparse
from urllib.request import Request, urlopen
import json, shutil

root = Path(__file__).resolve().parents[1]
inventory_path = root / 'docs/dependencies.json'
inventory = json.loads(inventory_path.read_text(encoding='utf-8'))
registry = root / '.cargo-home/registry/src'
groups = {}
for entry in inventory:
    existing = root / 'assets/licenses/rust' / f"{entry['name']}-{entry['version']}"
    entry['notices'] = sorted({*entry['notices'], *(p.relative_to(root).as_posix() for p in existing.rglob('*') if p.is_file())})
    if entry['notices']:
        continue
    source = next(registry.glob(f"*/{entry['name']}-{entry['version']}"))
    if not (source / '.cargo_vcs_info.json').exists():
        continue
    info = json.loads((source / '.cargo_vcs_info.json').read_text())
    repo = urlparse(entry['repository'])
    parts = repo.path.strip('/').removesuffix('.git').split('/')[:2]
    key = (repo.netloc, '/'.join(parts), info['git']['sha1'])
    groups.setdefault(key, []).append(entry)

def read(url):
    with urlopen(Request(url, headers={'User-Agent': 'Reqon-license-inventory'}), timeout=10) as response:
        return response.read()

def collect(group):
    (host, repo, commit), entries = group
    files = []
    try:
        if host == 'github.com':
            listing = json.loads(read(f'https://api.github.com/repos/{repo}/contents?ref={commit}'))
            candidates = [(item['name'], item.get('download_url')) for item in listing
                          if item.get('type') == 'file' and any(token in item['name'].upper() for token in ['LICENSE', 'LICENCE', 'COPYING', 'NOTICE'])]
        elif host == 'codeberg.org':
            candidates = [(name, f'https://codeberg.org/{repo}/raw/commit/{commit}/{name}')
                          for name in ['LICENSE-MIT', 'LICENSE-APACHE', 'LICENSE', 'LICENSE.md']]
        else:
            candidates = []
        for name, url in candidates:
            try:
                if url:
                    files.append((name, read(url)))
            except Exception:
                pass
        for entry in entries:
            for name, data in files:
                target = root / 'assets/licenses/rust' / f"{entry['name']}-{entry['version']}" / name
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(data)
                entry['notices'].append(target.relative_to(root).as_posix())
            entry['upstream_revision'] = commit
    except Exception as error:
        print(f'{repo}: {error}', flush=True)
    print(f'{repo}: {len(files)} upstream notices', flush=True)

with ThreadPoolExecutor(max_workers=8) as pool:
    list(pool.map(collect, groups.items()))

for entry in inventory:
    candidates = list(registry.glob(f"*/{entry['name']}-{entry['version']}"))
    if not candidates:
        continue
    source = candidates[0]
    if not entry['notices'] and 'github.com/emilk/egui' in entry['repository']:
        entry['notices'] = ['assets/licenses/egui-LICENSE-MIT', 'assets/licenses/egui-LICENSE-APACHE']
        entry['notice_source'] = 'Shared egui upstream notices; crate archive omits workspace root licenses'
    for path in source.rglob('*'):
        license_directory = 'LICENSES' in [part.upper() for part in path.relative_to(source).parts]
        font_notice = entry['name'] == 'epaint_default_fonts' and any(word in path.name.lower() for word in ['ofl', 'licence', 'license', 'copyright'])
        if path.is_file() and (license_directory or font_notice):
            target = root / 'assets/licenses/rust' / f"{entry['name']}-{entry['version']}" / path.relative_to(source)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(path, target)
            notice = target.relative_to(root).as_posix()
            if notice not in entry['notices']:
                entry['notices'].append(notice)
    if entry['name'].startswith('enum-map'):
        lines = set()
        for path in source.rglob('*'):
            if path.is_file() and path.suffix in ['.rs', '.md']:
                lines.update(line.strip().removeprefix('// ').strip() for line in path.read_text(encoding='utf-8').splitlines() if 'SPDX-FileCopyrightText:' in line or 'SPDX-License-Identifier:' in line)
        target = root / 'assets/licenses/rust' / f"{entry['name']}-{entry['version']}" / 'NOTICE-SPDX.txt'
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text('\n'.join(sorted(lines)) + '\n', encoding='utf-8')
        entry['notices'].append(target.relative_to(root).as_posix())
inventory_path.write_text(json.dumps(inventory, indent=2), encoding='utf-8')
print('Entries without a separate notice:', [entry['name'] for entry in inventory if not entry['notices']])
