"""Executable Bash snippets. Values are quoted as data, never evaluated as shell code."""
import json
import shlex
from request_transport import TLSOptions, GRPCOptions, HTTPOptions, expand, grpc_metadata, grpc_target, prepare_http, parse_message


def command_groups(data, variables=None):
    variables=variables or {}
    if data.get('kind')=='grpc':
        target=grpc_target(expand(data.get('server',''),variables))
        service=expand(data.get('service',''),variables).lstrip('.')
        method=expand(data.get('rpc_method',''),variables)
        if not service or not method:raise ValueError('Select a gRPC service and method to generate a command.')
        tls=TLSOptions.from_data(data,variables); groups=[['grpcurl']]
        if not tls.enabled:groups.append(['-plaintext'])
        else:
            if bool(tls.cert_file)!=bool(tls.key_file):raise ValueError('Select both a client certificate and its private key for mutual TLS.')
            if not tls.verify_server:groups.append(['-insecure'])
            for flag,value in (('-cacert',tls.ca_file if tls.verify_server else ''),('-cert',tls.cert_file),('-key',tls.key_file)):
                if value:groups.append([flag,value])
        if tls.server_name:groups.append(['-authority',tls.server_name])
        for key,value in grpc_metadata(data,variables):groups.append(['-H',f'{key}: {value}'])
        definition=expand(data.get('descriptor_file',''),variables).strip()
        if definition:groups.append(['-protoset',definition])
        settings=GRPCOptions.from_data(data)
        if settings.emit_defaults:groups.append(['-emit-defaults'])
        if settings.timeout is not None:groups.append(['-max-time',format(settings.timeout,'.3f').rstrip('0').rstrip('.')])
        # grpcurl has no unlimited sentinel; use the protobuf message size ceiling.
        groups.append(['-max-msg-sz',str(settings.receive_limit if settings.receive_limit>0 else 2147483647)])
        try:payload=parse_message(expand(data.get('message') or '{}',variables))
        except ValueError as error:raise ValueError('Complete the Message JSON to generate an executable command.') from error
        # grpcurl consumes a sequence of JSON objects, not an array, for client streaming.
        messages=payload if isinstance(payload,list) else [payload]
        text='\n'.join(json.dumps(value,ensure_ascii=False,separators=(',',':')) for value in messages)
        groups.extend([['-d',text],[target],[f'{service}/{method}']])
        return 'gRPCurl',groups
    url,method,headers,body=prepare_http(data,variables)
    general=data.get('_general')
    if general:
        from http_features import automatic_headers
        headers.update({k:v for k,v in automatic_headers(data,url,headers,body).items() if k.lower() not in ('host','content-length')})
    settings=HTTPOptions.from_data(data,variables)
    groups=[['curl']]
    version=data.get('http_version')
    if version=='HTTP/2':groups.append(['--http2-prior-knowledge'])
    elif version=='HTTP/1.x':groups.append(['--http1.1'])
    if settings.follow_redirects:groups.append(['--location'])
    if settings.follow_redirects and 'http_max_redirects' in data:groups.append(['--max-redirs',str(data['http_max_redirects'])])
    if data.get('http_follow_original_method'):
        groups.extend([[flag] for flag in ('--post301','--post302','--post303')])
    if data.get('http_follow_auth'):groups.append(['--location-trusted'])
    groups.extend([['--globoff'],['--request',method],['--url',url],['--max-time',format(settings.timeout,'g')]])
    if not settings.verify:groups.append(['--insecure'])
    for flag,value in (('--cacert',settings.ca_file if settings.verify else ''),('--cert',settings.cert_file),('--key',settings.key_file)):
        if value:groups.append([flag,value])
    if method=='HEAD':
        if body is not None:raise ValueError('Select Body: none to generate a cURL HEAD command.')
        groups.append(['--head'])
    for key,value in headers.items():
        if data.get('body_type')=='form-data' and key.lower() in ('content-type','content-length'):continue
        groups.append(['--header',f'{key}: {value}'])
    if any(key.lower()=='accept-encoding' and value.strip().lower()!='identity' for key,value in headers.items()):groups.append(['--compressed'])
    if data.get('body_type') in ('file','binary'):
        name=expand(data.get('body_file',''),variables)
        if general:
            from general_settings import resolve_file
            name=str(resolve_file(name,general))
        groups.append(['--data-binary','@'+name])
    elif data.get('body_type')=='form-data':
        for row in data.get('multipart_rows',[]):
            if not row[0] or len(row)>3 and not row[3]:continue
            key=expand(row[0],variables);value=expand(row[1],variables);file=len(row)>4 and row[4]=='File'
            if file:
                from http_features import read_body_file
                value=str(read_body_file(value,data,variables))
                # curl's multipart syntax has its own quoting, inside shell quoting.
                value='@"'+value.replace('\\','\\\\').replace('"','\\"')+'"'
            groups.append(['--form' if file else '--form-string',key+'='+value])
    elif body is not None:groups.append(['--data-raw',body.decode('utf-8')])
    return 'cURL',groups


def generate_snippet(data,variables=None):
    label,groups=command_groups(data,variables)
    return label,' \\\n  '.join(' '.join(shlex.quote(value) for value in group) for group in groups)
