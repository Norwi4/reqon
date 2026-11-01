"""HTTP editor options shared by request previews, snippets and transport."""
import base64
import json
import mimetypes
import uuid
from urllib.parse import urlsplit
from ui_brand import VERSION

RAW_TYPES={'Text':'text/plain','JavaScript':'application/javascript','JSON':'application/json','HTML':'text/html','XML':'application/xml'}
JWT_ALGORITHMS=('HS256','HS384','HS512','RS256','RS384','RS512','PS256','PS384','PS512','ES256','ES384','ES512','EdDSA')
REQUEST_DEFAULTS={
    'http_follow_original_method':False,'http_follow_auth':False,'http_remove_referer':False,
    'http_strict_parser':False,'http_encode_url':True,'http_disable_cookies':False,
    'http_server_cipher_order':False,'http_max_redirects':10,'http_disabled_tls':'','http_ciphers':'',
}


def jwt_token(data,variables):
    from request_transport import expand,parse_message
    if data.get('_preview'):return '<calculated when request is sent>'
    import jwt
    algorithm=data.get('jwt_algorithm','HS256')
    if algorithm not in JWT_ALGORITHMS:raise ValueError('Select a supported JWT signing algorithm.')
    payload=parse_message(expand(data.get('jwt_payload','{}') or '{}',variables))
    headers=parse_message(expand(data.get('jwt_headers','{}') or '{}',variables))
    if not isinstance(payload,dict) or not isinstance(headers,dict):raise ValueError('JWT payload and headers must be JSON objects.')
    if 'alg' in headers and headers['alg']!=algorithm:raise ValueError('JWT header alg must match the selected algorithm.')
    secret=expand(data.get('jwt_secret',''),variables)
    if not secret:raise ValueError('Enter a JWT secret or private key.')
    if data.get('jwt_secret_base64'):
        try:secret=base64.b64decode(secret,validate=True)
        except ValueError as error:raise ValueError('JWT secret is not valid Base64.') from error
    try:return jwt.encode(payload,secret,algorithm=algorithm,headers=headers)
    except (ValueError,TypeError,jwt.PyJWTError) as error:raise ValueError('Cannot sign JWT: '+str(error)) from error


def read_body_file(name,data,variables):
    from request_transport import expand
    from general_settings import resolve_file,GeneralConfig
    return resolve_file(expand(name,variables),data.get('_general') or GeneralConfig(working_directory=str(__import__('pathlib').Path.cwd())))


def multipart_body(data,variables,load_files):
    from request_transport import expand
    if not load_files:return None,'multipart/form-data'
    boundary='ReqonBoundary'+uuid.uuid4().hex;parts=[]
    def quoted(value):return str(value).replace('\r','').replace('\n','').replace('\\','\\\\').replace('"','\\"')
    for row in data.get('multipart_rows',[]):
        if not row[0] or len(row)>3 and not row[3]:continue
        key=quoted(expand(row[0],variables));value=expand(row[1],variables);kind=row[4] if len(row)>4 else 'Text'
        disposition=f'Content-Disposition: form-data; name="{key}"'
        if kind=='File':
            path=read_body_file(value,data,variables);content=path.read_bytes()
            disposition+=f'; filename="{quoted(path.name)}"\r\nContent-Type: {mimetypes.guess_type(path.name)[0] or "application/octet-stream"}'
        else:content=value.encode('utf-8')
        parts.extend([f'--{boundary}\r\n{disposition}\r\n\r\n'.encode('utf-8'),content,b'\r\n'])
    parts.append(f'--{boundary}--\r\n'.encode())
    return b''.join(parts),'multipart/form-data; boundary='+boundary


def automatic_headers(data,url,headers,body,preview=False):
    """Return generated headers; explicit user values always take precedence."""
    config=data.get('_general');disabled={name.lower() for name in data.get('disabled_headers',[])}
    explicit={name.lower() for name in headers};generated={}
    def add(name,value,required=False):
        if name.lower() not in explicit and (required or name.lower() not in disabled):generated[name]=value
    if getattr(config,'no_cache',True):add('Cache-Control','no-cache')
    if getattr(config,'postman_token',True):add('Postman-Token','<calculated when request is sent>' if preview else str(uuid.uuid4()))
    add('Host',urlsplit(url).netloc or '<calculated when request is sent>',True)
    add('User-Agent','Reqon/'+VERSION);add('Accept','*/*');add('Accept-Encoding','gzip, deflate')
    if data.get('http_version',getattr(config,'http_version','HTTP/1.x'))!='HTTP/2':add('Connection','keep-alive')
    if body is not None:add('Content-Length',str(len(body)),True)
    elif data.get('body_type') in ('file','binary','form-data'):add('Content-Length','<calculated when request is sent>',True)
    return generated


def preview_headers(data,variables):
    from request_transport import prepare_http
    incoming=dict(data,_preview=True,disabled_headers=[],url=data.get('url') or 'https://reqon.invalid')
    try:url,_,headers,body=prepare_http(incoming,variables)
    except (ValueError,OSError):
        url=data.get('url','');headers={r[0]:r[1] for r in data.get('header_rows',data.get('headers',[])) if r[0] and (len(r)<4 or r[3])};body=None
    generated=automatic_headers(dict(incoming,disabled_headers=[]),url,headers,body,True)
    manual={r[0].lower() for r in data.get('header_rows',data.get('headers',[])) if r[0] and (len(r)<4 or r[3])}
    generated.update({name:value for name,value in headers.items() if name.lower() not in manual})
    if not data.get('url'):generated['Host']='<calculated when request is sent>'
    if 'Authorization' in generated:generated['Authorization']='<calculated when request is sent>'
    return generated


def configure_tls(context,data,variables):
    import ssl
    from request_transport import expand
    protocols={'TLSv1':ssl.OP_NO_TLSv1,'TLSv1.1':ssl.OP_NO_TLSv1_1,'TLSv1.2':ssl.OP_NO_TLSv1_2,'TLSv1.3':ssl.OP_NO_TLSv1_3}
    for name in expand(data.get('http_disabled_tls',''),variables).split(','):
        name=name.strip()
        if not name:continue
        if name not in protocols:raise ValueError('Unknown TLS protocol: '+name)
        context.options|=protocols[name]
    if data.get('http_server_cipher_order'):context.options|=ssl.OP_CIPHER_SERVER_PREFERENCE
    else:context.options&=~ssl.OP_CIPHER_SERVER_PREFERENCE
    ciphers=expand(data.get('http_ciphers',''),variables).strip()
    if ciphers:
        try:context.set_ciphers(ciphers)
        except ssl.SSLError as error:raise ValueError('Invalid TLS cipher suite selection: '+str(error)) from error
