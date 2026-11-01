"""Per-channel gRPC relay for explicit TLS policies and HTTP/SOCKS proxies.

The local channel never inherits environment proxies. Its upstream uses the
selected proxy and TLS policy, including SNI, verification and mutual TLS.
"""
import asyncio
import ssl
import threading
from pathlib import Path
from urllib.parse import urlsplit

import grpc
from request_transport import load_client_identity


class TransportChannel(grpc.Channel):
    def __init__(self,target,tls,channel_options=(),proxy=None):
        parsed=urlsplit('//'+target)
        self.host=parsed.hostname; self.port=parsed.port or (443 if tls.enabled else 80);self.proxy=proxy
        self.server_name=tls.server_name or self.host
        self.context=None
        if tls.enabled:
            self.context=ssl.create_default_context() if tls.verify_server else ssl._create_unverified_context()
            self.context.keylog_filename=tls.keylog_file or None
            self.context.minimum_version=ssl.TLSVersion.TLSv1_2;self.context.set_alpn_protocols(['h2'])
            if tls.ca_file and tls.verify_server:self.context.load_verify_locations(cafile=str(Path(tls.ca_file).expanduser()))
            load_client_identity(self.context,tls)
        self.last_error=None; self.tasks=set(); self.server=None
        self._close_lock=threading.Lock(); self._closed=False
        self.loop=asyncio.new_event_loop()
        self.thread=threading.Thread(target=self.loop.run_forever,name='Reqon TLS relay',daemon=True)
        self.thread.start()
        try:
            port=asyncio.run_coroutine_threadsafe(self._start(),self.loop).result(timeout=5)
            self.channel=grpc.insecure_channel(f'127.0.0.1:{port}',options=[
                *channel_options,('grpc.enable_http_proxy',0),('grpc.default_authority',tls.server_name or target)])
        except BaseException:
            self.close(); raise

    async def _start(self):
        self.server=await asyncio.start_server(self._connect,'127.0.0.1',0)
        return self.server.sockets[0].getsockname()[1]

    async def _connect(self,reader,writer):
        task=asyncio.current_task(); self.tasks.add(task)
        upstream=None; pumps=[]
        async def pump(source,destination):
            while chunk:=await source.read(65536):
                destination.write(chunk)
                await destination.drain()
        try:
            if self._closed:return
            tls_args=dict(ssl=self.context,server_hostname=self.server_name,ssl_handshake_timeout=10) if self.context else {}
            if self.proxy:
                from proxy_support import open_proxy_socket
                cancelled=threading.Event();connect_lock=threading.Lock();pending_socket={}
                def connect():
                    connection=open_proxy_socket((self.host,self.port),self.proxy,10)
                    with connect_lock:
                        if cancelled.is_set():connection.close();return None
                        pending_socket['socket']=connection
                    return connection
                try:connection=await asyncio.to_thread(connect)
                except asyncio.CancelledError:
                    with connect_lock:
                        cancelled.set()
                        if abandoned:=pending_socket.pop('socket',None):abandoned.close()
                    raise
                with connect_lock:pending_socket.pop('socket',None)
                try:
                    connection.setblocking(False)
                    remote,upstream=await asyncio.open_connection(sock=connection,**tls_args)
                except BaseException:connection.close();raise
            else:remote,upstream=await asyncio.open_connection(self.host,self.port,**tls_args)
            if self.context and upstream.get_extra_info('ssl_object').selected_alpn_protocol()!='h2':
                raise OSError('The TLS server did not negotiate HTTP/2 (h2).')
            self.last_error=None
            pumps=[asyncio.create_task(pump(reader,upstream)),asyncio.create_task(pump(remote,writer))]
            done,_=await asyncio.wait(pumps,return_when=asyncio.FIRST_COMPLETED)
            for result in done:result.result()
        except (OSError,ValueError,TimeoutError) as error:
            self.last_error=str(error)
        finally:
            for pending in pumps:pending.cancel()
            streams=[stream for stream in (writer,upstream) if stream is not None]
            # Close both directions before awaiting anything: cancellation may
            # arrive again while a peer is withholding its TLS close_notify.
            for stream in streams:stream.close()
            try:
                if pumps:await asyncio.gather(*pumps,return_exceptions=True)
                await asyncio.wait_for(asyncio.gather(*(stream.wait_closed() for stream in streams),return_exceptions=True),timeout=0.5)
            except (OSError,TimeoutError):pass
            finally:
                for stream in streams:stream.transport.abort()
                self.tasks.discard(task)

    async def _shutdown(self):
        if self.server:
            self.server.close()
        pending=list(self.tasks)
        for task in pending:task.cancel()
        if pending:await asyncio.gather(*pending,return_exceptions=True)
        # On current Python, wait_closed also waits for client transports.
        # Cancel the relay tasks first so their sockets can actually close.
        if self.server:await self.server.wait_closed()
        # Let socket transport connection_lost callbacks finish before stopping
        # the event loop (particularly Windows' overlapped I/O completion).
        await asyncio.sleep(0.01)

    def close(self):
        with self._close_lock:
            if self._closed:return
            self._closed=True
            if hasattr(self,'channel'):self.channel.close()
            try:asyncio.run_coroutine_threadsafe(self._shutdown(),self.loop).result(timeout=3)
            finally:
                self.loop.call_soon_threadsafe(self.loop.stop)
                self.thread.join(timeout=2)
                if not self.thread.is_alive():self.loop.close()
                if self.context:self.context.keylog_filename=None

    def __enter__(self):return self
    def __exit__(self,exc_type,exc_value,traceback):
        self.close()
        if isinstance(exc_value,grpc.RpcError) and exc_value.code()==grpc.StatusCode.UNAVAILABLE and self.last_error:
            prefix='Proxy connection failed: ' if self.proxy else 'TLS connection failed: '
            raise ConnectionError(self.last_error if self.last_error.startswith(prefix) else prefix+self.last_error) from exc_value
    def subscribe(self,*args,**kwargs):return self.channel.subscribe(*args,**kwargs)
    def unsubscribe(self,*args,**kwargs):return self.channel.unsubscribe(*args,**kwargs)
    def unary_unary(self,*args,**kwargs):return self.channel.unary_unary(*args,**kwargs)
    def unary_stream(self,*args,**kwargs):return self.channel.unary_stream(*args,**kwargs)
    def stream_unary(self,*args,**kwargs):return self.channel.stream_unary(*args,**kwargs)
    def stream_stream(self,*args,**kwargs):return self.channel.stream_stream(*args,**kwargs)


UnverifiedTLSChannel=TransportChannel
