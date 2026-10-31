"""TLS without certificate validation for explicitly opted-out gRPC requests.

grpcio's Python API always validates certificates. A private loopback relay lets
Python's SSL transport apply the requested policy, including SNI and mutual TLS.
The connection to the actual server is always TLS, with HTTP/2 ALPN. No global
TLS settings, fetched trust anchors, or plaintext remote fallbacks are used.
"""
import asyncio
import ssl
import threading
from urllib.parse import urlsplit

import grpc
from request_transport import load_client_identity


class UnverifiedTLSChannel(grpc.Channel):
    def __init__(self,target,tls,channel_options=()):
        parsed=urlsplit('//'+target)
        self.host=parsed.hostname; self.port=parsed.port or 443
        self.server_name=tls.server_name or self.host
        self.context=ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
        self.context.check_hostname=False; self.context.verify_mode=ssl.CERT_NONE
        self.context.minimum_version=ssl.TLSVersion.TLSv1_2
        self.context.set_alpn_protocols(['h2'])
        load_client_identity(self.context,tls)
        self.last_error=None; self.tasks=set(); self.server=None
        self._close_lock=threading.Lock(); self._closed=False
        self.loop=asyncio.new_event_loop()
        self.thread=threading.Thread(target=self.loop.run_forever,name='Getman TLS relay',daemon=True)
        self.thread.start()
        try:
            port=asyncio.run_coroutine_threadsafe(self._start(),self.loop).result(timeout=5)
            self.channel=grpc.insecure_channel(f'127.0.0.1:{port}',options=[
                *channel_options,('grpc.default_authority',tls.server_name or target)])
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
            remote,upstream=await asyncio.open_connection(self.host,self.port,
                ssl=self.context,server_hostname=self.server_name,ssl_handshake_timeout=10)
            if upstream.get_extra_info('ssl_object').selected_alpn_protocol()!='h2':
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
            self.server.close(); await self.server.wait_closed()
        pending=list(self.tasks)
        for task in pending:task.cancel()
        if pending:await asyncio.gather(*pending,return_exceptions=True)
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

    def __enter__(self):return self
    def __exit__(self,exc_type,exc_value,traceback):
        self.close()
        if isinstance(exc_value,grpc.RpcError) and exc_value.code()==grpc.StatusCode.UNAVAILABLE and self.last_error:
            raise ConnectionError('TLS connection failed: '+self.last_error) from exc_value
    def subscribe(self,*args,**kwargs):return self.channel.subscribe(*args,**kwargs)
    def unsubscribe(self,*args,**kwargs):return self.channel.unsubscribe(*args,**kwargs)
    def unary_unary(self,*args,**kwargs):return self.channel.unary_unary(*args,**kwargs)
    def unary_stream(self,*args,**kwargs):return self.channel.unary_stream(*args,**kwargs)
    def stream_unary(self,*args,**kwargs):return self.channel.stream_unary(*args,**kwargs)
    def stream_stream(self,*args,**kwargs):return self.channel.stream_stream(*args,**kwargs)
