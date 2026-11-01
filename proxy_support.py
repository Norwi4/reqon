"""Local proxy preferences and per-connection routing, without global socket/env changes."""
import base64
import fnmatch
import http.client
import ipaddress
import json
import re
import socket
import sys
import urllib.request
from dataclasses import asdict,dataclass,field,replace
from urllib.parse import unquote,urlsplit

PROTOCOLS=('http','socks5','socks5h','socks4','socks4a')


@dataclass(frozen=True)
class ProxyConfig:
    system: bool = True
    environment: bool = True
    custom: bool = False
    http: bool = True
    https: bool = True
    protocol: str = 'http'
    host: str = ''
    port: str = '8080'
    auth: bool = False
    username: str = ''
    password: str = field(default='',repr=False)
    bypass: str = ''
    system_auth: bool = False
    system_username: str = ''
    system_password: str = field(default='',repr=False)
    blocked_error: str = field(default='',repr=False)

    @classmethod
    def from_dict(cls,data):
        if not isinstance(data,dict):raise ValueError('Invalid proxy settings.')
        values={key:value for key,value in data.items() if key in cls.__dataclass_fields__ and key!='blocked_error'}
        defaults=cls()
        for key,value in values.items():
            if type(value) is not type(getattr(defaults,key)):raise ValueError('Invalid proxy setting: '+key)
        return cls(**values)

    def error(self):
        if self.blocked_error:return self.blocked_error
        if not self.custom:return ''
        if self.protocol not in PROTOCOLS:return 'Select an HTTP or SOCKS proxy protocol.'
        if not self.http and not self.https:return 'Select HTTP, HTTPS or both request types.'
        host=self.host.strip()
        if not host or any(c.isspace() or c in '/@?#' for c in host):return 'Invalid host. Enter a hostname or IP address without a protocol.'
        try:
            parsed=urlsplit('//'+host)
            if not parsed.hostname or parsed.port is not None:return 'Enter the port in its separate field.'
        except ValueError:
            try:ipaddress.ip_address(host.strip('[]'))
            except ValueError:return 'Invalid proxy host.'
        if not str(self.port).isdigit() or not 1<=int(self.port)<=65535:return 'Port must be between 1 and 65535.'
        if self.auth and not self.username:return 'Enter the proxy username.'
        return ''


@dataclass(frozen=True)
class ProxyRoute:
    protocol: str
    host: str
    port: int
    username: str = ''
    password: str = field(default='',repr=False)

    @property
    def authority(self):return ('['+self.host+']' if ':' in self.host else self.host)+':'+str(self.port)

    def authorization(self):
        return 'Basic '+base64.b64encode((self.username+':'+self.password).encode()).decode('ascii') if self.username else ''


def bypass_host(host,port,rules):
    host=host.lower().rstrip('.').strip('[]')
    for rule in re.split(r'[,;\s]+',rules or ''):
        rule=rule.lower().strip()
        if not rule:continue
        if rule=='*':return True
        if rule=='<local>' and '.' not in host and ':' not in host:return True
        try:
            if '/' in rule and ipaddress.ip_address(host) in ipaddress.ip_network(rule,strict=False):return True
        except ValueError:pass
        wanted_port=None
        if rule.startswith('['):
            match=re.fullmatch(r'\[([^]]+)\](?::(\d+))?',rule)
            if match:rule,wanted_port=match.groups()
        elif rule.count(':')==1 and rule.rsplit(':',1)[1].isdigit():rule,wanted_port=rule.rsplit(':',1)
        if wanted_port and int(wanted_port)!=port:continue
        if rule.startswith('*.'):
            if fnmatch.fnmatchcase(host,rule):return True
        else:
            rule=rule.lstrip('.')
            if host==rule or host.endswith('.'+rule) or ('*' in rule and fnmatch.fnmatchcase(host,rule)):return True
    return False


def route_from_url(value):
    try:
        parsed=urlsplit(value if '://' in value else 'http://'+value)
        protocol=parsed.scheme.lower()
        if protocol=='socks':protocol='socks5h'
        if protocol not in PROTOCOLS:raise ValueError('Only HTTP CONNECT and SOCKS proxy endpoints are supported.')
        if not parsed.hostname or parsed.path not in ('','/') or parsed.query or parsed.fragment:raise ValueError('Invalid proxy endpoint.')
        return ProxyRoute(protocol,parsed.hostname,parsed.port or (80 if protocol=='http' else 1080),unquote(parsed.username or ''),unquote(parsed.password or ''))
    except ValueError:raise ValueError('Invalid proxy URL. Use an HTTP or SOCKS proxy with a valid host and port.') from None


def system_route(url):
    # Qt uses Windows' native proxy discovery, including PAC, rather than
    # urllib's combined OS/environment lookup. The toggles stay independent.
    if sys.platform=='win32':
        from PySide6.QtCore import QUrl
        from PySide6.QtNetwork import QNetworkProxy,QNetworkProxyFactory,QNetworkProxyQuery
        for proxy in QNetworkProxyFactory.systemProxyForQuery(QNetworkProxyQuery(QUrl(url))):
            if proxy.type()==QNetworkProxy.ProxyType.NoProxy:return None
            if proxy.type() in (QNetworkProxy.ProxyType.HttpProxy,QNetworkProxy.ProxyType.HttpCachingProxy):protocol='http'
            elif proxy.type()==QNetworkProxy.ProxyType.Socks5Proxy:protocol='socks5h'
            else:continue
            return ProxyRoute(protocol,proxy.hostName(),proxy.port(),proxy.user(),proxy.password())
    elif sys.platform=='darwin':
        parsed=urlsplit(url)
        if not urllib.request.proxy_bypass_macosx_sysconf(parsed.netloc):
            value=urllib.request.getproxies_macosx_sysconf().get(parsed.scheme)
            if value:return route_from_url(value)
    return None


