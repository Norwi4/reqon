"""Structured failures for the response UI; retain protocol status and metadata."""
import base64
import ssl
import urllib.error
import grpc


GRPC_ERRORS={
    1:('Request cancelled','The request was cancelled before it completed.'),
    2:('Unknown error','The request failed without a more specific gRPC status.'),
    3:('Invalid request','One or more request values are invalid. Check the message and metadata.'),
    4:('Deadline exceeded','The request did not finish before its deadline. Check Connection Timeout in Settings.'),
    5:('Not found','The requested resource could not be found.'),
    6:('Already exists','The resource you are trying to create already exists.'),
    7:('Permission denied','The caller does not have permission to perform this operation.'),
    8:('Resource exhausted','A resource limit was reached. Check the details and Maximum response message size in Settings.'),
    9:('Precondition failed','The server rejected this request because a required condition was not met.'),
    10:('Request aborted','The operation was aborted. Review the server details before trying again.'),
    11:('Out of range','A request value is outside the range supported by this operation.'),
    12:('Not implemented','The server does not support the requested operation.'),
    13:('Internal server error','The service encountered an internal error while processing the request.'),
    14:('Service unavailable','The service is currently unavailable. Check the server address, TLS settings and network connection.'),
    15:('Data loss','The operation reported missing or corrupted data. Review the server details.'),
    16:('Authentication required','The server could not authenticate this request. Check Authorization and Metadata.'),
}


def _rpc_exception(error):
    seen=set()
    while error is not None and id(error) not in seen:
        seen.add(id(error))
        if isinstance(error,grpc.RpcError) and hasattr(error,'code'):return error
        error=error.__cause__ or error.__context__
    return None


def _metadata(error,name):
    try:rows=getattr(error,name)() or []
    except Exception:return []
    return [[str(key),base64.b64encode(value).decode('ascii') if isinstance(value,bytes) else str(value)] for key,value in rows]


def failure_result(error,protocol,duration,message=None,cancelled=False):
    rpc=_rpc_exception(error) if protocol=='grpc' else None
    code=rpc.code() if rpc else None
    number=code.value[0] if isinstance(code,grpc.StatusCode) else -1
    reason=code.name if isinstance(code,grpc.StatusCode) else 'CLIENT_ERROR'
    details=(rpc.details() or 'No additional error details were provided.') if rpc is error else str(error)
    title='Could not send request';hint='Check the request configuration and try again.'
    badge='REQUEST_ERROR';category='request';art='request';tone='error'
    if rpc:
        title,hint=GRPC_ERRORS.get(number,('Request failed','Review the error details below.'))
        badge=f'{number} {reason}';category='grpc'
        if number==14:category='connection';art='connection'
        elif number==4:category='timeout';art='timeout'
        elif number==1:category='cancelled';art='cancelled';tone='neutral'
    elif isinstance(error,(TimeoutError,)) or isinstance(getattr(error,'reason',None),TimeoutError):
        title='Request timed out';hint='The connection did not complete in time. Check the server and network connection.'
        badge='TIMEOUT';reason='TIMEOUT';category='timeout';art='timeout'
    elif isinstance(error,(ConnectionError,urllib.error.URLError)):
        title='Could not connect';hint='Check the server address and network connection, then try again.'
        badge='CONNECTION_ERROR';reason='CONNECTION_ERROR';category='connection';art='connection'
    if number==14 or category=='connection':
        lower=details.lower()
        if 'certificate_verify_failed' in lower or 'certificate verify failed' in lower:
            title='Certificate verification failed'
            hint='The server certificate is not trusted. Check its CA certificate and the certificate verification setting.'
            category='tls'
        elif 'tls' in lower or 'ssl' in lower or isinstance(getattr(error,'reason',error),ssl.SSLError):
            title='TLS connection failed';hint='A secure connection could not be established. Check TLS and certificate settings.';category='tls'
    if cancelled:
        number=1 if protocol=='grpc' else -1;reason='CANCELLED';badge='1 CANCELLED' if protocol=='grpc' else 'CANCELLED'
        title,hint=GRPC_ERRORS[1];details='Request cancelled.';category='cancelled';art='cancelled';tone='neutral'
    text=message if message is not None else f'{badge}: {title}\n\n{details}'
    return {'error':text,'status':number,'reason':reason,'duration':duration,'size':len(details.encode('utf-8')),
        'body':details,'headers':_metadata(rpc,'initial_metadata') if rpc else [],
        'trailers':_metadata(rpc,'trailing_metadata') if rpc else [],
        'error_info':{'title':title,'hint':hint,'details':details,'badge':badge,
            'category':category,'illustration':art,'tone':tone}}
