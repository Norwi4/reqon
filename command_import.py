"""Parse pasted curl/grpcurl as data. Never invoke a shell, read files or connect."""
import base64
import json
import math
import re
from dataclasses import dataclass
from urllib.parse import parse_qsl, quote, unquote, urlencode, urlsplit, urlunsplit


@dataclass
class ImportedRequest:
    data: dict
    notes: list[str]


def split_command(text, shell='auto'):
    text = text.strip().replace('\r\n', '\n')
    if not text:
        raise ValueError('Paste a cURL or gRPCurl command.')
    if len(text) > 2_000_000:
        raise ValueError('The command is too large (maximum 2 MB).')
    if shell == 'auto':
        shell = 'cmd' if re.search(r'\^\s*\n', text) else 'powershell' if re.search(r'`\s*\n', text) else 'bash'
    tokens, buf, state, started, i = [], [], '', False, 0
    escapes = {'n':'\n', 'r':'\r', 't':'\t', 'a':'\a', 'b':'\b', 'f':'\f', 'v':'\v', '\\':'\\', "'":"'", '"':'"'}
    while i < len(text):
        c = text[i]
        if state == 'ansi':
            if c == "'":
                state = ''; i += 1; continue
            if c == '\\':
                match = re.match(r'\\(?:x([0-9a-fA-F]{1,2})|u([0-9a-fA-F]{4})|U([0-9a-fA-F]{8})|([0-7]{1,3}))', text[i:])
                if match:
                    digits = next(group for group in match.groups() if group is not None)
                    try: buf.append(chr(int(digits, 8 if match.group(4) else 16)))
                    except ValueError: raise ValueError('Invalid character escape in the command.') from None
                    i += len(match.group()); continue
                if i + 1 < len(text):
                    nxt = text[i+1]; buf.append(escapes.get(nxt, '\\'+nxt)); i += 2; continue
            buf.append(c); i += 1; continue
        if state == "'":
            if c == "'":
                if shell == 'powershell' and text[i:i+2] == "''":
                    buf.append("'"); i += 2; continue
                state = ''
            else: buf.append(c)
            i += 1; continue
        if shell == 'cmd' and text[i:i+4] == '^\\^"':
            buf.append('"'); started = True; i += 4; continue
        escape = '`' if shell == 'powershell' else '^' if shell == 'cmd' else '\\'
        if c == escape:
            if i + 1 == len(text): raise ValueError('The command ends with an incomplete escape.')
            nxt = text[i+1]
            if nxt == '\n': i += 2; continue
            if shell == 'bash' and state == '"' and nxt not in '\\"$`':
                buf.append(c); i += 1; continue
            buf.append(escapes.get(nxt, nxt) if shell == 'powershell' and state == '"' else nxt)
            started = True; i += 2; continue
        if shell == 'cmd' and state == '"' and text[i:i+2] == '\\"':
            buf.append('"'); i += 2; continue
        if c == '"':
            state = '' if state == '"' else '"'; started = True; i += 1; continue
        if c == '`' or text[i:i+2] == '$(':
            raise ValueError('Shell substitutions are not supported. Paste the actual values instead.')
        if not state:
            if shell == 'bash' and text[i:i+2] == "$'":
                state = 'ansi'; started = True; i += 2; continue
            if c == "'" and shell != 'cmd':
                state = "'"; started = True; i += 1; continue
            if c.isspace():
                if started: tokens.append(''.join(buf)); buf = []; started = False
                i += 1; continue
            if c in ';|&<>':
                raise ValueError('Paste one command without pipes or redirection; quote URLs and values containing shell operators.')
            if c == '#' and not started:
                end = text.find('\n', i); i = len(text) if end < 0 else end; continue
        buf.append(c); started = True; i += 1
    if state: raise ValueError('Close the quotation marks in the command.')
    if started: tokens.append(''.join(buf))
    if any('\0' in token for token in tokens): raise ValueError('NUL bytes cannot be imported into a text request.')
    return tokens