def resolve_proxy(url,config=None):
    config=config or ProxyConfig();parsed=urlsplit(url)
    if config.blocked_error:raise ValueError('Proxy: '+config.blocked_error)
    scheme=parsed.scheme;host=parsed.hostname or '';port=parsed.port or (443 if scheme=='https' else 80)
    if config.custom:
        error=config.error()
        if error:raise ValueError('Proxy: '+error)
        if not getattr(config,scheme,False) or bypass_host(host,port,config.bypass):return None
        return ProxyRoute(config.protocol,config.host.strip().strip('[]'),int(config.port),config.username if config.auth else '',config.password if config.auth else '')
    route=None
    if config.environment:
        env=urllib.request.getproxies_environment()
        if bypass_host(host,port,env.get('no','')):return None
        value=env.get(scheme) or env.get('all')
        if value:route=route_from_url(value)
    if route is None and config.system:route=system_route(url)
    if route and config.system_auth and not route.username:
        route=replace(route,username=config.system_username,password=config.system_password)
    return route


def open_proxy_socket(address,route,timeout=None,source_address=None):
    if route is None:return socket.create_connection(address,timeout,source_address)
    import socks
    kind={'http':socks.HTTP,'socks5':socks.SOCKS5,'socks5h':socks.SOCKS5,'socks4':socks.SOCKS4,'socks4a':socks.SOCKS4}[route.protocol]
    try:
        if route.protocol=='http':
            connection=http.client.HTTPConnection(route.host,route.port,timeout=timeout,source_address=source_address)
            auth=route.authorization()
            connection.set_tunnel(*address,headers={'Proxy-Authorization':auth} if auth else {})
            try:
                connection.connect();result=connection.sock;connection.sock=None;return result
            finally:connection.close()
        return socks.create_connection(address,timeout=timeout,source_address=source_address,
            proxy_type=kind,proxy_addr=route.host,proxy_port=route.port,
            proxy_rdns=route.protocol in ('http','socks5h','socks4a'),
            proxy_username=route.username or None,proxy_password=route.password or None)
    except (OSError,socks.ProxyError) as error:
        # Never place a credential-bearing proxy URL in errors/history.
        detail=str(error)
        for secret in (route.password,route.username):
            if secret:detail=detail.replace(secret,'***')
        raise ConnectionError('Proxy connection failed: '+detail) from None


def protect_secret(value,decrypt=False):
    """Protect locally saved Windows passwords using the current user's DPAPI."""
    if not value:return ''
    if sys.platform!='win32':return value
    import ctypes
    from ctypes import wintypes
    class Blob(ctypes.Structure):_fields_=[('size',wintypes.DWORD),('data',ctypes.POINTER(ctypes.c_ubyte))]
    raw=base64.b64decode(value[6:],validate=True) if decrypt and value.startswith('dpapi:') else value.encode('utf-8')
    if decrypt and not value.startswith('dpapi:'):return value
    buffer=ctypes.create_string_buffer(raw);source=Blob(len(raw),ctypes.cast(buffer,ctypes.POINTER(ctypes.c_ubyte)));target=Blob()
    crypt=ctypes.WinDLL('crypt32',use_last_error=True);kernel=ctypes.WinDLL('kernel32',use_last_error=True)
    kernel.LocalFree.argtypes=[ctypes.c_void_p];kernel.LocalFree.restype=ctypes.c_void_p
    func=crypt.CryptUnprotectData if decrypt else crypt.CryptProtectData
    func.argtypes=[ctypes.POINTER(Blob),ctypes.c_void_p,ctypes.c_void_p,ctypes.c_void_p,ctypes.c_void_p,wintypes.DWORD,ctypes.POINTER(Blob)]
    func.restype=wintypes.BOOL
    if not func(ctypes.byref(source),None,None,None,None,1,ctypes.byref(target)):raise OSError('Unable to protect/read the locally stored proxy password.')
    try:
        result=ctypes.string_at(target.data,target.size)
        return result.decode('utf-8') if decrypt else 'dpapi:'+base64.b64encode(result).decode('ascii')
    finally:kernel.LocalFree(target.data)


class ProxyPreferences:
    def __init__(self,store):
        self.store=store;self.load_error='';self.config=ProxyConfig()
        rows=store.all("SELECT data FROM workspace_state WHERE key='proxy'")
        if rows:
            data={}
            try:
                data=json.loads(rows[0][0])
                ProxyConfig.from_dict(data)
                for key in ('password','system_password'):data[key]=protect_secret(data.get(key,''),decrypt=True)
                self.config=ProxyConfig.from_dict(data)
            except (ValueError,OSError,TypeError):
                self.load_error='Saved proxy settings could not be read. Re-enter the proxy credentials in Settings > Proxy.'
                try:self.config=ProxyConfig.from_dict(dict(data,password='',system_password=''))
                except (ValueError,TypeError):pass
                self.config=replace(self.config,blocked_error=self.load_error)

    def save(self,config):
        data=asdict(config)
        data.pop('blocked_error',None)
        for key in ('password','system_password'):data[key]=protect_secret(data[key])
        with self.store.atomic():self.store.db.execute("INSERT OR REPLACE INTO workspace_state(key,data) VALUES('proxy',?)",(json.dumps(data),))
        self.config=config;self.load_error=''
