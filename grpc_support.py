import json
import time
import hashlib
import tempfile
import threading
from contextlib import contextmanager, nullcontext
from importlib.resources import files
from pathlib import Path

import grpc
from request_transport import TLSOptions, GRPCOptions, grpc_target, grpc_metadata, tls_credentials, parse_message
from google.protobuf import descriptor_pb2, descriptor_pool, json_format, message_factory
from grpc_reflection.v1alpha.proto_reflection_descriptor_database import ProtoReflectionDescriptorDatabase


class ReflectionDatabase(ProtoReflectionDescriptorDatabase):
    """Bound reflection requests so a missing server cannot leave discovery running forever."""
    def __init__(self,channel,metadata=(),deadline=None):
        super().__init__(channel); self.metadata=metadata; self.deadline=min(time.monotonic()+10,deadline) if deadline is not None else time.monotonic()+10
    def _do_one_request(self,request,key):
        remaining=self.deadline-time.monotonic()
        if remaining<=0:raise TimeoutError('Server reflection did not complete within 10 seconds.')
        responses=self._stub.ServerReflectionInfo(iter([request]),timeout=remaining,metadata=self.metadata)
        try:
            response=next(responses)
            if response.WhichOneof('message_response')=='error_response':
                raise ValueError(response.error_response.error_message)
            return response
        finally:responses.cancel()


def channel_for(server, tls, receive_limit=10*1024*1024):
    target=grpc_target(server)
    options=tls if isinstance(tls,TLSOptions) else TLSOptions(bool(tls))
    settings=[('grpc.max_receive_message_length',receive_limit)]
    if options.enabled and not options.verify_server:
        from tls_channel import UnverifiedTLSChannel
        return UnverifiedTLSChannel(target,options,settings)
    if options.server_name:
        settings.extend([('grpc.ssl_target_name_override',options.server_name),('grpc.default_authority',options.server_name)])
    return grpc.secure_channel(target,tls_credentials(options),options=settings) if options.enabled else grpc.insecure_channel(target,options=settings)


class Cancellation:
    """Close the active gRPC channel on Cancel, including an in-flight reflection."""
    def __init__(self):self.lock=threading.Lock(); self.active=None; self.cancelled=False
    def cancel(self):
        with self.lock:self.cancelled=True; active=self.active
        if active is not None:active.close()
    @contextmanager
    def use(self,channel):
        with channel:
            with self.lock:self.active=channel; cancelled=self.cancelled
            try:
                if cancelled:raise ValueError('Request cancelled.')
                yield channel
            finally:
                with self.lock:self.active=None


def discover(server, tls=False, descriptor_file='', metadata=()):
    """Return fully qualified service and method names from reflection or a descriptor set."""
    return discover_contract(server,tls,descriptor_file,metadata)[0]


def discover_contract(server, tls=False, descriptor_file='', metadata=()):
    """Retain the loaded contract so example generation also works after going offline."""
    if descriptor_file:
        bundle = descriptor_pb2.FileDescriptorSet()
        content=Path(descriptor_file).read_bytes(); bundle.ParseFromString(content)
        return ([(f'{file.package}.{service.name}'.strip('.'), method.name)
                 for file in bundle.file for service in file.service for method in service.method],content)
    with channel_for(server, tls) as channel:
        database = ReflectionDatabase(channel,metadata)
        services = database.get_services()
        pool = descriptor_pool.DescriptorPool(database)
        bundle=descriptor_pb2.FileDescriptorSet(); seen=set(); methods=[]
        def include(file):
            if file.name in seen:return
            seen.add(file.name)
            for dependency in file.dependencies:include(dependency)
            file.CopyToProto(bundle.file.add())
        for name in services:
            if name.startswith(('grpc.reflection.v1alpha.','grpc.reflection.v1.')):continue
            service=pool.FindServiceByName(name); include(service.file)
            methods.extend((name,method.name) for method in service.methods)
        return methods,bundle.SerializeToString()


def rpc_error(error,reflection=False):
    if isinstance(error,grpc.RpcError):
        code=error.code(); details=error.details() or ''
        hints={grpc.StatusCode.UNIMPLEMENTED:'This server does not expose gRPC server reflection. Import a local .proto file or enable reflection on the server.',
               grpc.StatusCode.UNAVAILABLE:'Unable to connect to the gRPC server. Check the host, port and TLS setting.',
               grpc.StatusCode.UNAUTHENTICATED:'The server requires authentication. Check Authorization and Metadata.',
               grpc.StatusCode.PERMISSION_DENIED:'The server denied access. Check your credentials.',
               grpc.StatusCode.CANCELLED:'Request cancelled.',
               grpc.StatusCode.RESOURCE_EXHAUSTED:'The server rejected a resource limit or the response exceeds Maximum response message size in Settings.',
               grpc.StatusCode.DEADLINE_EXCEEDED:'Server reflection timed out. Check that the host and port are reachable.' if reflection else 'Connection Timeout exceeded. Adjust the deadline in Settings.'}
        if not reflection:hints[grpc.StatusCode.UNIMPLEMENTED]='The server does not implement this method.'
        hint=hints.get(code,'The gRPC request failed.')
        if 'CERTIFICATE_VERIFY_FAILED' in details or 'certificate verify failed' in details.lower():
            hint='The server certificate could not be verified. Configure its CA certificate, or explicitly turn OFF Settings > Enable server certificate verification to match a client that skips this check. TLS encryption remains enabled.'
        elif 'Peer name' in details and 'not in peer certificate' in details:
            hint='The server name does not match its certificate. Check the host or Overwrite server name for certificate verification in Settings.'
        return f'{code.name}: {hint}\n\n{details}'
    return str(error)


