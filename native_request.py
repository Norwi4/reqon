"""GUI request adapter. Rust owns HTTP/gRPC execution; Python keeps editor policy.

The private pipe carries complete response chunks. Secrets never appear in argv,
temporary request files or regression reports. Cancel terminates only this request.
"""
import base64
import atexit
import hashlib
from contextlib import ExitStack
import json
import os
from pathlib import Path
import ssl
import subprocess
import threading
import time
from urllib.parse import urlsplit, urlunsplit

from runner_controller import runner_binary, RUNNER_VERSION

_idle=[]
_idle_lock=threading.Lock()
_cache_generation=0


def _dispose(process):
    if process.poll() is None:
        try:process.terminate()
        except OSError:pass
    process.wait(timeout=5)
    for pipe in (process.stdin,process.stdout):pipe.close()


def shutdown_idle():
    global _cache_generation
    with _idle_lock:
        _cache_generation+=1
        processes=[entry[1] for entry in _idle];_idle.clear()
    for process in processes:_dispose(process)


def invalidate_connections():
    """Refresh action: retire cached engines without blocking the Qt thread."""
    global _cache_generation
    with _idle_lock:
        _cache_generation+=1
        processes=[entry[1] for entry in _idle];_idle.clear()
    def dispose():
        for process in processes:_dispose(process)
    if processes:threading.Thread(target=dispose,name='reqon-engine-retire',daemon=True).start()


atexit.register(shutdown_idle)


class NativeError(ValueError):
    pass


class RequestProcess:
    def __init__(self):
        environment = os.environ.copy()
        if os.name=='nt' and not os.environ.get('SystemRoot'):
            import ctypes
            directory = ctypes.create_unicode_buffer(32768)
            if ctypes.windll.kernel32.GetWindowsDirectoryW(directory,len(directory)):
                environment['SystemRoot'] = directory.value
        binary=runner_binary();stat=Path(binary).stat()
        self.key=(binary,stat.st_mtime_ns,stat.st_size,RUNNER_VERSION,_cache_generation,
            hashlib.sha256(json.dumps(environment,sort_keys=True).encode()).digest())
        self.process=None;self._hello=None
        with _idle_lock:
            for index,(key,process,hello) in enumerate(_idle):
                if key==self.key and process.poll() is None:
                    self.process=process;self._hello=hello;_idle.pop(index);break
        if self.process is None:
            self.process = subprocess.Popen([binary, 'request'], stdin=subprocess.PIPE,
                stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                env=environment,
                creationflags=subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0)
        self.lock = threading.Lock()
        self.closed = False;self.completed=False;self.released=False

    def __enter__(self):
        return self

    def __exit__(self, exc_type, *_):
        with self.lock:
            reusable=(exc_type is None and self.completed and not self.closed
                and self.process.poll() is None and self.key[4]==_cache_generation)
            if reusable:self.released=True
        if reusable:
            with _idle_lock:
                old=_idle.pop(0)[1] if len(_idle)>=2 else None
                _idle.append((self.key,self.process,self._hello))
            if old:_dispose(old)
        else:
            self.close();_dispose(self.process)

    def close(self):
        # Called on the UI thread by Cancellation: never wait for a worker here.
        with self.lock:
            if self.released:return
            self.closed = True
            if self.process.poll() is None:
                try: self.process.terminate()
                except OSError: pass

    def hello(self):
        if self._hello is not None:
            if self._hello != dict(type='hello', protocol=1, version=RUNNER_VERSION):
                raise NativeError('Reqon engine version mismatch. Install the matching application and CLI.')
            return
        timer = threading.Timer(5, self.close)
        timer.daemon = True; timer.start()
        try:
            value = self.read()
            if value != dict(type='hello', protocol=1, version=RUNNER_VERSION):
                raise NativeError('Reqon engine version mismatch. Install the matching application and CLI.')
            self._hello=value
        finally: timer.cancel(); timer.join()

    def send(self, value):
        if self.closed: raise NativeError('Request cancelled.')
        self.completed=False
        self.process.stdin.write(json.dumps(dict(value, protocol=1), ensure_ascii=False).encode('utf-8') + b'\n')
        self.process.stdin.flush()

    def read(self):
        raw = self.process.stdout.readline(1024 * 1024 + 1)
        if not raw:
            raise NativeError('Request cancelled.' if self.closed else 'Reqon engine stopped before returning a response. Check the bundled engine version.')
        if len(raw) > 1024 * 1024: raise NativeError('Invalid engine response frame.')
        value = json.loads(raw)
        if value.get('type')=='complete':self.completed=True
        if value.get('type') == 'error':
            message = value.get('message', 'Request failed.')
            code = value.get('code') or ''
            if code:
                import re
                code = re.sub(r'(?<!^)(?=[A-Z])', '_', code).upper()
                # Preserve the existing error panel's gRPC status classification.
                import grpc
                if 'certificate' in message.lower(): message = 'CERTIFICATE_VERIFY_FAILED: '+message
                error = GrpcError(getattr(grpc.StatusCode, code, grpc.StatusCode.UNKNOWN), message,
                    value.get('headers', []), value.get('trailers', []))
                raise error
            if 'timeout' in message.lower(): raise TimeoutError(message)
            if any(word in message.lower() for word in ('certificate', 'unknownissuer')): raise ssl.SSLError(message)
            raise NativeError(message)
        return value

    def chunks(self):
        while True:
            value = self.read()
            if value.get('type') == 'complete': return
            if value.get('type') != 'chunk': raise NativeError('Unexpected engine response frame.')
            yield base64.b64decode(value['data'], validate=True)


