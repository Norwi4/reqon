"""HTTP/1 and HTTP/2 with local cookies, bounded downloads and per-request routing."""
import email.message
import http.cookiejar
import http.client
import json
import re
import select
import socket
import ssl
import threading
import time
import uuid
import zlib
from pathlib import Path
from urllib.parse import urlsplit,urljoin
from urllib.request import Request
import httpcore
from general_settings import resolve_file
from proxy_support import resolve_proxy,open_proxy_socket
from request_transport import prepare_http,HTTPOptions,load_client_identity,expand,parse_message


class CookieStore:
    def __init__(self,directory):
        self.lock=threading.RLock();self.jar=http.cookiejar.MozillaCookieJar(str(Path(directory)/'cookies.txt'))
        if Path(self.jar.filename).exists():
            try:self.jar.load(ignore_discard=True,ignore_expires=False)
            except (OSError,http.cookiejar.LoadError):pass
    def headers(self,request):
        with self.lock:self.jar.add_cookie_header(request)
    def receive(self,request,headers):
        class Response:
            def info(self):return headers
        with self.lock:
            self.jar.extract_cookies(Response(),request);self.jar.save(ignore_discard=True,ignore_expires=False)


class Exchange(httpcore.NetworkBackend):
    def __init__(self,deadline,proxy=None):self.deadline=deadline;self.proxy=proxy;self.lock=threading.RLock();self.sockets=set();self.closed=False;self.require_h2=False
    def remaining(self):
        if self.closed:raise ValueError('Request cancelled.')
        if self.deadline is None:return None
        remaining=self.deadline-time.monotonic()
        if remaining<=0:raise TimeoutError('Request timeout exceeded.')
        return remaining
    def connect_tcp(self,host,port,timeout=None,local_address=None,socket_options=None):
        connection=open_proxy_socket((host,port),self.proxy,self.remaining(),(local_address,0) if local_address else None)
        with self.lock:
            if self.closed:connection.close();raise ValueError('Request cancelled.')
            self.sockets.add(connection)
        for option in socket_options or []:connection.setsockopt(*option)
        return Stream(connection,self)
    def close(self):
        with self.lock:
            self.closed=True
            for connection in list(self.sockets):
                try:connection.shutdown(socket.SHUT_RDWR)
                except OSError:pass
                connection.close()
            self.sockets.clear()
    def __enter__(self):return self
    def __exit__(self,*_):self.close()


class Stream(httpcore.NetworkStream):
    def __init__(self,connection,owner):self.socket=connection;self.owner=owner
    def read(self,max_bytes,timeout=None):self.socket.settimeout(self.owner.remaining());return self.socket.recv(max_bytes)
    def write(self,buffer,timeout=None):self.socket.settimeout(self.owner.remaining());self.socket.sendall(buffer)
    def close(self):
        with self.owner.lock:self.owner.sockets.discard(self.socket);self.socket.close()
    def start_tls(self,ssl_context,server_hostname=None,timeout=None):
        self.socket.settimeout(self.owner.remaining())
        with self.owner.lock:
            old=self.socket;self.socket=ssl_context.wrap_socket(old,server_hostname=server_hostname,do_handshake_on_connect=False)
            self.owner.sockets.discard(old);self.owner.sockets.add(self.socket)
        self.socket.do_handshake()
        if self.owner.require_h2 and self.socket.selected_alpn_protocol()!='h2':raise ValueError('The server did not negotiate HTTP/2. Select Auto or HTTP/1.x in General.')
        return self
    def get_extra_info(self,info):
        if info=='ssl_object':return self.socket if isinstance(self.socket,ssl.SSLSocket) else None
        if info=='socket':return self.socket
        if info=='is_readable':return self.socket.fileno()<0 or bool(select.select([self.socket],[],[],0)[0])
        if info=='client_addr':return self.socket.getsockname()
        if info=='server_addr':return self.socket.getpeername()


