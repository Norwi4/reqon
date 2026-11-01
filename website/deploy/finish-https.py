#!/usr/bin/python3
"""Enable HTTPS after public DNS points to this VDS; no credentials required."""
import ipaddress
from pathlib import Path
import re
import shutil
import subprocess
import sys

DOMAIN = 'reqon.ru'
SERVER_IP = '168.113.178.77'
CONFIG = Path('/etc/nginx/conf.d/reqon-site.conf')
CERT = Path('/etc/letsencrypt/live/reqon.ru/fullchain.pem')

def run(*args):
    return subprocess.run(args, text=True, check=True, capture_output=True).stdout

def addresses(resolver, hostname, record):
    answer = run('dig', '@' + resolver, hostname, record, '+short', '+time=3', '+tries=1')
    result = set()
    for line in answer.splitlines():
        try:
            result.add(str(ipaddress.ip_address(line.strip())))
        except ValueError:
            pass
    return result

def ready(hostname):
    for resolver in ('1.1.1.1', '8.8.8.8'):
        if addresses(resolver, hostname, 'A') != {SERVER_IP} or addresses(resolver, hostname, 'AAAA'):
            return False
    return True

def main():
    if not ready(DOMAIN):
        print('Waiting for public DNS: reqon.ru must point to ' + SERVER_IP, flush=True)
        return
    domains = [DOMAIN]
    if ready('www.' + DOMAIN):
        domains.append('www.' + DOMAIN)
    if '--check-only' in sys.argv:
        print('DNS ready: ' + ', '.join(domains))
        return
    has_certificate = False
    if CERT.exists():
        san = run('openssl', 'x509', '-in', str(CERT), '-noout', '-ext', 'subjectAltName')
        certificate_names = set(re.findall(r'DNS:([^,\s]+)', san))
        has_certificate = set(domains).issubset(certificate_names)
    if not has_certificate:
        command = ['certbot', 'certonly', '--webroot', '-w', '/var/lib/reqon-acme',
                   '--cert-name', DOMAIN, '--non-interactive', '--agree-tos',
                   '--register-unsafely-without-email', '--expand', '--keep-until-expiring']
        for domain in domains:
            command.extend(['-d', domain])
        subprocess.run(command, check=True)
    www_https = ''
    if 'www.' + DOMAIN in domains:
        www_https = '''
server {
    listen 443 ssl;
    listen [::]:443 ssl;
    server_name www.reqon.ru;
    ssl_certificate /etc/letsencrypt/live/reqon.ru/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/reqon.ru/privkey.pem;
    return 301 https://reqon.ru$request_uri;
}
'''
    config = '''server {
    listen 80;
    listen [::]:80;
    server_name reqon.ru www.reqon.ru;
    location ^~ /.well-known/acme-challenge/ {
        root /var/lib/reqon-acme;
        default_type text/plain;
        try_files $uri =404;
    }
    location / { return 301 https://reqon.ru$request_uri; }
}
server {
    listen 443 ssl;
    listen [::]:443 ssl;
    server_name reqon.ru;
    ssl_certificate /etc/letsencrypt/live/reqon.ru/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/reqon.ru/privkey.pem;
    access_log /var/log/nginx/reqon-access.log;
    error_log /var/log/nginx/reqon-error.log;
    gzip on;
    gzip_vary on;
    gzip_types text/css application/javascript application/json image/svg+xml;
    location / {
        proxy_pass http://127.0.0.1:4173;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
''' + www_https
    if CONFIG.read_text() != config:
        backup = CONFIG.with_suffix('.conf.before-https')
        shutil.copy2(CONFIG, backup)
        CONFIG.write_text(config)
        try:
            subprocess.run(['nginx', '-t'], check=True)
        except subprocess.CalledProcessError:
            shutil.copy2(backup, CONFIG)
            raise
        subprocess.run(['systemctl', 'reload', 'nginx'], check=True)
    print('HTTPS enabled: ' + ', '.join(domains), flush=True)
    if len(domains) == 2:
        subprocess.run(['systemctl', 'disable', '--now', 'reqon-https-bootstrap.timer'], check=True)

if __name__ == '__main__':
    main()
