"""Windows error presentation from typed causes and protocol status; never parse message text."""
import errno
import socket
import ssl


def decorate_failure(result,error=None):
    info=dict(result.get('error_info',{}));category=info.get('category');pending=[error] if error else [];causes=[];seen=set()
    # Already decorated by Worker (or stored in a response example). Rendering
    # it again must not discard the typed cause after the exception is gone.
    if error is None and 'asset' in info:return result
    # The legacy presenter also infers TLS from diagnostic strings. That is
    # useful as legacy copy, but not evidence for choosing a new illustration.
    if category=='tls':category='connection' if result.get('status')==14 else 'request'
    while pending:
        cause=pending.pop()
        if not isinstance(cause,BaseException) or id(cause) in seen:continue
        seen.add(id(cause));causes.append(cause)
        pending.extend((cause.__cause__,cause.__context__,getattr(cause,'reason',None)))
    if any(isinstance(e,ssl.SSLError) for e in causes):category='tls'
    elif any(isinstance(e,socket.gaierror) for e in causes):category='dns'
    elif any(isinstance(e,TimeoutError) or getattr(e,'errno',None) in (errno.ETIMEDOUT,10060) for e in causes):category='timeout'
    elif any(isinstance(e,ConnectionRefusedError) or getattr(e,'errno',None) in (errno.ECONNREFUSED,errno.ENETUNREACH,errno.EHOSTUNREACH,10061,10065) for e in causes):category='connection_refused'
    if info.get('category')=='cancelled':category='cancelled'
    assets={'tls':'error-tls-handshake','dns':'error-dns-resolution','timeout':'error-request-timeout','connection_refused':'error-connection-refused'}
    info['asset']=assets.get(category)
    if category=='tls':info.update(title='TLS error',hint='A secure connection could not be established. Check TLS and certificate settings.')
    elif category=='timeout':info.update(title='Request timed out',hint='The request took too long to complete. Check the timeout settings and network connection.')
    elif category=='dns':info.update(title='DNS error',hint='Check the server hostname and DNS configuration.')
    elif category=='connection_refused':info.update(title='Connection failed',hint='Could not connect to the target service. Check the address, port and network connection.')
    elif result.get('status')==14 and not info['asset']:info['asset']='error-service-unavailable'
    # gRPC exposes UNAVAILABLE for several transport failures without a typed
    # cause. Keep that status generic and preserve every diagnostic detail.
    info['category']=category;result['error_info']=info;return result


def http_error_info(result):
    status=result['status'];reason=result.get('reason','')
    return {'badge':f'{status} {reason}','title':'Service unavailable' if status==503 else reason or 'Request failed',
        'hint':'The server returned an HTTP error response. The original response body is available in Body.',
        'details':result.get('body') or 'The server returned no response body.',
        'asset':'error-service-unavailable' if status==503 else None,'tone':'error'}
