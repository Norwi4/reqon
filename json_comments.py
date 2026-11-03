"""JSON editor comments without changing string values or diagnostic positions."""
import json
import re

STRING=r'"(?:\\.|[^"\\])*"'
TOKENS=re.compile(STRING+r'|//[^\r\n]*|/\*[\s\S]*?\*/|[{}\[\],:]|[^/\s{}\[\],:]+')


def comment_ranges(text,in_block=False,allow_incomplete=False):
    """Return comment spans and continuation state, ignoring quoted strings."""
    if not in_block and '/' not in text:return [],False
    spans=[];i=0;length=len(text)
    while i<length:
        if in_block or text.startswith('/*',i):
            start=i;end=text.find('*/',i if in_block else i+2)
            if end<0:
                if not allow_incomplete:raise json.JSONDecodeError('Unterminated block comment',text,start)
                spans.append((start,length));return spans,True
            i=end+2;spans.append((start,i));in_block=False
        elif text[i]=='"':
            i+=1
            while i<length:
                if text[i]=='\\':i+=2
                elif text[i]=='"':i+=1;break
                else:i+=1
        elif text.startswith('//',i):
            start=i
            while i<length and text[i] not in '\r\n':i+=1
            spans.append((start,i))
        else:i+=1
    return spans,in_block


def strip_comments(text):
    spans,_=comment_ranges(text)
    if not spans:return text
    chars=list(text)
    for start,end in spans:
        for i in range(start,end):
            if chars[i] not in '\r\n':chars[i]=' '
    return ''.join(chars)


def normalize(text):
    # Whitespace replacements retain line/column positions for the editor.
    clean=strip_comments(text)
    return re.sub(STRING+r'|,(?=\s*[}\]])',lambda match:match.group() if match.group().startswith('"') else ' ',clean)


def loads(text):
    # Most request bodies are ordinary JSON. Avoid scanning every character in
    # Python unless comments/trailing commas actually require normalization.
    try:return json.loads(text)
    except json.JSONDecodeError:return json.loads(normalize(text))


def looks_like_json(text):
    if text.lstrip().startswith(('{','[')):return True
    try:return strip_comments(text).lstrip().startswith(('{','['))
    except json.JSONDecodeError:return text.lstrip().startswith(('{','[','/*'))


def format_json(text,indent=2):
    """Format JSONC tokens, preserving comments, order and original string values."""
    parsed=loads(text)
    spans,_=comment_ranges(text)
    if not spans:return json.dumps(parsed,ensure_ascii=False,indent=indent)
    indentation=' '*indent if isinstance(indent,int) else indent
    tokens=list(TOKENS.finditer(text));lines=[];line='';depth=0
    def flush():
        nonlocal line
        if line.strip():lines.append(indentation*depth+line.rstrip())
        line=''
    for index,match in enumerate(tokens):
        token=match.group();previous=tokens[index-1] if index else None
        following=tokens[index+1] if index+1<len(tokens) else None
        if token.startswith(('//','/*')):
            gap=text[previous.end():match.start()] if previous else '\n'
            if '\n' in gap or '\r' in gap:flush()
            line+=(' ' if line and not line.endswith(' ') else '')+token
            if token.startswith('//') or '\n' in token or '\r' in token or (previous and previous.group()==','):flush()
            else:line+=' '
        elif token in ('{','['):
            line+=token
            if not following or following.group() not in ('}',']'):flush()
            depth+=1
        elif token in ('}',']'):
            if previous and previous.group() not in ('{','['):flush()
            depth-=1;line+=token
        elif token==',':
            line+=token
            inline_comment=following and following.group().startswith(('//','/*')) and not any(c in text[match.end():following.start()] for c in '\r\n')
            if not inline_comment:flush()
        elif token==':':line+=': '
        else:line+=token
    flush()
    return '\n'.join(lines)
