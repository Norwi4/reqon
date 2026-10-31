#!/usr/bin/env bash
set -euo pipefail
cd /src
version="${GETMAN_VERSION:-0.11.0}"
bash packaging/build-deb.sh "$version"
python_bin=/opt/getman-build-env/bin/python
xvfb-run -a -s '-screen 0 1600x1000x24' sh -c 'openbox >/tmp/getman-openbox.log 2>&1 & exec "$@"' sh \
    "$python_bin" -X faulthandler -m unittest discover -s tests -p "${GETMAN_TEST_PATTERN:-test_*.py}" -v
apt-get install -y "./dist/getman_${version}_astra1.8_amd64.deb"
xvfb-run -a -s '-screen 0 1600x1000x24' sh -c 'openbox >/tmp/getman-openbox.log 2>&1 & exec "$@"' sh \
    "$python_bin" tests/smoke_packaged_linux.py
mkdir -p /out
cp "dist/getman_${version}_astra1.8_amd64.deb" "dist/getman_${version}_linux-dependencies.txt" \
    "dist/getman_${version}_linux-build-os.txt" /out/
cd /out
sha256sum "getman_${version}_astra1.8_amd64.deb" > "getman_${version}_astra1.8_amd64.deb.sha256"
