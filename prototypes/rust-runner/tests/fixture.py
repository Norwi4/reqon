"""Independent Python gRPC fixture. The Rust client has no generated fixture types."""
import json
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import grpc
from google.protobuf import descriptor_pb2, descriptor_pool, message_factory
from grpc_reflection.v1alpha import reflection


class WorkflowServer:
    def __init__(self, reflection_enabled=True, reflection_v1=False, credentials=None, store=None, java_profile=False, on_create=None):
        self.java_profile = java_profile
        self.on_create = on_create
        self.store = store
        self.lock = threading.Lock()
        self.objects = {}
        self.calls = []
        self.slow_started = threading.Event()
        self.slow_cancelled = threading.Event()
        self.pool = descriptor_pool.DescriptorPool()
        common = descriptor_pb2.FileDescriptorProto(name="poc_common.proto", package="poc", syntax="proto3")
        info = common.message_type.add(name="ServerCallInfo")
        info.field.add(name="success", number=1, type=8, label=1)
        info.field.add(name="code", number=2, type=9, label=1)
        if java_profile:
            info.field.add(name="result", number=3, type=9, label=1)
            info.field.add(name="result_code", json_name="resultCode", number=4, type=9, label=1)
        self.pool.Add(common)
        root = descriptor_pb2.FileDescriptorProto(name="poc_workflow.proto", package="poc", syntax="proto3")
        root.dependency.append(common.name)
        request = root.message_type.add(name="Request")
        for index, name in enumerate(("name", "id", "payload"), 1):
            request.field.add(name=name, number=index, type=9, label=1)
        response = root.message_type.add(name="Response")
        response.field.add(name="server_call_info", json_name="serverCallInfo", number=1, type=11, type_name=".poc.ServerCallInfo", label=1)
        for index, name in enumerate(("id", "status", "payload"), 2):
            response.field.add(name=name, number=index, type=9, label=1)
        response.field.add(name="count", number=5, type=5, label=1)
        service = root.service.add(name="Workflow")
        methods = ["Create", "Get", "BusinessError", "TransportError", "Slow", "Echo", "EchoError", "Watch"]
        for method in methods:
            service.method.add(name=method, input_type=".poc.Request", output_type=".poc.Response", server_streaming=method == "Watch")
        self.pool.Add(root)
        self.Request = message_factory.GetMessageClass(self.pool.FindMessageTypeByName("poc.Request"))
        self.Response = message_factory.GetMessageClass(self.pool.FindMessageTypeByName("poc.Response"))
        self.server = grpc.server(ThreadPoolExecutor(max_workers=8))
        handlers = {}
        for method in methods:
            factory = grpc.unary_stream_rpc_method_handler if method == "Watch" else grpc.unary_unary_rpc_method_handler
            handlers[method] = factory(lambda req, ctx, method=method: self.handle(method, req, ctx), request_deserializer=self.Request.FromString, response_serializer=lambda msg: msg.SerializeToString())
        self.server.add_generic_rpc_handlers((grpc.method_handlers_generic_handler("poc.Workflow", handlers),))
        if reflection_enabled:
            reflection.enable_server_reflection(("poc.Workflow", reflection.SERVICE_NAME), self.server, pool=self.pool)
            if reflection_v1:
                # v1 and v1alpha carry wire-compatible messages. Alias the independent
                # Python reflection handler to exercise v1 negotiation as well.
                from grpc_reflection.v1alpha import reflection_pb2, reflection_pb2_grpc
                servicer = reflection.ReflectionServicer(("poc.Workflow",), pool=self.pool)
                handler = grpc.stream_stream_rpc_method_handler(servicer.ServerReflectionInfo, request_deserializer=reflection_pb2.ServerReflectionRequest.FromString, response_serializer=lambda v: v.SerializeToString())
                self.server.add_generic_rpc_handlers((grpc.method_handlers_generic_handler("grpc.reflection.v1.ServerReflection", {"ServerReflectionInfo": handler}),))
        self.port = self.server.add_secure_port("127.0.0.1:0", credentials) if credentials else self.server.add_insecure_port("127.0.0.1:0")
        self.endpoint = f"{'https' if credentials else 'http'}://127.0.0.1:{self.port}"
        self.server.start()

    def handle(self, method, request, context):
        context.send_initial_metadata((('x-fixture', 'grpc'),))
        context.set_trailing_metadata((('x-finished', 'yes'),))
        with self.lock:
            self.calls.append((method, request.id, dict(context.invocation_metadata())))
        if method == "TransportError":
            context.abort(grpc.StatusCode.UNAVAILABLE, "fixture infrastructure failure")
        if method == "EchoError":
            context.abort(grpc.StatusCode.UNAVAILABLE, request.payload[::-1])
        if method == "Slow":
            self.slow_started.set()
            context.add_callback(self.slow_cancelled.set)
            while context.is_active():
                time.sleep(0.005)
            return self.Response()
        response = self.Response()
        response.server_call_info.success = method != "BusinessError"
        response.server_call_info.code = "INVALID_INPUT" if method == "BusinessError" else "OK"
        if self.java_profile:
            response.server_call_info.result = 'ERROR' if method == 'BusinessError' else 'SUCCESS'
            response.server_call_info.result_code = 'INVALID_INPUT' if method == 'BusinessError' else 'OK'
        if method == "Create":
            if self.store:
                value = self.store.create(request.name)
                response.id, response.status = value['id'], value['status']
                if self.on_create:
                    self.on_create({'id': response.id, 'kind': 'created'})
                return response
            with self.lock:
                response.id = f"object-{len(self.objects) + 1}"
                self.objects[response.id] = {"name": request.name, "polls": 0}
            response.status = "DRAFT"
            if self.on_create:
                self.on_create({'id': response.id, 'kind': 'created'})
        elif method == "Get":
            if self.store:
                value = self.store.get(request.id)
                if value is None:
                    context.abort(grpc.StatusCode.NOT_FOUND, 'unknown object')
                response.id, response.status, response.count = value['id'], value['status'], value['count']
                return response
            with self.lock:
                obj = self.objects.get(request.id)
                if obj is None:
                    context.abort(grpc.StatusCode.NOT_FOUND, "unknown object")
                obj["polls"] += 1
                response.id = request.id
                response.count = obj["polls"]
                response.status = "CREATED" if obj["polls"] >= 3 and obj["name"] != "never" else "DRAFT"
        else:
            response.id = request.id
            response.payload = request.payload
            response.status = "CREATED"
            response.count = 7
        return iter([response]) if method == "Watch" else response

    def close(self):
        self.server.stop(0).wait(5)


