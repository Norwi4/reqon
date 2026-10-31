import threading
from concurrent import futures
import grpc
from google.protobuf import descriptor_pb2,descriptor_pool,message_factory
from grpc_reflection.v1alpha import reflection


class ReflectionServer:
    def __init__(self,package='picker',credentials=None):
        self.gate=threading.Event(); self.gate.set(); self.disabled=False; self.required_token=None; self.calls=[]
        owner=self
        class Interceptor(grpc.ServerInterceptor):
            def intercept_service(self,continuation,details):
                if details.method.startswith('/grpc.reflection.'):
                    owner.calls.append(tuple(details.invocation_metadata)); owner.gate.wait(4)
                    code=None; message=''
                    if owner.disabled:code=grpc.StatusCode.UNIMPLEMENTED;message='Reflection is disabled for this server.'
                    elif owner.required_token and dict(details.invocation_metadata).get('authorization')!=owner.required_token:
                        code=grpc.StatusCode.UNAUTHENTICATED;message='A bearer token is required.'
                    if code:
                        def fail(requests,context):context.abort(code,message)
                        return grpc.stream_stream_rpc_method_handler(fail)
                return continuation(details)
        descriptor=descriptor_pb2.FileDescriptorProto(name=f'{package}_reflection.proto',package=package,syntax='proto3')
        message=descriptor.message_type.add(name='Request'); message.field.add(name='name',number=1,label=1,type=9)
        services={'Customers':['GetCustomer','ListCustomers'],'Orders':['GetOrder','ListOrders'],'Archive':['GetCustomer']}
        for name,methods in services.items():
            service=descriptor.service.add(name=name)
            for method in methods:service.method.add(name=method,input_type=f'.{package}.Request',output_type=f'.{package}.Request')
        pool=descriptor_pool.Default();pool.Add(descriptor);cls=message_factory.GetMessageClass(pool.FindMessageTypeByName(f'{package}.Request'))
        self.server=grpc.server(futures.ThreadPoolExecutor(max_workers=8),interceptors=(Interceptor(),))
        for service,methods in services.items():
            handlers={method:grpc.unary_unary_rpc_method_handler(lambda request,context:request,request_deserializer=cls.FromString,response_serializer=lambda response:response.SerializeToString()) for method in methods}
            self.server.add_generic_rpc_handlers((grpc.method_handlers_generic_handler(f'{package}.{service}',handlers),))
        reflection.enable_server_reflection(tuple(f'{package}.{name}' for name in services)+(reflection.SERVICE_NAME,),self.server)
        self.port=self.server.add_secure_port('127.0.0.1:0',credentials) if credentials else self.server.add_insecure_port('127.0.0.1:0')
        self.server.start();self.address=f'{"localhost" if credentials else "127.0.0.1"}:{self.port}'
    def close(self):self.gate.set();self.server.stop(0).wait(5)
