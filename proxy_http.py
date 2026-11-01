"""Per-opener HTTP proxy routing, including SOCKS and redirect/bypass reevaluation."""
import http.client
import urllib.request
from proxy_support import resolve_proxy,open_proxy_socket


class RoutingHandler(urllib.request.ProxyHandler):
    def __init__(self,config):
        self.config=config
        super().__init__({'http':'policy','https':'policy'})

    def proxy_open(self,req,proxy,type):
        route=resolve_proxy(req.full_url,self.config)
        req._reqon_route=route
        req.remove_header('Proxy-authorization')
        if route and route.protocol=='http':
            auth=route.authorization()
            if auth:req.add_unredirected_header('Proxy-authorization',auth)
            req.set_proxy(route.authority,'http')
        return None


class HTTPHandler(urllib.request.HTTPHandler):
    def http_open(self,req):
        route=getattr(req,'_reqon_route',None)
        if route is None or route.protocol=='http':return super().http_open(req)
        class Connection(http.client.HTTPConnection):
            def connect(self):self.sock=open_proxy_socket((self.host,self.port),route,self.timeout,self.source_address)
        return self.do_open(Connection,req)


class HTTPSHandler(urllib.request.HTTPSHandler):
    def https_open(self,req):
        route=getattr(req,'_reqon_route',None)
        if route is None or route.protocol=='http':return super().https_open(req)
        class Connection(http.client.HTTPSConnection):
            def connect(self):
                sock=open_proxy_socket((self.host,self.port),route,self.timeout,self.source_address)
                try:self.sock=self._context.wrap_socket(sock,server_hostname=self.host)
                except BaseException:sock.close();raise
        return self.do_open(Connection,req,context=self._context)
