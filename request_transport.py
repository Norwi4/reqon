"""Request preparation shared by the network clients and command snippets."""
import base64
import json
from dataclasses import dataclass
from functools import lru_cache
from pathlib import Path
import re
import ssl
import gzip
import zlib
import urllib.request
from urllib.parse import urlencode, urlsplit, urlunsplit,quote
from ui_brand import USE_REQON_UI
if USE_REQON_UI:import json_comments


def expand(value, variables):
    return re.sub(r'\{\{\s*([^{}]+?)\s*\}\}', lambda m: str(variables.get(m.group(1), m.group(0))), str(value))


def parse_message(text):
    if USE_REQON_UI:return json_comments.loads(text)
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
    proxy_config: object = None
    keylog_file: str = ''

    @classmethod
    def from_data(cls, data, variables=None):
        variables=variables or {}
        def path(value):
            text=expand(value,variables).strip()
            return str(Path(text).expanduser()) if text else ''
        return cls(bool(data.get('tls',False)), *(path(data.get(key,''))
            for key in ('tls_ca_file','tls_cert_file','tls_key_file')),
            bool(data.get('tls_verify',False)),expand(data.get('tls_server_name',''),variables).strip(),data.get('_proxy_config'),data.get('_tls_keylog',''))


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
    auth=data.get('auth_type');value=expand(data.get('auth_value',''),variables)
    key='authorization';entry=None
    if auth=='Bearer Token':entry='Bearer '+value
    elif auth=='Basic Auth':entry='Basic '+base64.b64encode(value.encode()).decode()
    elif auth=='API Key':
        key,_,entry=value.partition(':');key=key.strip().lower();entry=entry.strip()
    if entry is not None:
        metadata=[(k,v) for k,v in metadata if k!=key];metadata.append((key,entry))
    return metadata


def prepare_http(data, variables,load_files=False):
    url=expand(data.get('url',''),variables).strip()
    if not url:raise ValueError('Enter a request URL to generate a command.')
    if '://' not in url:url='https://'+url
    parts=urlsplit(url)
    if parts.scheme not in ('http','https') or not parts.netloc:raise ValueError('Enter a valid HTTP or HTTPS URL.')
    pairs=[(expand(k,variables),expand(v,variables)) for k,v in active_rows(data,'params','param_rows')]
    original_pairs=[list(row[:2]) for row in active_rows(data,'params','param_rows')]
    encode=data.get('http_encode_url',True)
    encoded=expand(data['imported_query'],variables) if 'imported_query' in data and original_pairs==data.get('imported_query_params') else urlencode(pairs) if encode else '&'.join(k+'='+v for k,v in pairs)
    query='&'.join(part for part in (parts.query,encoded) if part)
    url=urlunsplit((parts.scheme,parts.netloc,parts.path,query,''))
    headers={}
    def header(key,value,default=False):
        existing=next((name for name in headers if name.lower()==key.lower()),None)
        if default and existing is not None:return
        if existing is not None:del headers[existing]
        headers[key]=value
    for k,v in active_rows(data,'headers','header_rows'):header(expand(k,variables),expand(v,variables))
    manual_names={name.lower() for name in headers}
    auth=data.get('auth_type','No Auth'); value=expand(data.get('auth_value',''),variables)
    if auth=='Bearer Token':header('Authorization','Bearer '+value)
    elif auth=='Basic Auth':header('Authorization','Basic '+base64.b64encode(value.encode()).decode())
    elif auth=='API Key':
        key,_,secret=value.partition(':')
        if data.get('api_key_location')=='Query Params':
            query+='&' if query else '';query+=urlencode({key.strip():secret.strip()}) if encode else key.strip()+'='+secret.strip()
        else:header(key.strip(),secret.strip())
    elif auth=='JWT Bearer':
        from http_features import jwt_token
        token=jwt_token(data,variables)
        if data.get('jwt_location','Request Header')=='Query Param':
            key=expand(data.get('jwt_query_name','token'),variables)
            query+='&' if query else '';query+=urlencode({key:token}) if encode else key+'='+token
        else:header('Authorization',(expand(data.get('jwt_prefix','Bearer'),variables)+' '+token).strip())
    path=quote(parts.path,safe="/%:@!$&'()*+,;=-._~") if encode else parts.path
    if encode:query=quote(query,safe="%/?@!$&'()*+,;=:-._~")
    url=urlunsplit((parts.scheme,parts.netloc,path,query,''))
    body=None
    if data.get('body_type')=='raw':
        text=expand(data.get('body',''),variables);original=text
        if USE_REQON_UI:
            media_type=next((value.split(';',1)[0].strip().lower() for key,value in headers.items() if key.lower()=='content-type'),'')
            if media_type in ('application/json','text/json') or media_type.endswith('+json') or not media_type and (data.get('raw_language')=='JSON' or 'raw_language' not in data and json_comments.looks_like_json(text)):
                try:text=json_comments.normalize(text)
                except json.JSONDecodeError:
                    if data.get('_general') is None or data['_general'].request_validation:raise
        body=text.encode('utf-8')
        if text!=original and any(key.lower()=='content-length' for key in headers):header('Content-Length',str(len(body)))
        from http_features import RAW_TYPES
        header('Content-Type',RAW_TYPES.get(data.get('raw_language'),'application/json' if body.lstrip().startswith((b'{',b'[')) else 'text/plain; charset=utf-8'),True)
    elif data.get('body_type')=='x-www-form-urlencoded':
        body=urlencode([(expand(k,variables),expand(v,variables)) for k,v in active_rows(data,'form','form_rows')]).encode()
        header('Content-Type','application/x-www-form-urlencoded',True)
    elif data.get('body_type') in ('file','binary'):
        header('Content-Type','application/octet-stream',True)
        if load_files:
            from http_features import read_body_file
            body=read_body_file(data.get('body_file',''),data,variables).read_bytes()
    elif data.get('body_type')=='form-data':
        from http_features import multipart_body
        body,media=multipart_body(data,variables,load_files)
        # The boundary belongs to this serialization and must match the body.
        header('Content-Type',media)
    elif data.get('body_type')=='GraphQL':
        query_text=expand(data.get('graphql_query',''),variables)
        values=parse_message(expand(data.get('graphql_variables','') or '{}',variables))
        if not isinstance(values,dict):raise ValueError('GraphQL variables must be a JSON object.')
        body=json.dumps({'query':query_text,'variables':values},ensure_ascii=False).encode('utf-8');header('Content-Type','application/json',True)
    disabled={name.lower() for name in data.get('disabled_headers',[])}
    headers={name:value for name,value in headers.items() if name.lower() not in disabled or name.lower() in manual_names}
    return url,data.get('method','GET'),headers,body