def options(tokens, values, flags, short=False):
    result, positional = [], []
    i = 0
    while i < len(tokens):
        token = tokens[i]; i += 1
        if token == '--': positional.extend(tokens[i:]); break
        if not token.startswith('-') or token == '-': positional.append(token); continue
        key, sep, value = token.partition('=')
        if key in values:
            if not sep:
                if i >= len(tokens): raise ValueError(f'{key} needs a value.')
                value = tokens[i]; i += 1
            result.append((values[key], value)); continue
        if key in flags:
            if sep: raise ValueError(f'{key} does not take a value.')
            result.append((flags[key], None)); continue
        if short and token.startswith('-') and not token.startswith('--'):
            chars = token[1:]
            while chars:
                key, chars = '-'+chars[0], chars[1:]
                if key in values:
                    if chars: value, chars = chars, ''
                    elif i < len(tokens): value = tokens[i]; i += 1
                    else: raise ValueError(f'{key} needs a value.')
                    result.append((values[key], value)); break
                if key not in flags: raise ValueError(f'Unsupported option: {key}. Remove it or configure this request manually.')
                result.append((flags[key], None))
            continue
        raise ValueError(f'Unsupported option: {key}. Remove it or configure this request manually.')
    return result, positional


def inline(value, option):
    if value.startswith('@'):
        raise ValueError(f'{option} references a file or stdin. Paste its contents as an inline value instead.')
    return value


def number(value, option, maximum=2147483.647):
    try: result = float(value)
    except ValueError: raise ValueError(f'{option} needs a non-negative number.') from None
    if not math.isfinite(result) or not 0 <= result <= maximum:
        raise ValueError(f'{option} must be between 0 and {maximum}.')
    return result


def header(value):
    inline(value, 'Header')
    key, colon, val = value.partition(':')
    if not colon and value.endswith(';'): key, val, colon = value[:-1], '', ':'
    if not colon or not key.strip() or any(c in value for c in '\r\n'):
        raise ValueError('Headers must use the form "Name: value".')
    return [key.strip(), val.lstrip(), '']


def authorization(data, rows, grpc=False):
    matches = [row for row in rows if row[0].lower() == 'authorization']
    if len(matches) != 1: return
    row = matches[0]; scheme, _, value = row[1].partition(' ')
    if scheme.lower() == 'bearer':
        data.update(auth_type='Bearer Token', auth_value=value); rows.remove(row)
    elif scheme.lower() == 'basic' and not grpc:
        try: decoded = base64.b64decode(value, validate=True).decode('utf-8')
        except (ValueError, UnicodeError): return
        if ':' in decoded:
            data.update(auth_type='Basic Auth', auth_value=decoded); rows.remove(row)


def encoded_data(value):
    if value.startswith('='): return quote(value[1:], safe='')
    key, sep, content = value.partition('=')
    if sep: return key+'='+quote(content, safe='')
    if '@' in value: raise ValueError('--data-urlencode references a file. Paste its contents inline instead.')
    return quote(value, safe='')