def reflection_error(error):return rpc_error(error,reflection=True)


def compile_proto(source_path,directory):
    """Compile a user-selected schema to a portable descriptor set, without executing generated code."""
    from grpc_tools import protoc
    source=Path(source_path).resolve()
    with tempfile.TemporaryDirectory(prefix='getman-proto-') as temporary:
        output=Path(temporary)/'schema.protoset'
        includes=files('grpc_tools').joinpath('_proto')
        result=protoc.main(['protoc',f'-I{source.parent}',f'-I{includes}','--include_imports',f'--descriptor_set_out={output}',str(source)])
        if result!=0:
            raise ValueError(f'Could not compile {source.name}. Check the protobuf syntax and ensure imported .proto files are available relative to {source.parent}.')
        content=output.read_bytes()
    root=Path(directory); root.mkdir(parents=True,exist_ok=True)
    target=root/(hashlib.sha256(content).hexdigest()+'.protoset'); target.write_bytes(content)
    return str(target)


def _pool(channel, descriptor_file, metadata=(), schema_bytes=None, deadline=None):
    if schema_bytes is None and not descriptor_file:
        return descriptor_pool.DescriptorPool(ReflectionDatabase(channel,metadata,deadline))
    bundle = descriptor_pb2.FileDescriptorSet()
    bundle.ParseFromString(schema_bytes if schema_bytes is not None else Path(descriptor_file).read_bytes())
    pool = descriptor_pool.DescriptorPool()
    pending = list(bundle.file)
    while pending:
        remaining = []
        for file in pending:
            try:
                pool.Add(file)
            except TypeError:
                remaining.append(file)
        if len(remaining) == len(pending):
            raise ValueError('Descriptor set has missing imports or invalid dependencies')
        pending = remaining
    return pool


def describe_method(server, tls, descriptor_file, service_name, method_name, metadata=(), schema_bytes=None):
    with (nullcontext(None) if descriptor_file or schema_bytes is not None else channel_for(server or 'localhost:1',tls)) as channel:
        service = _pool(channel, descriptor_file, metadata,schema_bytes).FindServiceByName(service_name.lstrip('.'))
        method = service.FindMethodByName(method_name)
        if method is None:
            raise ValueError('Method not found')
        types = {1:'double',2:'float',3:'int64',4:'uint64',5:'int32',6:'fixed64',7:'fixed32',8:'bool',9:'string',12:'bytes',13:'uint32',15:'sfixed32',16:'sfixed64',17:'sint32',18:'sint64'}
        def fields(message):
            return [(field.name, field.message_type.name if field.message_type else field.enum_type.name if field.enum_type else types.get(field.type, str(field.type))) for field in message.fields]
        return method.input_type.name, fields(method.input_type), method.output_type.name, fields(method.output_type)