def sample(endpoint):
    return {
        "schema_version": "reqon-prototype/v1", "id": "create-and-check", "endpoint": endpoint,
        "timeout_ms": 10000,
        "steps": [
            {"type": "grpc", "id": "create", "method": "poc.Workflow/Create", "request": {"name": "example"}, "expect": [{"path": "/serverCallInfo/success", "equals": True}], "save": {"objectId": "/id"}},
            {"type": "grpc", "id": "wait-for-created", "method": "poc.Workflow/Get", "request": {"id": "${objectId}"}, "expect": [{"path": "/serverCallInfo/success", "equals": True}, {"path": "/id", "equals": "${objectId}"}], "poll": {"read_only": True, "interval_ms": 20, "timeout_ms": 1000, "until": [{"path": "/status", "equals": "CREATED"}]}, "save": {"pollCount": "/count"}},
            {"type": "javascript", "id": "script-check", "timeout_ms": 100, "script": "assert(response.id === vars.objectId, 'id mismatch'); assert(vars.pollCount === 3, 'numeric variable lost'); return {verified: true};"},
        ],
    }


if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument("--scenario", type=Path, default=Path("artifacts/demo.json"))
    args = parser.parse_args()
    server = WorkflowServer()
    args.scenario.parent.mkdir(parents=True, exist_ok=True)
    args.scenario.write_text(json.dumps(sample(server.endpoint), ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"Fixture listening on {server.endpoint}; scenario: {args.scenario}", flush=True)
    try:
        while True:
            time.sleep(1)
    except KeyboardInterrupt:
        pass
    finally:
        server.close()
