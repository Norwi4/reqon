"""Loopback-only forwarding proxies for real HTTP, TLS and gRPC transport tests."""
import base64
import http.client
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
import select
import socket
import socketserver
import struct
import threading
from urllib.parse import urlsplit


def connect(host,port):
    if host not in ('127.0.0.1','::1','localhost','origin.test'):raise ValueError('Non-local fixture target')
    return socket.create_connection(('127.0.0.1',int(port)),3)


def relay(left,right):
    with right:
        while True:
            ready,_,_=select.select([left,right],[],[],3)
            if not ready:return
            for source in ready:
                try:
                    data=source.recv(65536)
                    if not data:return
                    (right if source is left else left).sendall(data)
                except OSError:return


class Proxy:
    def __init__(self,protocol='http',auth=False):
        self.calls=[];self.auth=auth;owner=self
        if protocol=='http':
            class Handler(BaseHTTPRequestHandler):
                def log_message(self,*args):pass
                def authorized(self):
                    owner.calls.append((self.command,self.path,self.headers.get('Proxy-Authorization')))
                    if owner.auth and self.headers.get('Proxy-Authorization')!='Basic '+base64.b64encode(b'test:secret').decode():
                        self.send_error(407);return False
                    return True
                def do_CONNECT(self):
                    if not self.authorized():return
                    host,port=self.path.rsplit(':',1)
                    upstream=connect(host,port)
                    self.send_response(200);self.end_headers();relay(self.connection,upstream)
                def do_GET(self):
                    if not self.authorized():return
                    url=urlsplit(self.path);connection=http.client.HTTPConnection('127.0.0.1',url.port,timeout=3)
                    if url.hostname not in ('127.0.0.1','localhost','origin.test'):raise ValueError('Non-local fixture target')
                    headers={k:v for k,v in self.headers.items() if k.lower() not in ('proxy-authorization','proxy-connection','connection')}
                    try:
                        connection.request('GET',url.path+('?' +url.query if url.query else ''),headers=headers)
                        response=connection.getresponse();body=response.read();self.send_response(response.status)
                        for key,value in response.getheaders():
                            if key.lower() not in ('connection','transfer-encoding','content-length'):self.send_header(key,value)
                        self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
                    finally:connection.close()
            self.server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
        else:
            class Handler(socketserver.StreamRequestHandler):
                def read(self,n):
                    data=self.rfile.read(n)
                    if len(data)!=n:raise EOFError()
                    return data
                def terminated(self):
                    value=b''
                    while (byte:=self.read(1))!=b'\0':value+=byte
                    return value.decode()
                def handle(self):
                    version=self.read(1)[0];user=password=''
                    if version==5:
                        methods=self.read(self.read(1)[0]);method=2 if owner.auth else 0
                        self.connection.sendall(bytes([5,method]))
                        if method==2:
                            self.read(1);user=self.read(self.read(1)[0]).decode();password=self.read(self.read(1)[0]).decode()
                            self.connection.sendall(bytes([1,0 if (user,password)==('test','secret') else 1]))
                            if (user,password)!=('test','secret'):return
                        version,command,zero,kind=self.read(4)
                        if kind==1:host=socket.inet_ntoa(self.read(4))
                        elif kind==3:host=self.read(self.read(1)[0]).decode()
                        elif kind==4:host=socket.inet_ntop(socket.AF_INET6,self.read(16))
                        else:raise ValueError('Unsupported fixture address')
                        port=struct.unpack('!H',self.read(2))[0]
                        reply=b'\x05\x00\x00\x01\x7f\x00\x00\x01\x00\x00'
                    elif version==4:
                        command=self.read(1)[0];port=struct.unpack('!H',self.read(2))[0];ip=self.read(4);user=self.terminated()
                        kind=3 if ip[:3]==b'\0\0\0' and ip[3] else 1
                        host=self.terminated() if kind==3 else socket.inet_ntoa(ip)
                        reply=b'\x00\x5a\x00\x00\x00\x00\x00\x00'
                    else:raise ValueError('Unsupported fixture protocol')
                    owner.calls.append((version,host,port,kind,user,password))
                    upstream=connect(host,port);self.connection.sendall(reply);relay(self.connection,upstream)
            class Server(socketserver.ThreadingTCPServer):daemon_threads=True
            self.server=Server(('127.0.0.1',0),Handler)
        self.port=self.server.server_address[1]
        self.thread=threading.Thread(target=self.server.serve_forever,kwargs={'poll_interval':.02},daemon=True);self.thread.start()
    def close(self):self.server.shutdown();self.server.server_close();self.thread.join(3)
    def __enter__(self):return self
    def __exit__(self,*args):self.close()