def example_message(server,tls,descriptor_file,service_name,method_name,metadata=(),schema_bytes=None):
    """Build valid ProtoJSON test values, including nested, repeated and well-known fields."""
    with (nullcontext(None) if descriptor_file or schema_bytes is not None else channel_for(server or 'localhost:1',tls)) as channel:
        pool=_pool(channel,descriptor_file,metadata,schema_bytes)
        method=pool.FindServiceByName(service_name.lstrip('.')).FindMethodByName(method_name)
        if method is None:raise ValueError('Method not found')
        def scalar(field):
            if field.enum_type:
                if field.enum_type.full_name=='google.protobuf.NullValue':return None
                values=field.enum_type.values; return values[min(1,len(values)-1)].name
            if field.type==field.TYPE_STRING:return 'example'
            if field.type==field.TYPE_BYTES:return 'ZXhhbXBsZQ=='
            if field.type==field.TYPE_BOOL:return True
            if field.type in (field.TYPE_FLOAT,field.TYPE_DOUBLE):return 1.0
            if field.type in (field.TYPE_INT64,field.TYPE_UINT64,field.TYPE_SINT64,field.TYPE_FIXED64,field.TYPE_SFIXED64):return '1'
            return 1
        def message(descriptor,ancestors=()):
            name=descriptor.full_name
            special={'google.protobuf.Timestamp':'2026-01-01T00:00:00Z', 'google.protobuf.Duration':'1s',
                     'google.protobuf.FieldMask':'exampleField', 'google.protobuf.Empty':{},
                     'google.protobuf.Struct':{'key':'value'}, 'google.protobuf.Value':'example',
                     'google.protobuf.ListValue':['example'], 'google.protobuf.Any':{}}
            if name in special:return special[name]
            if name.startswith('google.protobuf.') and name.rsplit('.',1)[-1] in (
                    'DoubleValue','FloatValue','Int64Value','UInt64Value','Int32Value','UInt32Value','BoolValue','StringValue','BytesValue'):
                return scalar(descriptor.fields_by_name['value'])
            if name in ancestors or len(ancestors)>=8:return {}
            ancestors=(*ancestors,name); result={}; oneofs=set()
            for field in descriptor.fields:
                if field.containing_oneof:
                    group=field.containing_oneof.name
                    if group in oneofs:continue
                    oneofs.add(group)
                if field.message_type and field.message_type.GetOptions().map_entry:
                    key=field.message_type.fields_by_name['key']; value=field.message_type.fields_by_name['value']
                    sample=message(value.message_type,ancestors) if value.message_type else scalar(value)
                    result[field.name]={('true' if key.type==key.TYPE_BOOL else 'key' if key.type==key.TYPE_STRING else '1'):sample}
                    continue
                value=message(field.message_type,ancestors) if field.message_type else scalar(field)
                result[field.name]=[value] if field.is_repeated else value
            return result
        sample=message(method.input_type)
        # Verify the template against the same descriptor before showing it to the user.
        cls=message_factory.GetMessageClass(method.input_type); json_format.ParseDict(sample,cls(),descriptor_pool=pool).SerializeToString()
        return json.dumps([sample] if method.client_streaming else sample,ensure_ascii=False,indent=2)


def invoke(data, variables, expand, cancellation=None):
    started = time.monotonic()
    server = expand(data.get('server', ''), variables)
    service_name = expand(data.get('service', ''), variables).lstrip('.')
    method_name = expand(data.get('rpc_method', ''), variables)
    metadata=grpc_metadata(data,variables)
    settings=GRPCOptions.from_data(data); cancellation=cancellation or Cancellation()
    deadline=started+settings.timeout if settings.timeout is not None else None
    def remaining():
        if deadline is None:return None
        value=deadline-time.monotonic()
        if value<=0:raise TimeoutError('Connection Timeout exceeded. Adjust the deadline in Settings.')
        return value
    with cancellation.use(channel_for(server,TLSOptions.from_data(data,variables),settings.receive_limit)) as channel:
        pool = _pool(channel, expand(data.get('descriptor_file',''),variables),metadata,deadline=deadline)
        service = pool.FindServiceByName(service_name)
        method = service.FindMethodByName(method_name)
        if method is None:
            raise ValueError(f'Method {method_name} not found in {service_name}')
        input_cls = message_factory.GetMessageClass(method.input_type)
        output_cls = message_factory.GetMessageClass(method.output_type)
        payload = parse_message(expand(data.get('message') or '{}', variables))
        route = f'/{service_name}/{method_name}'
        def encode(value):
            return json_format.ParseDict(value, input_cls()).SerializeToString()
        def decode(raw):
            return json_format.MessageToDict(output_cls.FromString(raw), preserving_proto_field_name=True,always_print_fields_with_no_presence=settings.emit_defaults)
        if method.client_streaming:
            if not isinstance(payload, list): raise ValueError('Client streaming requires a JSON array of messages')
            messages = (encode(item) for item in payload)
            call = channel.stream_stream(route, request_serializer=lambda x:x, response_deserializer=decode) if method.server_streaming else channel.stream_unary(route, request_serializer=lambda x:x, response_deserializer=decode)
            if method.server_streaming:
                response = call(messages, timeout=remaining(), metadata=metadata); rpc_call=response
            else: response,rpc_call = call.with_call(messages, timeout=remaining(), metadata=metadata)
        else:
            call = channel.unary_stream(route, request_serializer=lambda x:x, response_deserializer=decode) if method.server_streaming else channel.unary_unary(route, request_serializer=lambda x:x, response_deserializer=decode)
            if method.server_streaming:
                response = call(encode(payload), timeout=remaining(), metadata=metadata); rpc_call=response
            else: response,rpc_call = call.with_call(encode(payload), timeout=remaining(), metadata=metadata)
        result = list(response) if method.server_streaming else response
        headers=list(rpc_call.initial_metadata() or [])
        trailers=list(rpc_call.trailing_metadata() or [])
    body = json.dumps(result, ensure_ascii=False, indent=2)
    return {'status': 0, 'reason': 'OK', 'body': body, 'headers': headers, 'trailers':trailers, 'size': len(body.encode()),
            'url': server, 'duration': round((time.monotonic()-started)*1000)}
