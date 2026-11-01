#!/bin/sh
# Reviewed for the existing Ubuntu 24.04 / Nginx server at 168.113.178.77.
set -eu
test "$(id -u)" = 0
test -f /opt/reqon/website/server.mjs
NODE_VERSION=v22.23.4
NODE_SHA256=1805bcd35e6815559bd2eb97db438d3fdb0eb3a5e6a977b7c1badca76337e9ce
apt-get update -qq
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends certbot xz-utils ca-certificates
if [ ! -x /opt/reqon/node/bin/node ]; then
    install -d /opt/reqon/node /opt/reqon/downloads
    curl --fail --show-error --location --retry 3 "https://nodejs.org/dist/${NODE_VERSION}/node-${NODE_VERSION}-linux-x64.tar.xz" -o /opt/reqon/downloads/node.tar.xz
    printf '%s  %s\n' "$NODE_SHA256" /opt/reqon/downloads/node.tar.xz | sha256sum --check
    tar -xJf /opt/reqon/downloads/node.tar.xz -C /opt/reqon/node --strip-components=1
fi
/opt/reqon/node/bin/node --version
id reqon >/dev/null 2>&1 || useradd --system --home /opt/reqon --shell /usr/sbin/nologin reqon
install -d -o reqon -g reqon -m 750 /opt/reqon/website/.cache
install -d -m 755 /var/lib/reqon-acme/.well-known/acme-challenge
cd /opt/reqon/website
SITE_URL=https://reqon.ru /opt/reqon/node/bin/node src/build.mjs
install -m 644 deploy/reqon-website.service /etc/systemd/system/reqon-website.service
install -m 644 deploy/reqon-https-bootstrap.service /etc/systemd/system/reqon-https-bootstrap.service
install -m 644 deploy/reqon-https-bootstrap.timer /etc/systemd/system/reqon-https-bootstrap.timer
if [ ! -f /etc/nginx/conf.d/reqon-site.conf ]; then
    install -m 644 deploy/nginx-http.conf /etc/nginx/conf.d/reqon-site.conf
fi
install -d /etc/letsencrypt/renewal-hooks/deploy
install -m 755 deploy/reload-nginx.sh /etc/letsencrypt/renewal-hooks/deploy/reqon-reload-nginx
systemctl daemon-reload
systemctl enable --now reqon-website.service
nginx -t
systemctl reload nginx
systemctl enable --now certbot.timer reqon-https-bootstrap.timer
systemctl start reqon-https-bootstrap.service
systemctl --no-pager --full status reqon-website.service
curl --fail --show-error http://127.0.0.1:4173/healthz