def import_curl(tokens):
    values = {'-X':'method', '--request':'method', '--url':'url', '-H':'header', '--header':'header',
              '-d':'data', '--data':'data', '--data-ascii':'data', '--data-raw':'raw', '--data-binary':'binary',
              '--data-urlencode':'encoded', '--json':'json', '-u':'user', '--user':'user',
              '--oauth2-bearer':'bearer', '-b':'cookie', '--cookie':'cookie', '-A':'agent', '--user-agent':'agent',
              '-e':'referer', '--referer':'referer', '-m':'timeout', '--max-time':'timeout',
              '--cacert':'ca', '--cert':'cert', '-E':'cert', '--key':'key', '--url-query':'query',
              '-F':'multipart','--form':'multipart','--form-string':'form_string','--max-redirs':'max_redirects',
              '-o':'output', '--output':'output', '-w':'output', '--write-out':'output'}
    flags = {'-G':'get', '--get':'get', '-I':'head', '--head':'head', '-L':'redirect', '--location':'redirect',
             '--no-location':'no_redirect', '-k':'insecure', '--insecure':'insecure', '--no-insecure':'secure',
             '--compressed':'compressed', '--basic':'basic', '-g':'ignore', '--globoff':'ignore',
             '--http1.1':'http1', '--http2':'http_auto', '--http2-prior-knowledge':'http2',
             '-s':'ignore', '--silent':'ignore', '-S':'ignore', '--show-error':'ignore', '-v':'ignore', '--verbose':'ignore',
             '-i':'ignore', '--include':'ignore', '--show-headers':'ignore', '-f':'ignore', '--fail':'ignore',
             '--location-trusted':'trusted','--post301':'original_method','--post302':'original_method','--post303':'original_method',
             '--fail-with-body':'ignore', '--no-progress-meter':'ignore', '-N':'ignore', '--no-buffer':'ignore'}
    opts, urls = options(tokens, values, flags, short=True)
    data = dict(kind='http', name='Imported request', method='GET', url='', params=[], headers=[],
                auth_type='No Auth', auth_value='', body_type='none', body='', form=[],
                http_verify=True, http_follow_redirects=False, http_timeout=0.0,
                http_ca_file='', http_cert_file='', http_key_file='')
    notes, chunks, query, cookies = [], [], [], []
    multipart=[];binary_file=None
    explicit, get, head, json_body, compressed, has_body = None, False, False, False, False, False
    for key, value in opts:
        if key == 'url': urls.append(value)
        elif key == 'method': explicit = value
        elif key == 'header': data['headers'].append(header(value))
        elif key in ('data','raw','binary','encoded','json'):
            if key=='binary' and value.startswith('@'):
                if value=='@-' or binary_file is not None or chunks:raise ValueError('Import one binary file, without stdin or inline data.')
                binary_file=value[1:];has_body=True;continue
            if binary_file is not None:raise ValueError('A binary file cannot be combined with inline data.')
            if key != 'raw': inline(value, '--'+key)
            if key == 'encoded': value = encoded_data(value)
            if chunks and key != 'json': chunks.append('&')
            chunks.append(value); has_body = True; json_body |= key == 'json'
        elif key in ('multipart','form_string'):
            name,sep,content=value.partition('=')
            if not sep or not name:raise ValueError('Form fields must use name=value.')
            kind='Text'
            if key=='multipart' and content.startswith('@'):
                content=content[1:];kind='File'
                if content.startswith('"') and content.endswith('"'):content=content[1:-1].replace('\\"','"').replace('\\\\','\\')
                elif any(char in content for char in ',;'):raise ValueError('Use one file per form field without curl file modifiers.')
                if not content or content=='-':raise ValueError('Select a local file instead of stdin.')
            elif key=='multipart' and (content.startswith('<') or ';type=' in content):raise ValueError('Paste a plain form value or use --form-string.')
            multipart.append([name,content,'',True,kind]);has_body=True
        elif key == 'query': query.append(value[1:] if value.startswith('+') else encoded_data(value))
        elif key == 'get': get = True
        elif key == 'head': head = True
        elif key == 'user':
            if ':' not in value: raise ValueError('--user needs username:password (use username: for an empty password).')
            data.update(auth_type='Basic Auth', auth_value=value)
        elif key == 'bearer': data.update(auth_type='Bearer Token', auth_value=value)
        elif key == 'cookie':
            if '=' not in value: raise ValueError('--cookie must contain inline cookies, for example "session=value"; cookie files are not imported.')
            cookies.append(value)
        elif key in ('agent','referer'): data['headers'].append(['User-Agent' if key=='agent' else 'Referer', value, ''])
        elif key in ('insecure','secure'): data['http_verify'] = key == 'secure'
        elif key in ('redirect','no_redirect'): data['http_follow_redirects'] = key == 'redirect'
        elif key == 'timeout': data['http_timeout'] = number(value, '--max-time')
        elif key == 'max_redirects':data['http_max_redirects']=int(number(value,'--max-redirs',100))
        elif key == 'trusted':data.update(http_follow_redirects=True,http_follow_auth=True)
        elif key == 'original_method':data['http_follow_original_method']=True
        elif key in ('ca','cert','key'): data['http_'+key+'_file'] = value
        elif key == 'compressed': compressed = True
        elif key in ('http1','http_auto','http2'):data['http_version']={'http1':'HTTP/1.x','http_auto':'Auto','http2':'HTTP/2'}[key]
        elif key == 'output': notes.append('Output formatting/file options are omitted; the response appears in Reqon.')
    if len(urls) != 1: raise ValueError('Import one request URL at a time.')
    url = urls[0]
    if '://' not in url: url = 'http://'+url
    try:
        parts = urlsplit(url); parts.port
    except ValueError: raise ValueError('The request URL is invalid.') from None
    if parts.scheme not in ('http','https') or not parts.hostname:
        raise ValueError('cURL import supports HTTP and HTTPS URLs.')
    if parts.username is not None:
        if data['auth_type'] == 'No Auth':
            data.update(auth_type='Basic Auth', auth_value=unquote(parts.username)+':'+unquote(parts.password or ''))
        parts = parts._replace(netloc=parts.netloc.rsplit('@',1)[1])
    if cookies: data['headers'].append(['Cookie', '; '.join(cookies), ''])
    # HTTP currently sends one value per header name. Make this limitation explicit.
    seen = set()
    for row in data['headers']:
        name = row[0].lower()
        if name in seen: raise ValueError(f'Repeated HTTP header "{row[0]}" cannot be sent separately by Reqon. Combine its values before importing.')
        seen.add(name)
    authorization(data, data['headers'])
    def default_header(name, value):
        if not any(row[0].lower()==name.lower() for row in data['headers']): data['headers'].append([name,value,''])
    payload = ''.join(chunks)
    if multipart and (chunks or binary_file is not None):raise ValueError('Multipart fields cannot be combined with another body type.')
    if get and (multipart or binary_file is not None):raise ValueError('--get cannot be combined with file or multipart bodies.')
    if get and has_body: query.append(payload); has_body = False
    if head and has_body: raise ValueError('A HEAD request cannot be imported with a request body.')
    method = explicit or ('HEAD' if head else 'GET' if get else 'POST' if has_body else 'GET')
    if not re.fullmatch(r"[!#$%&'*+.^_`|~0-9A-Za-z-]+", method): raise ValueError('The HTTP method is invalid.')
    data['method'] = method
    if parts.query: query.insert(0, parts.query)
    raw_query = '&'.join(query)
    data['params'] = [[k,v,''] for k,v in parse_qsl(raw_query, keep_blank_values=True)]
    # Preserve encodings/signatures and bare query keys until the user edits Params.
    data['imported_query'] = raw_query
    data['imported_query_params'] = [[k,v] for k,v,*_ in data['params'] if k]
    data['url'] = urlunsplit((parts.scheme, parts.netloc, parts.path, '', ''))
    if multipart:data.update(body_type='form-data',multipart_rows=multipart)
    elif binary_file is not None:data.update(body_type='binary',body_file=binary_file)
    elif has_body:
        if json_body: default_header('Content-Type','application/json'); default_header('Accept','application/json')
        else: default_header('Content-Type','application/x-www-form-urlencoded')
        content_type = next((row[1].split(';')[0].strip().lower() for row in data['headers'] if row[0].lower()=='content-type'), '')
        form = parse_qsl(payload, keep_blank_values=True)
        # Only use the form editor if serializing its rows preserves the exact bytes.
        if content_type=='application/x-www-form-urlencoded' and payload and urlencode(form)==payload and all(k for k,v in form):
            data.update(body_type='x-www-form-urlencoded', form=[[k,v,''] for k,v in form])
        else: data.update(body_type='raw', body=payload)
    if compressed: default_header('Accept-Encoding','gzip, deflate')
    data['name'] = method+' '+(parts.path or '/')
    return ImportedRequest(data, list(dict.fromkeys(notes)))


