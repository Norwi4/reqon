"""Request preparation shared by the network clients and command snippets."""
import base64
import json
from dataclasses import dataclass
from functools import lru_cache
from pathlib import Path
import re
import ssl
from urllib.parse import urlencode, urlsplit, urlunsplit


def expand(value, variables):
    return re.sub(r'\{\{\s*([^{}]+?)\s*\}\}', lambda m: str(variables.get(m.group(1), m.group(0))), str(value))


def parse_message(text):
    # Preserve strings (URLs, escaped quotes) while removing editor-style JSON comments.
    token=r'"(?:\\.|[^"\\])*"'
    clean=re.sub(token+r'|//[^\r\n]*|/\*[\s\S]*?\*/',
                 lambda m:m.group() if m.group().startswith('"') else re.sub(r'[^\r\n]',' ',m.group()),text)
    clean=re.sub(token+r'|,(?=\s*[}\]])',lambda m:m.group() if m.group().startswith('"') else '',clean)
    return json.loads(clean)


def active_rows(data, key, saved_key=None):
    rows=data.get(saved_key, data.get(key, [])) if saved_key else data.get(key, [])
    return [(str(row[0]),str(row[1])) for row in rows if len(row)>=2 and row[0] and (len(row)<4 or row[3])]


def grpc_target(server):
    value=server.strip()
    if not value:raise ValueError('Enter a gRPC server host:port')
    if '{{' in value:raise ValueError('The server URL contains an unresolved variable.')
    if '://' in value:
        parts=urlsplit(value)
        if parts.scheme not in ('grpc','grpcs','http','https'):
            raise ValueError('Use a host:port, grpc://, grpcs://, http:// or https:// server URL.')
        if parts.path not in ('','/') or parts.query or parts.fragment or parts.username is not None:
            raise ValueError('The gRPC server URL must contain only a host and optional port.')
        value=parts.netloc
    else:value=value.rstrip('/')
    if not value or any(c.isspace() for c in value) or any(c in value for c in '/?#@'):
        raise ValueError('Enter a valid gRPC server host:port (without a path).')
    parsed=urlsplit('//'+value)
    if not parsed.hostname:raise ValueError('Enter a valid gRPC server host:port.')
    parsed.port  # Validate the optional port before passing the target to gRPC.
    return value


@dataclass(frozen=True)
class TLSOptions:
    enabled: bool = False
    ca_file: str = ''
    cert_file: str = ''
    key_file: str = ''
    verify_server: bool = False
    server_name: str = ''

    @classmethod
    def from_data(cls, data, variables=None):
        variables=variables or {}
        def path(value):
            text=expand(value,variables).strip()
            return str(Path(text).expanduser()) if text else ''
        return cls(bool(data.get('tls',False)), *(path(data.get(key,''))
            for key in ('tls_ca_file','tls_cert_file','tls_key_file')),
            bool(data.get('tls_verify',False)),expand(data.get('tls_server_name',''),variables).strip())


@dataclass(frozen=True)
class GRPCOptions:
    emit_defaults: bool = True
    max_response_mb: int = 10
    timeout_ms: int = 0

    @classmethod
    def from_data(cls,data):
        size=int(data.get('grpc_max_response_mb',10)); timeout=int(data.get('grpc_timeout_ms',0))
        if not 0<=size<=2047:raise ValueError('Maximum response message size must be between 0 and 2047 MB.')
        if not 0<=timeout<=2147483647:raise ValueError('Connection Timeout must be a non-negative number of milliseconds (up to 2147483647).')
        return cls(bool(data.get('grpc_emit_defaults',True)),size,timeout)

    @property
    def receive_limit(self):return self.max_response_mb*1024*1024 if self.max_response_mb else -1

    @property
    def timeout(self):return self.timeout_ms/1000 if self.timeout_ms else None