import grpc
class GrpcError(grpc.RpcError):
    def __init__(self, code, details, headers=(), trailers=()):
        self._code, self._details, self._headers, self._trailers = code, details, headers, trailers
    def code(self): return self._code
    def details(self): return self._details
    def initial_metadata(self): return self._headers
    def trailing_metadata(self): return self._trailers
    def __str__(self): return self._details


def transport_options(options):
    enabled = getattr(options, 'enabled', True)
    verify = getattr(options, 'verify', getattr(options, 'verify_server', True))
    return dict(allow_plaintext_remote=True,
        ca_cert=str(Path(options.ca_file).expanduser()) if enabled and verify and options.ca_file else None,
        client_cert=str(Path(options.cert_file).expanduser()) if enabled and options.cert_file else None,
        client_key=str(Path(options.key_file).expanduser()) if enabled and options.key_file else None)


class HTTPResponse:
    def __init__(self, process, head):
        self.process = process
        self.status = head['status']
        self.headers = [(k.encode('latin-1'), v.encode('latin-1')) for k,v in head['headers']]
        self.extensions = dict(http_version=head['http_version'].replace('HTTP/2.0', 'HTTP/2').encode(),
            reason_phrase=head['reason'].encode('latin-1'))
    def __enter__(self): return self
    def __exit__(self, exc_type, *_):
        if exc_type is None and not self.process.completed:
            for _ in self.process.chunks():pass
    def iter_stream(self): return self.process.chunks()


class HTTPPool:
    """httpcore-compatible exchange so redirects, cookies and decoding stay shared."""
    def __init__(self, data, variables, options, context, route, deadline, cancellation):
        self.data, self.variables, self.options = data, variables, options
        self.context, self.route, self.deadline, self.cancellation = context, route, deadline, cancellation
        self.stack = ExitStack()
    def __enter__(self): return self
    def __exit__(self, *args): return self.stack.__exit__(*args)
    def stream(self, method, url, headers, content, extensions):
        from request_transport import TLSOptions
        from tls_channel import TransportChannel
        parts = urlsplit(url); version = self.data.get('http_version', 'HTTP/1.x')
        transport = transport_options(self.options)
        route = self.route
        raw_headers = not any(k.lower()==b'accept' for k,v in headers)
        # OpenSSL-specific cipher policies and SOCKS keep the existing transport
        # relay. HTTP parsing, execution and response streaming are always Rust.
        relay = None
        custom_tls = parts.scheme == 'https' and any(self.data.get(k) for k in
            ('http_ciphers', 'http_disabled_tls', 'http_server_cipher_order', '_tls_keylog'))
        if custom_tls or raw_headers and (parts.scheme=='https' or route) or route and (route.protocol != 'http' or version == 'HTTP/2' and parts.scheme == 'http'):
            tls = TLSOptions(enabled=False)
            relay = self.stack.enter_context(self.cancellation.use(TransportChannel(parts.netloc, tls, proxy=route, raw=True,
                ssl_context=self.context if parts.scheme == 'https' else None, http_version=version)))
            # The relay needs the original TLS server name and default HTTPS port.
            relay.server_name = parts.hostname
            relay.port = parts.port or (443 if parts.scheme == 'https' else 80)
            if version=='Auto':
                timeout=10 if self.deadline is None else max(.001,self.deadline-time.monotonic())
                version=relay.prepare_http(timeout)
            url = urlunsplit(('http', relay.address, parts.path, parts.query, ''))
            raw_headers=True
            transport = dict(allow_plaintext_remote=True)
        elif route:
            transport.update(proxy='http://' + route.authority, proxy_authorization=route.authorization() or None)
        process = self.stack.enter_context(self.cancellation.use(RequestProcess()))
        try:
            process.hello()
            remaining = 0 if self.deadline is None else max(1, round((self.deadline-time.monotonic())*1000))
            if self.deadline is not None and self.deadline <= time.monotonic(): raise TimeoutError('Request timeout exceeded.')
            process.send(dict(kind='http', url=url, method=method,
                origin=parts.scheme+'://'+parts.netloc,
                headers=[(k.decode('ascii'), v.decode('utf-8')) for k,v in headers],
                body=base64.b64encode(content).decode('ascii') if content is not None else None,
                http_version=version, timeout_ms=remaining, verify=self.options.verify,
                limit=self.data['_general'].max_response_mb*1024*1024,
                strict=bool(self.data.get('http_strict_parser')),
                raw_headers=raw_headers,
                skip_redirect_body=self.options.follow_redirects, transport=transport))
            head = process.read()
            if head.get('type') != 'headers': raise NativeError('Expected response headers from engine.')
            return HTTPResponse(process, head)
        except Exception as error:
            if relay and relay.last_error: raise ConnectionError(relay.last_error) from error
            if self.data.get('http_strict_parser') and 'header' in str(error).lower(): raise NativeError('Strict HTTP parser: '+str(error)) from error
            raise


