#!/usr/bin/env bash
set -euo pipefail
cd /src
version="${REQON_VERSION:-0.13.3}"
bash packaging/build-deb.sh "$version"
python_bin=/opt/reqon-build-env/bin/python
xvfb-run -a -s '-screen 0 1600x1000x24' sh -c 'openbox >/tmp/reqon-openbox.log 2>&1 & exec "$@"' sh \
    "$python_bin" -X faulthandler -m unittest discover -s tests -p "${REQON_TEST_PATTERN:-test_*.py}" -v
apt-get install -y "./dist/reqon_${version}_astra1.8_amd64.deb"
xvfb-run -a -s '-screen 0 1600x1000x24' sh -c 'openbox >/tmp/reqon-openbox.log 2>&1 & exec "$@"' sh \
    "$python_bin" tests/smoke_packaged_linux.py
mkdir -p /out
cp "dist/reqon_${version}_astra1.8_amd64.deb" "dist/reqon_${version}_linux-dependencies.txt" \
    "dist/reqon_${version}_linux-build-os.txt" /out/
cd /out
sha256sum "reqon_${version}_astra1.8_amd64.deb" > "reqon_${version}_astra1.8_amd64.deb.sha256"
