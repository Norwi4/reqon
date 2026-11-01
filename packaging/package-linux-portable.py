"""Archive the native Linux bundle produced by build-deb.sh."""
import io
from pathlib import Path
import re
import sys
import tarfile

root=Path(__file__).resolve().parents[1]
version=sys.argv[1]
if not re.fullmatch(r'\d+\.\d+\.\d+',version):raise SystemExit('Expected x.y.z version')
bundle=root/'dist/linux/Reqon'
if sys.platform!='linux' or not (bundle/'Reqon').is_file():
    raise SystemExit('Build the native Linux bundle first')
target=root/f'dist/Reqon-{version}-linux-x86_64.tar.gz'
with tarfile.open(target,'w:gz') as archive:
    archive.add(bundle,arcname='Reqon')
    for name,mode,content in [
        ('reqon',0o755,'#!/bin/sh\ncd -- "$(dirname -- "$0")" || exit 1\nexec ./Reqon "$@"\n'),
        ('README.txt',0o644,f'Reqon {version}\n\nRun ./reqon from this folder.\nRequires Linux x86-64, glibc >= 2.36 and an X11/Wayland desktop.\nKeep _internal next to the executable.\nWorkspace data: $XDG_DATA_HOME/Reqon or ~/.local/share/Reqon.\n'),
    ]:
        data=content.encode();info=tarfile.TarInfo('Reqon/'+name);info.mode=mode;info.size=len(data)
        archive.addfile(info,io.BytesIO(data))
print(target)
