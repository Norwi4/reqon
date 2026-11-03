#!/usr/bin/env bash
set -euo pipefail
cd /src
apt-get update
apt-get install -y xdotool /out/reqon_0.19.0_astra1.8_amd64.deb
xvfb-run -a -s '-screen 0 1600x1000x24' sh -c 'openbox >/tmp/reqon-http-openbox.log 2>&1 & exec "$@"' sh \
    /opt/reqon-build-env/bin/python tests/smoke_packaged_linux_http.py
REQON_TEST_RESIZE=1 xvfb-run -a -s '-screen 0 1600x1000x24' sh -c 'openbox >/tmp/reqon-resize-openbox.log 2>&1 & exec "$@"' sh \
    /opt/reqon-build-env/bin/python tests/smoke_packaged_linux.py