def import_grpcurl(tokens):
    values = {'-H':'header', '-rpc-header':'header', '-d':'message', '-cacert':'tls_ca_file', '-cert':'tls_cert_file',
              '-key':'tls_key_file', '-authority':'tls_server_name', '-protoset':'descriptor_file',
              '-max-time':'timeout', '-max-msg-sz':'size', '-format':'format'}
    flags = {'-plaintext':'plaintext', '-insecure':'insecure', '-emit-defaults':'defaults', '-v':'ignore', '-vv':'ignore'}
    opts, args = options(tokens, values, flags)
    if len(args)!=2 or args[1] in ('list','describe'): raise ValueError('Paste a gRPCurl invocation with a server and Service/Method.')
    server, method = args
    method = method.lstrip('/')
    service, sep, rpc = method.rpartition('/')
    if not sep: service, sep, rpc = method.rpartition('.')
    if not service or not re.fullmatch(r'[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*',service) or not re.fullmatch(r'[A-Za-z_]\w*',rpc):
        raise ValueError('Use a gRPC method in the form package.Service/Method.')
    from request_transport import grpc_target
    server = grpc_target(server)
    data = dict(kind='grpc', name=rpc, server=server, service=service, rpc_method=rpc, message='{}', metadata=[],
                tls=True, tls_verify=True, auth_type='No Auth', auth_value='', descriptor_file='',
                grpc_emit_defaults=False, grpc_timeout_ms=0, grpc_max_response_mb=4)
    notes = []
    for key,value in opts:
        if key=='header': data['metadata'].append(header(value))
        elif key=='message':
            if value=='@': raise ValueError('gRPCurl -d @ reads stdin. Paste the Message JSON inline instead.')
            data['message'] = value
        elif key=='plaintext': data['tls'] = False
        elif key=='insecure': data['tls_verify'] = False
        elif key=='defaults': data['grpc_emit_defaults'] = True
        elif key=='timeout': data['grpc_timeout_ms'] = round(number(value,'-max-time')*1000)
        elif key=='size':
            size=number(value,'-max-msg-sz',2147483647)
            if not size.is_integer(): raise ValueError('-max-msg-sz needs a whole number of bytes.')
            data['grpc_max_response_mb'] = min(2047,max(1,math.ceil(size/1048576))) if size else 4
            if size==2147483647:data['grpc_max_response_mb']=0
            elif size and size%1048576: notes.append('Maximum response size is rounded up to a supported whole MB.')
        elif key=='format':
            if value!='json': raise ValueError('Only gRPCurl JSON messages are supported.')
        elif key!='ignore': data[key] = value
    authorization(data,data['metadata'],grpc=True)
    # A streaming grpcurl payload is consecutive JSON values; Reqon uses an array.
    decoder=json.JSONDecoder(); rest=data['message'].strip(); messages=[]
    try:
        while rest:
            value,end=decoder.raw_decode(rest); messages.append(value); rest=rest[end:].lstrip()
    except ValueError: pass  # Preserve templates/comments for the editor to resolve on send.
    else:
        if len(messages)>1: data['message']=json.dumps(messages,ensure_ascii=False,indent=2)
    if bool(data.get('tls_cert_file')) != bool(data.get('tls_key_file')):
        notes.append('Select both the client certificate and its PEM private key in Settings before sending.')
    return ImportedRequest(data,notes)


def parse_command(text, shell='auto'):
    tokens = split_command(text, shell)
    if not tokens: raise ValueError('Paste a cURL or gRPCurl command.')
    executable = tokens.pop(0).replace('\\','/').rsplit('/',1)[-1].lower().removesuffix('.exe')
    if executable not in ('curl','grpcurl'): raise ValueError('The command must start with curl or grpcurl.')
    result = import_curl(tokens) if executable=='curl' else import_grpcurl(tokens)
    if any(re.search(r'\$(?:[A-Za-z_]|\{)', token) for token in tokens):
        result.notes.append('Shell variables are kept literally. Replace them with values or Reqon {{variables}} before sending.')
    return result