def invoke(data, variables, expand, cancellation=None):
    from grpc_support import Cancellation
    from request_transport import TLSOptions, GRPCOptions, grpc_target, grpc_metadata, parse_message
    from proxy_support import resolve_proxy
    from tls_channel import TransportChannel
    started = time.monotonic(); cancellation = cancellation or Cancellation()
    server = expand(data.get('server', ''), variables)
    target = grpc_target(server); tls = TLSOptions.from_data(data, variables)
    settings = GRPCOptions.from_data(data)
    endpoint = ('https://' if tls.enabled else 'http://') + target
    route = resolve_proxy(endpoint, data.get('_proxy_config'))
    transport = transport_options(tls)
    transport.update(grpc_emit_defaults=settings.emit_defaults,
        grpc_descriptor_set=expand(data.get('descriptor_file', ''), variables) or None)
    relay = None
    with ExitStack() as stack:
        if route or tls.enabled and (not tls.verify_server or tls.keylog_file):
            relay = stack.enter_context(TransportChannel(target, tls, proxy=route, raw=True))
            endpoint = 'http://' + relay.address
            transport.update(ca_cert=None, client_cert=None, client_key=None)
        process = stack.enter_context(cancellation.use(RequestProcess()))
        try:
            process.hello()
            remaining = 0 if not settings.timeout_ms else settings.timeout_ms-round((time.monotonic()-started)*1000)
            if settings.timeout_ms and remaining <= 0: raise TimeoutError('Connection Timeout exceeded.')
            process.send(dict(kind='grpc', url=endpoint,
                origin=('https://' if tls.enabled else 'http://')+(tls.server_name or target),
                tls_server_name=tls.server_name or None,
                method=expand(data.get('service', ''), variables).lstrip('.')+'/'+expand(data.get('rpc_method', ''), variables),
                headers=grpc_metadata(data, variables),
                payload=parse_message(expand(data.get('message') or '{}', variables)),
                timeout_ms=remaining, limit=max(0, settings.receive_limit), transport=transport,
                grpc_cache=relay is None))
            head = process.read()
            if head.get('type') != 'headers': raise NativeError('Expected gRPC response headers from engine.')
            body = b''.join(process.chunks()).decode('utf-8')
        except Exception as error:
            if relay and relay.last_error: raise GrpcError(grpc.StatusCode.UNAVAILABLE, relay.last_error) from error
            if isinstance(error, TimeoutError): raise GrpcError(grpc.StatusCode.DEADLINE_EXCEEDED, str(error)) from error
            # Tonic's transport timer can win the race with the outer RPC deadline
            # and report CANCELLED. Only normalize that timeout at our own deadline;
            # an earlier cancellation returned by the server remains CANCELLED.
            if (isinstance(error, GrpcError) and error.code()==grpc.StatusCode.CANCELLED
                    and error.details()=='Timeout expired' and settings.timeout_ms
                    and round((time.monotonic()-started)*1000)>=settings.timeout_ms):
                raise GrpcError(grpc.StatusCode.DEADLINE_EXCEEDED, error.details(),
                    error.initial_metadata(), error.trailing_metadata()) from error
            raise
    return dict(status=0, reason='OK', body=body, headers=[tuple(row) for row in head['headers']], trailers=[tuple(row) for row in head['trailers']],
        size=len(body.encode('utf-8')), url=server, duration=round((time.monotonic()-started)*1000),
        engine='rust', engine_version=RUNNER_VERSION)