def validate_schema(data,variables,config,url,body):
    if not config.request_validation:return
    content_type=next((value for key,value in data.get('_headers',{}).items() if key.lower()=='content-type'),'')
    if 'json' in content_type or data.get('http_schema'):
        payload=json.loads((body or b'null').decode('utf-8'))
    else:return
    name=expand(data.get('http_schema',''),variables)
    if not name:return
    from jsonschema import validators
    from referencing import Registry,Resource
    from referencing.jsonschema import DRAFT202012
    from referencing.exceptions import NoSuchResource
    document=json.loads(resolve_file(name,config).read_text(encoding='utf-8-sig'));schema=document
    if 'openapi' in document:
        path=urlsplit(url).path;operation=None
        for pattern,value in document.get('paths',{}).items():
            if re.fullmatch(re.sub(r'\\\{[^}]+\\\}',r'[^/]+',re.escape(pattern)),path):operation=value.get(data.get('method','GET').lower());break
        if not operation:raise ValueError('The local OpenAPI document has no matching operation for this request.')
        body_schema=operation.get('requestBody',{})
        if '$ref' in body_schema:
            body_schema=document
            for key in operation['requestBody']['$ref'].removeprefix('#/').split('/'):body_schema=body_schema[key]
        media=body_schema.get('content',{});entry=media.get(content_type.split(';')[0]) or media.get('application/json',{})
        schema=entry.get('schema',{});schema=dict(schema,components=document.get('components',{}))
    def deny_remote(uri):raise NoSuchResource(ref=uri)
    registry=Registry(retrieve=deny_remote).with_resource('urn:reqon:schema',Resource.from_contents(schema,default_specification=DRAFT202012))
    validator=validators.validator_for(schema);validator.check_schema(schema)
    errors=sorted(validator(schema,registry=registry).iter_errors(payload),key=lambda error:str(error.path))
    if errors:
        error=errors[0];raise ValueError('Request validation: '+('/'.join(map(str,error.path)) or '$')+': '+error.message)


def read_limited(chunks,encoding,limit):
    """Limit both compressed bytes and decoded bytes; never inflate a bomb first."""
    codecs=[name.strip().lower() for name in encoding.split(',') if name.strip() and name.strip().lower()!='identity']
    decoders=[]
    for name in reversed(codecs):
        if name not in ('gzip','deflate'):raise ValueError('Unsupported response encoding: '+name)
        decoders.append((name,zlib.decompressobj(31 if name=='gzip' else 15),True))
    wire=0;size=0;pieces=[]
    def decode(chunk):
        for i,(name,decoder,first) in enumerate(decoders):
            output=[];decoded_size=0
            while chunk:
                if name=='gzip' and decoder.eof:decoder=zlib.decompressobj(31)
                try:value=decoder.decompress(chunk,max(1,limit-size-decoded_size+1) if limit else 0)
                except zlib.error:
                    if name!='deflate' or not first:raise
                    decoder=zlib.decompressobj(-15);value=decoder.decompress(chunk,max(1,limit-size-decoded_size+1) if limit else 0)
                decoded_size+=len(value);output.append(value)
                if limit and (decoded_size+size>limit or decoder.unconsumed_tail):raise ValueError('Max response size exceeded. Adjust General > Max response size.')
                chunk=decoder.unused_data
                if chunk and name!='gzip':raise ValueError('Invalid compressed response.')
                first=False
            value=b''.join(output)
            decoders[i]=(name,decoder,False);chunk=value
        return chunk
    for chunk in chunks:
        wire+=len(chunk)
        if limit and wire>limit:raise ValueError('Max response size exceeded. Adjust General > Max response size.')
        chunk=decode(chunk);size+=len(chunk)
        if limit and size>limit:raise ValueError('Max response size exceeded. Adjust General > Max response size.')
        pieces.append(chunk)
    if wire and any(not decoder.eof for _,decoder,_ in decoders):raise ValueError('Truncated compressed response.')
    return b''.join(pieces),wire


def send_http(data,variables,proxy,cookies,cancellation):
    contexts=[]
    try:return _send_http(data,variables,proxy,cookies,cancellation,contexts)
    finally:
        for context in contexts:context.keylog_filename=None


