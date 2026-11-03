"""Response formatting shared by the worker and the response view."""
import json


def signature(grpc, config):
    return (grpc, config.response_format, config.indent)


def format_body(result, grpc, config):
    text = result['body']
    headers = result.get('headers', {})
    entries = headers.items() if isinstance(headers, dict) else headers
    media = next((str(v).lower() for k,v in entries if str(k).lower() == 'content-type'), '')
    if grpc or config.response_format == 'JSON' or 'json' in media:
        try: return json.dumps(json.loads(text), ensure_ascii=False, indent=config.indent)
        except (ValueError, TypeError): pass
    elif 'xml' in media:
        try:
            from xml.dom.minidom import parseString
            return parseString(text).toprettyxml(indent=config.indent)
        except Exception: pass
    return text


def prepare_response(result, grpc, config):
    if 'error' not in result:
        result['display_body'] = format_body(result, grpc, config)
        result['display_format'] = signature(grpc, config)
    return result
