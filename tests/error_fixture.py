"""Local gRPC service with real application failures and metadata."""
import time
from concurrent import futures
import grpc
from google.protobuf import descriptor_pb2,descriptor_pool,message_factory
from grpc_reflection.v1alpha import reflection

SERVER_DETAILS='Ошибка REQUEST_SOURCE_ABSENCE_ERROR. Входящий вызов не имеет заголовка request_source и будет немедленно прерван.'


class ErrorServer:
    def __init__(self):
        file=descriptor_pb2.FileDescriptorProto(name='response_errors.proto',package='errors',syntax='proto3')
        message=file.message_type.add(name='Message');message.field.add(name='name',number=1,label=1,type=9)
        service=file.service.add(name='API')
        for name in ('Reject','Unavailable','Slow','Echo'):
            service.method.add(name=name,input_type='.errors.Message',output_type='.errors.Message')
        pool=descriptor_pool.DescriptorPool();pool.Add(file)
        Message=message_factory.GetMessageClass(pool.FindMessageTypeByName('errors.Message'))
        def reject(request,context):
            context.send_initial_metadata((('source','local-test'),))
            context.set_trailing_metadata((('request-id','test-42'),('diagnostic-bin',b'\x00\xff')))
            context.abort(grpc.StatusCode.FAILED_PRECONDITION,SERVER_DETAILS)
        def unavailable(request,context):context.abort(grpc.StatusCode.UNAVAILABLE,'Service temporarily unavailable.')
        def slow(request,context):
            while context.is_active():time.sleep(.01)
            return request
        handlers={name:grpc.unary_unary_rpc_method_handler(fn,request_deserializer=Message.FromString,response_serializer=lambda x:x.SerializeToString())
            for name,fn in [('Reject',reject),('Unavailable',unavailable),('Slow',slow),('Echo',lambda request,context:request)]}
        self.server=grpc.server(futures.ThreadPoolExecutor(max_workers=4))
        self.server.add_generic_rpc_handlers((grpc.method_handlers_generic_handler('errors.API',handlers),))
        reflection.enable_server_reflection(('errors.API',reflection.SERVICE_NAME),self.server,pool=pool)
        self.port=self.server.add_insecure_port('127.0.0.1:0');self.server.start()
    def data(self,method='Reject'):
        return {'kind':'grpc','server':f'127.0.0.1:{self.port}','service':'errors.API','rpc_method':method,'message':'{"name":"local test"}'}
    def close(self):self.server.stop(0).wait(5)