def load_client_identity(context,options):
    if bool(options.cert_file)!=bool(options.key_file):
        raise ValueError('Select both a client certificate and its private key for mutual TLS.')
    if options.cert_file:
        try:
            context.load_cert_chain(str(Path(options.cert_file).expanduser()),
                str(Path(options.key_file).expanduser()),password=lambda:'')
        except (OSError,ssl.SSLError) as error:
            raise ValueError('The client certificate and key must be matching PEM files; use an unencrypted private key.') from error


@lru_cache(maxsize=1)
def system_roots():
    # Python loads the Windows trust store as well as the system OpenSSL store on Linux.
    try:
        certificates=ssl.create_default_context().get_ca_certs(binary_form=True)
        return ''.join(ssl.DER_cert_to_PEM_cert(cert) for cert in certificates).encode('ascii')
    except (OSError,ssl.SSLError):return b''


def tls_credentials(options):
    import grpc
    roots=system_roots()
    def read(path,label):
        try:return Path(path).expanduser().read_bytes()
        except OSError as error:raise ValueError(f'Cannot read {label}: {path}. {error.strerror or error}') from error
    if options.ca_file:
        ca=read(options.ca_file,'CA certificate')
        try:ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT).load_verify_locations(cadata=ca.decode('ascii'))
        except (ValueError,UnicodeError,ssl.SSLError) as error:
            raise ValueError('The CA certificate must be a valid PEM certificate or PEM bundle.') from error
        roots+=b'\n'+ca
    load_client_identity(ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT),options)
    cert=key=None
    if options.cert_file:
        cert=read(options.cert_file,'client certificate'); key=read(options.key_file,'client private key')
    return grpc.ssl_channel_credentials(root_certificates=roots or None,private_key=key,certificate_chain=cert)


def grpc_metadata(data, variables):
    metadata=[(expand(k,variables).lower(),expand(v,variables)) for k,v in active_rows(data,'metadata','metadata_rows')]
    if data.get('auth_type')=='Bearer Token':
        metadata=[(k,v) for k,v in metadata if k!='authorization']
        metadata.append(('authorization','Bearer '+expand(data.get('auth_value',''),variables)))
    return metadata


def prepare_http(data, variables):
    url=expand(data.get('url',''),variables).strip()
    if not url:raise ValueError('Enter a request URL to generate a command.')
    if '://' not in url:url='https://'+url
    parts=urlsplit(url)
    if parts.scheme not in ('http','https') or not parts.netloc:raise ValueError('Enter a valid HTTP or HTTPS URL.')
    pairs=[(expand(k,variables),expand(v,variables)) for k,v in active_rows(data,'params','param_rows')]
    query='&'.join(part for part in (parts.query,urlencode(pairs)) if part)
    url=urlunsplit((parts.scheme,parts.netloc,parts.path,query,''))
    headers={}
    def header(key,value,default=False):
        existing=next((name for name in headers if name.lower()==key.lower()),None)
        if default and existing is not None:return
        if existing is not None:del headers[existing]
        headers[key]=value
    for k,v in active_rows(data,'headers','header_rows'):header(expand(k,variables),expand(v,variables))
    auth=data.get('auth_type','No Auth'); value=expand(data.get('auth_value',''),variables)
    if auth=='Bearer Token':header('Authorization','Bearer '+value)
    elif auth=='Basic Auth':header('Authorization','Basic '+base64.b64encode(value.encode()).decode())
    elif auth=='API Key':
        key,_,secret=value.partition(':'); header(key.strip(),secret.strip())
    body=None
    if data.get('body_type')=='raw':
        body=expand(data.get('body',''),variables).encode('utf-8')
        header('Content-Type','application/json' if body.lstrip().startswith((b'{',b'[')) else 'text/plain; charset=utf-8',True)
    elif data.get('body_type')=='x-www-form-urlencoded':
        body=urlencode([(expand(k,variables),expand(v,variables)) for k,v in active_rows(data,'form','form_rows')]).encode()
        header('Content-Type','application/x-www-form-urlencoded',True)
    return url,data.get('method','GET'),headers,body