@dataclass(frozen=True)
class HTTPOptions:
    verify: bool = True
    follow_redirects: bool = True
    timeout: float = 30
    ca_file: str = ''
    cert_file: str = ''
    key_file: str = ''

    @classmethod
    def from_data(cls,data,variables=None):
        import math
        timeout=float(data.get('http_timeout',30))
        if not math.isfinite(timeout) or not 0<=timeout<=2147483.647:
            raise ValueError('HTTP timeout must be between 0 and 2147483.647 seconds.')
        return cls(bool(data.get('http_verify',True)),bool(data.get('http_follow_redirects',True)),timeout,
                   *(expand(data.get(key,''),variables or {}).strip() for key in ('http_ca_file','http_cert_file','http_key_file')))


def open_http(request,data,variables,proxy_config=None):
    from proxy_http import RoutingHandler,HTTPHandler,HTTPSHandler
    options=HTTPOptions.from_data(data,variables)
    context=ssl.create_default_context() if options.verify else ssl._create_unverified_context()
    if options.ca_file and options.verify:context.load_verify_locations(cafile=str(Path(options.ca_file).expanduser()))
    load_client_identity(context,options)
    handlers=[RoutingHandler(proxy_config),HTTPHandler(),HTTPSHandler(context=context)]
    class Redirects(urllib.request.HTTPRedirectHandler):
        def redirect_request(self,req,fp,code,msg,headers,newurl):
            if not options.follow_redirects:return None
            redirected=super().redirect_request(req,fp,code,msg,headers,newurl)
            def origin(url):
                parts=urlsplit(url);return parts.scheme,parts.hostname,parts.port or (443 if parts.scheme=='https' else 80)
            if redirected and origin(req.full_url)!=origin(newurl):
                for name in ('Authorization','Cookie','Proxy-authorization'):redirected.remove_header(name)
            return redirected
    handlers.append(Redirects())
    return urllib.request.build_opener(*handlers).open(request,timeout=options.timeout or None)


def decode_http_content(content,encoding):
    for name in reversed([part.strip().lower() for part in encoding.split(',') if part.strip()]):
        if name=='gzip':content=gzip.decompress(content)
        elif name=='deflate':
            try:content=zlib.decompress(content)
            except zlib.error:content=zlib.decompress(content,-zlib.MAX_WBITS)
        elif name!='identity':raise ValueError(f'Unsupported response encoding: {name}. Use Accept-Encoding: gzip, deflate or identity.')
    return content