def _send_http(data,variables,proxy,cookies,cancellation,contexts):
    started=time.monotonic();config=data['_general'];options=HTTPOptions.from_data(data,variables)
    deadline=started+options.timeout if options.timeout else None
    version=data.get('http_version',config.http_version)
    url,method,headers,body=prepare_http(data,variables)
    if data.get('body_type')=='file':body=resolve_file(expand(data.get('body_file',''),variables),config).read_bytes()
    lower={key.lower() for key in headers}
    if config.no_cache and 'cache-control' not in lower:headers['Cache-Control']='no-cache'
    if config.postman_token and 'postman-token' not in lower:headers['Postman-Token']=str(uuid.uuid4())
    validate_schema(dict(data,_headers=headers),variables,config,url,body)
    context=ssl.create_default_context() if options.verify else ssl._create_unverified_context();contexts.append(context);context.keylog_filename=data.get('_tls_keylog')
    if options.ca_file and options.verify:context.load_verify_locations(cafile=str(Path(options.ca_file).expanduser()))
    load_client_identity(context,options)
    for redirect in range(21):
        request=Request(url,data=body,headers=headers,method=method)
        if cookies and not config.disable_cookies:cookies.headers(request)
        route=resolve_proxy(url,proxy)
        tunnel=route if route and (route.protocol!='http' or version=='HTTP/2' and urlsplit(url).scheme=='http') else None
        exchange=Exchange(deadline,tunnel)
        exchange.require_h2=version=='HTTP/2'
        kwargs=dict(ssl_context=context,http1=version!='HTTP/2',http2=version!='HTTP/1.x',network_backend=exchange,max_connections=1)
        if route and route.protocol=='http' and not tunnel:
            pool=httpcore.HTTPProxy('http://'+route.authority,proxy_auth=(route.username,route.password) if route.username else None,**kwargs)
        else:pool=httpcore.ConnectionPool(**kwargs)
        outgoing=[(k.encode('ascii'),v.encode('utf-8')) for k,v in request.header_items() if k.lower()!='proxy-authorization']
        with cancellation.use(exchange),pool:
            remaining=exchange.remaining();timeouts={name:remaining for name in ('connect','read','write','pool')}
            with pool.stream(method,url,headers=outgoing,content=body,extensions={'timeout':timeouts}) as response:
                actual=response.extensions.get('http_version',b'').decode()
                if version=='HTTP/2' and actual!='HTTP/2':raise ValueError('The server did not negotiate HTTP/2. Select Auto or HTTP/1.x in General.')
                message=email.message.Message()
                for key,value in response.headers:message[key.decode('latin-1')]=value.decode('latin-1')
                if cookies and not config.disable_cookies:cookies.receive(request,message)
                if options.follow_redirects and response.status in (301,302,303,307,308) and message.get('Location'):
                    if redirect==20:raise ValueError('Too many redirects (limit: 20).')
                    target=urljoin(url,message['Location']);old=urlsplit(url);new=urlsplit(target)
                    if new.scheme not in ('http','https'):raise ValueError('Unsupported redirect protocol.')
                    if (old.scheme,old.hostname,old.port)!=(new.scheme,new.hostname,new.port):
                        headers={key:value for key,value in headers.items() if key.lower() not in ('authorization','cookie','host','proxy-authorization')}
                    if response.status==303 and method!='HEAD' or response.status in (301,302) and method=='POST':
                        method='GET';body=None;headers={k:v for k,v in headers.items() if k.lower() not in ('content-length','content-type','transfer-encoding')}
                    url=target;continue
                limit=config.max_response_mb*1024*1024
                if method!='HEAD' and response.status not in (204,304) and limit and message.get('Content-Length','').isdigit() and int(message['Content-Length'])>limit:raise ValueError('Max response size exceeded. Adjust General > Max response size.')
                content,size=read_limited(response.iter_stream(),message.get('Content-Encoding',''),limit)
                exchange.remaining()
                return dict(status=response.status,reason=response.extensions.get('reason_phrase',b'').decode('latin-1') or http.client.responses.get(response.status,''),
                    headers=dict(message),body=content.decode(message.get_content_charset() or 'utf-8',errors='replace'),size=size,url=url,http_version=actual,duration=round((time.monotonic()-started)*1000))
