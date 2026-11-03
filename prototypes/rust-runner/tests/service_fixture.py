"""Local HTTP/TLS/proxy and PostgreSQL fixtures shared with the gRPC service."""
import base64
import datetime
import gzip
import ipaddress
import json
import select
import socket
import socketserver
import ssl
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlsplit

import psycopg
from psycopg import sql
from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.x509.oid import NameOID, ExtendedKeyUsageOID
from manage_postgres import ensure_postgres


class PostgresStore:
    def __init__(self):
        self.connection = psycopg.connect(ensure_postgres(), autocommit=True)
        self.schema = 'reqon_fixture_' + uuid.uuid4().hex
        self.table = sql.Identifier(self.schema, 'objects')
        self.lock = threading.Lock()
        self.connection.execute(sql.SQL('CREATE SCHEMA {}').format(sql.Identifier(self.schema)))
        self.connection.execute(sql.SQL('CREATE TABLE {} (id BIGSERIAL PRIMARY KEY, name TEXT NOT NULL, polls INT NOT NULL DEFAULT 0)').format(self.table))

    def create(self, name):
        with self.lock:
            row = self.connection.execute(sql.SQL('INSERT INTO {} (name) VALUES (%s) RETURNING id').format(self.table), (name,)).fetchone()
            return {'id': f'object-{row[0]}', 'status': 'DRAFT', 'count': 0}

    def get(self, identifier):
        try:
            number = int(identifier.removeprefix('object-'))
        except ValueError:
            return None
        with self.lock:
            row = self.connection.execute(sql.SQL('UPDATE {} SET polls=polls+1 WHERE id=%s RETURNING name,polls').format(self.table), (number,)).fetchone()
            if row is None:
                return None
            return {'id': identifier, 'status': 'CREATED' if row[1] >= 3 and row[0] != 'never' else 'DRAFT', 'count': row[1]}

    def count(self):
        with self.lock:
            return self.connection.execute(sql.SQL('SELECT count(*) FROM {}').format(self.table)).fetchone()[0]

    def close(self):
        # Only the unique schema created by this fixture is ever removed.
        self.connection.execute(sql.SQL('DROP SCHEMA {} CASCADE').format(sql.Identifier(self.schema)))
        self.connection.close()


def certificates(directory):
    directory = Path(directory)
    directory.mkdir(parents=True, exist_ok=True)
    now = datetime.datetime.now(datetime.timezone.utc)
    ca_key = ec.generate_private_key(ec.SECP256R1())
    subject = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, 'Reqon temporary fixture CA')])
    ca = (x509.CertificateBuilder().subject_name(subject).issuer_name(subject).public_key(ca_key.public_key())
          .serial_number(x509.random_serial_number()).not_valid_before(now-datetime.timedelta(days=1)).not_valid_after(now+datetime.timedelta(days=7))
          .add_extension(x509.BasicConstraints(ca=True, path_length=0), critical=True).sign(ca_key, hashes.SHA256()))
    result = {}
    for kind in ('server', 'client'):
        key = ec.generate_private_key(ec.SECP256R1())
        cert = (x509.CertificateBuilder().subject_name(x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, 'localhost' if kind == 'server' else 'reqon-client')]))
                .issuer_name(subject).public_key(key.public_key()).serial_number(x509.random_serial_number())
                .not_valid_before(now-datetime.timedelta(days=1)).not_valid_after(now+datetime.timedelta(days=7))
                .add_extension(x509.SubjectAlternativeName([x509.DNSName('localhost'), x509.IPAddress(ipaddress.ip_address('127.0.0.1'))]), critical=False)
                .add_extension(x509.ExtendedKeyUsage([ExtendedKeyUsageOID.SERVER_AUTH if kind == 'server' else ExtendedKeyUsageOID.CLIENT_AUTH]), critical=False)
                .sign(ca_key, hashes.SHA256()))
        result[kind+'_cert'] = cert.public_bytes(serialization.Encoding.PEM)
        result[kind+'_key'] = key.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8, serialization.NoEncryption())
    result['ca'] = ca.public_bytes(serialization.Encoding.PEM)
    for name, data in result.items():
        path = directory / (name + '.pem')
        path.write_bytes(data)
        path.chmod(0o600)
    return result


class HttpFixture:
    def __init__(self, store=None, tls_dir=None, mtls=False):
        self.store = store
        self.calls = []
        self.ports = set()
        self.slow_started = threading.Event()
        self.slow_cancelled = threading.Event()
        fixture = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = 'HTTP/1.1'

            def setup(self):
                super().setup()
                self.connection.setsockopt(socket.IPPROTO_TCP,socket.TCP_NODELAY,1)

            def handle(self):
                try:
                    super().handle()
                except (ConnectionResetError, BrokenPipeError, ssl.SSLError):
                    fixture.slow_cancelled.set()

            def log_message(self, *_):
                pass

            def read_body(self):
                if self.headers.get('transfer-encoding') == 'chunked':
                    body = bytearray()
                    while True:
                        size = int(self.rfile.readline().split(b';')[0], 16)
                        if size == 0:
                            self.rfile.readline()
                            return bytes(body)
                        body.extend(self.rfile.read(size))
                        self.rfile.read(2)
                        if len(body) > 32*1024*1024:
                            raise ValueError('fixture body limit')
                return self.rfile.read(int(self.headers.get('content-length', 0)))

            def reply(self, value, status=200, headers=(), raw=False):
                payload = value if raw else json.dumps(value, ensure_ascii=False).encode()
                self.send_response(status)
                self.send_header('content-type', 'application/octet-stream' if raw else 'application/json')
                self.send_header('content-length', str(len(payload)))
                for key, val in headers:
                    self.send_header(key, val)
                self.end_headers()
                if self.command != 'HEAD':
                    try:
                        self.wfile.write(payload)
                    except (BrokenPipeError, ConnectionResetError, ssl.SSLError):
                        fixture.slow_cancelled.set()

            def do_any(self):
                target = urlsplit(self.path)
                query = parse_qs(target.query, keep_blank_values=True)
                body = self.read_body()
                headers = {k.lower(): v for k, v in self.headers.items()}
                fixture.calls.append((self.command, target.path, headers, body))
                fixture.ports.add(self.client_address[1])
                if target.path == '/objects' and self.command == 'POST':
                    return self.reply(fixture.store.create(json.loads(body)['name']), 201)
                if target.path.startswith('/objects/'):
                    value = fixture.store.get(target.path.rsplit('/',1)[-1])
                    return self.reply(value or {'error':'missing'}, 200 if value else 404)
                if target.path == '/redirect':
                    return self.reply({}, int(query.get('status',['302'])[0]), [('location',query.get('to',['/echo'])[0])])
                if target.path == '/cookies/set':
                    return self.reply({'ok':True}, headers=[('set-cookie','session=fixture; Path=/; HttpOnly')])
                if target.path == '/gzip':
                    payload = gzip.compress(json.dumps({'value':'x'*int(query.get('size',['20'])[0])}).encode())
                    return self.reply(payload, headers=[('content-encoding','gzip')], raw=True)
                if target.path == '/binary':
                    return self.reply(bytes([0,255,128,10]), raw=True)
                if target.path == '/large':
                    return self.reply(b'x'*int(query.get('size',['1048576'])[0]), raw=True)
                if target.path == '/slow':
                    fixture.slow_started.set()
                    end = time.monotonic()+int(query.get('ms',['5000'])[0])/1000
                    while time.monotonic() < end:
                        if not isinstance(self.connection, ssl.SSLSocket):
                            readable,_,_ = select.select([self.connection],[],[],0.01)
                            if readable and not self.connection.recv(1):
                                fixture.slow_cancelled.set()
                                return
                        else:
                            time.sleep(.01)
                code = int(target.path.rsplit('/',1)[-1]) if target.path.startswith('/status/') else 200
                try:
                    decoded = json.loads(body) if body else None
                except (ValueError, UnicodeError):
                    decoded = body.decode(errors='replace')
                return self.reply({'method':self.command,'path':target.path,'query':query,'headers':headers,'body':decoded,'binary':base64.b64encode(body).decode(),'nullValue':None}, code, [('x-multi','one'),('x-multi','two')])

            do_GET = do_POST = do_PUT = do_PATCH = do_DELETE = do_OPTIONS = do_HEAD = do_any

        self.server = ThreadingHTTPServer(('127.0.0.1',0), Handler)
        self.server.daemon_threads = True
        scheme = 'http'
        if tls_dir:
            tls_dir=Path(tls_dir)
            context=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
            context.load_cert_chain(tls_dir/'server_cert.pem',tls_dir/'server_key.pem')
            if mtls:
                context.load_verify_locations(tls_dir/'ca.pem')
                context.verify_mode=ssl.CERT_REQUIRED
            self.server.socket=context.wrap_socket(self.server.socket,server_side=True)
            scheme='https'
        self.endpoint=f'{scheme}://127.0.0.1:{self.server.server_port}'
        self.thread=threading.Thread(target=self.server.serve_forever,daemon=True)
        self.thread.start()

    def close(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(2)


class ProxyFixture:
    def __init__(self, authorization=None):
        fixture=self
        self.calls=[]
        class Handler(socketserver.BaseRequestHandler):
            def handle(self):
                data=b''
                while not data.endswith(b'\r\n\r\n') and len(data)<16384:
                    byte=self.request.recv(1)
                    if not byte: return
                    data+=byte
                lines=data.decode('iso-8859-1').split('\r\n')
                method,target,version=lines[0].split()
                headers=dict(line.split(': ',1) for line in lines[1:] if ': ' in line)
                auth=next((v for k,v in headers.items() if k.lower()=='proxy-authorization'),None)
                fixture.calls.append((method,target,auth))
                if authorization is not None and auth!=authorization:
                    self.request.sendall(b'HTTP/1.1 407 Proxy Authentication Required\r\nContent-Length: 0\r\nConnection: close\r\n\r\n')
                    return
                if method=='CONNECT':
                    host,port=target.rsplit(':',1)
                    upstream=socket.create_connection((host.strip('[]'),int(port)),timeout=5)
                    self.request.sendall(b'HTTP/1.1 200 Connection Established\r\n\r\n')
                else:
                    uri=urlsplit(target)
                    upstream=socket.create_connection((uri.hostname,uri.port or 80),timeout=5)
                    header_lines=[f'{k}: {v}' for k,v in headers.items() if k.lower() not in ('proxy-authorization','connection','proxy-connection')]
                    origin=(uri.path or '/')+('?' + uri.query if uri.query else '')
                    upstream.sendall((f'{method} {origin} {version}\r\n'+'\r\n'.join(header_lines)+'\r\nConnection: close\r\n\r\n').encode('iso-8859-1'))
                try:
                    while True:
                        ready,_,_=select.select([self.request,upstream],[],[],5)
                        if not ready:return
                        for src in ready:
                            chunk=src.recv(65536)
                            if not chunk:return
                            (upstream if src is self.request else self.request).sendall(chunk)
                except (OSError,ConnectionError):
                    pass
                finally:
                    upstream.close()
        class Server(socketserver.ThreadingTCPServer):
            allow_reuse_address=True
            daemon_threads=True
        self.server=Server(('127.0.0.1',0),Handler)
        self.endpoint=f'http://127.0.0.1:{self.server.server_address[1]}'
        self.thread=threading.Thread(target=self.server.serve_forever,daemon=True)
        self.thread.start()

    def close(self):
        self.server.shutdown(); self.server.server_close(); self.thread.join(2)
