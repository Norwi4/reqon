use crate::model::{Reply, Request as ApiRequest, resolve};
use anyhow::{Context, Result, bail, ensure};
use base64::Engine as _;
use bytes::{Buf, BufMut, Bytes};
use prost::Message;
use prost_reflect::{DescriptorPool, DynamicMessage};
use prost_types::{FileDescriptorProto, FileDescriptorSet};
use std::{collections::BTreeMap, time::Duration};
use tonic::{
    Request, Status,
    client::Grpc,
    codec::{Codec, DecodeBuf, Decoder, EncodeBuf, Encoder},
    transport::{Channel, ClientTlsConfig, Endpoint},
};
use tonic_reflection::pb::v1alpha::{
    ServerReflectionRequest, ServerReflectionResponse, server_reflection_request::MessageRequest,
    server_reflection_response::MessageResponse,
};

#[derive(Clone, Debug)]
pub struct MethodInfo {
    pub name: String,
    pub client_streaming: bool,
    pub server_streaming: bool,
    pub example: String,
}
#[derive(Clone, Default)]
struct RawCodec;
struct RawEncoder;
struct RawDecoder;
impl Codec for RawCodec {
    type Encode = Bytes;
    type Decode = Bytes;
    type Encoder = RawEncoder;
    type Decoder = RawDecoder;
    fn encoder(&mut self) -> RawEncoder {
        RawEncoder
    }
    fn decoder(&mut self) -> RawDecoder {
        RawDecoder
    }
}
impl Encoder for RawEncoder {
    type Item = Bytes;
    type Error = Status;
    fn encode(&mut self, item: Bytes, dst: &mut EncodeBuf<'_>) -> Result<(), Status> {
        dst.put(item);
        Ok(())
    }
}
impl Decoder for RawDecoder {
    type Item = Bytes;
    type Error = Status;
    fn decode(&mut self, src: &mut DecodeBuf<'_>) -> Result<Option<Bytes>, Status> {
        Ok(Some(src.copy_to_bytes(src.remaining())))
    }
}

struct Client {
    channel: Channel,
    headers: tonic::metadata::MetadataMap,
    pool: DescriptorPool,
    files: BTreeMap<String, FileDescriptorProto>,
    reflection: Option<&'static str>,
}
impl Client {
    async fn connect(request: &ApiRequest, vars: &BTreeMap<String, String>) -> Result<Self> {
        let url = request.grpc_endpoint(vars)?;
        let mut endpoint =
            Endpoint::from_shared(url.clone())?.connect_timeout(Duration::from_secs(30));
        if request.timeout > 0 {
            endpoint = endpoint.timeout(Duration::from_millis(request.timeout));
        }
        if url.starts_with("https://") {
            let mut tls = if request.verify_tls
                && (request.ca_certificate.is_empty() || request.default_ca.unwrap_or(true))
            {
                ClientTlsConfig::new().with_native_roots()
            } else {
                ClientTlsConfig::new()
            };
            if let Some(authority) = request.portable["tls_server_name"]
                .as_str()
                .filter(|s| !s.is_empty())
            {
                let authority = resolve(authority, vars);
                tls = tls.domain_name(authority.clone());
                endpoint = endpoint.origin(format!("https://{authority}").parse()?);
            }
            if request.verify_tls && !request.ca_certificate.is_empty() {
                tls = tls.ca_certificate(tonic::transport::Certificate::from_pem(std::fs::read(
                    crate::engine::file_path(request, &resolve(&request.ca_certificate, vars)),
                )?));
            }
            if let Some(pem) = &request.client_identity {
                tls = tls.identity(tonic::transport::Identity::from_pem(pem, pem));
            } else if !request.client_certificate.is_empty() {
                tls = tls.identity(tonic::transport::Identity::from_pem(
                    std::fs::read(crate::engine::file_path(
                        request,
                        &resolve(&request.client_certificate, vars),
                    ))?,
                    std::fs::read(crate::engine::file_path(
                        request,
                        &resolve(&request.client_key, vars),
                    ))?,
                ));
            }
            endpoint = if request.verify_tls {
                endpoint.tls_config(tls)?
            } else {
                endpoint.tls_config_with_verifier(
                    tls,
                    std::sync::Arc::new(crate::network::Unverified),
                )?
            };
        }
        let channel = if request.proxy.is_empty() {
            endpoint
                .connect()
                .await
                .context("Cannot connect to gRPC server")?
        } else {
            let proxy = resolve(&request.proxy, vars);
            let uri: url::Url = url.parse()?;
            let host = uri.host_str().context("gRPC URL has no host")?.to_owned();
            let port = uri.port_or_known_default().unwrap_or(80);
            let tls = crate::network::proxy_tls_config(request, vars)?;
            endpoint
                .connect_with_connector(tower::service_fn(move |_| {
                    let host = host.clone();
                    let proxy = proxy.clone();
                    let tls = tls.clone();
                    async move {
                        crate::network::tcp_with_tls(&host, port, &proxy, tls)
                            .await
                            .map(hyper_util::rt::TokioIo::new)
                            .map_err(|error| std::io::Error::other(error.to_string()))
                    }
                }))
                .await
                .context("Cannot connect to gRPC proxy")?
        };
        let mut headers = tonic::metadata::MetadataMap::new();
        for p in request
            .headers
            .iter()
            .filter(|p| p.enabled && !p.key.is_empty())
        {
            let key = resolve(&p.key, vars).to_lowercase();
            let value = resolve(&p.value, vars);
            if key.ends_with("-bin") {
                headers.append_bin(
                    key.parse::<tonic::metadata::MetadataKey<tonic::metadata::Binary>>()?,
                    tonic::metadata::MetadataValue::from_bytes(
                        &base64::prelude::BASE64_STANDARD.decode(value)?,
                    ),
                );
            } else {
                headers.append(
                    key.parse::<tonic::metadata::MetadataKey<tonic::metadata::Ascii>>()?,
                    value.parse()?,
                );
            }
        }
        match request.auth.kind.as_str() {
            "basic" => {
                headers.insert(
                    "authorization",
                    format!(
                        "Basic {}",
                        base64::prelude::BASE64_STANDARD.encode(format!(
                            "{}:{}",
                            resolve(&request.auth.username, vars),
                            resolve(&request.auth.password, vars)
                        ))
                    )
                    .parse()?,
                );
            }
            "bearer" => {
                headers.insert(
                    "authorization",
                    format!("Bearer {}", resolve(&request.auth.token, vars)).parse()?,
                );
            }
            "apikey" => {
                headers.insert(
                    resolve(&request.auth.key, vars)
                        .parse::<tonic::metadata::MetadataKey<tonic::metadata::Ascii>>()?,
                    resolve(&request.auth.value, vars).parse()?,
                );
            }
            "none" | "" | "inherit" => {}
            kind => bail!("Authorization {kind} is not available for gRPC"),
        }
        let pool = load_proto_pool(request, vars).await?;
        Ok(Self {
            channel,
            headers,
            pool,
            files: BTreeMap::new(),
            reflection: None,
        })
    }
    async fn reflect_at(
        &self,
        api: &'static str,
        query: MessageRequest,
    ) -> Result<MessageResponse> {
        let message = ServerReflectionRequest {
            host: String::new(),
            message_request: Some(query),
        };
        let mut request = Request::new(tokio_stream::iter([Bytes::from(message.encode_to_vec())]));
        *request.metadata_mut() = self.headers.clone();
        let mut client = Grpc::new(self.channel.clone()).max_decoding_message_size(8 * 1024 * 1024);
        client.ready().await?;
        let mut stream = client
            .streaming(request, api.parse()?, RawCodec)
            .await?
            .into_inner();
        let bytes = stream
            .message()
            .await?
            .context("Reflection returned no response")?;
        let response = ServerReflectionResponse::decode(bytes)?
            .message_response
            .context("Empty reflection response")?;
        match response {
            MessageResponse::ErrorResponse(error) => bail!(
                "Reflection error {}: {}",
                error.error_code,
                error.error_message
            ),
            other => Ok(other),
        }
    }
    async fn reflect(&mut self, query: MessageRequest) -> Result<MessageResponse> {
        const V1: &str = "/grpc.reflection.v1.ServerReflection/ServerReflectionInfo";
        const ALPHA: &str = "/grpc.reflection.v1alpha.ServerReflection/ServerReflectionInfo";
        if let Some(api) = self.reflection {
            return self.reflect_at(api, query).await;
        }
        match self.reflect_at(V1, query.clone()).await {
            Ok(result) => {
                self.reflection = Some(V1);
                Ok(result)
            }
            Err(error)
                if error
                    .downcast_ref::<Status>()
                    .is_some_and(|s| s.code() == tonic::Code::Unimplemented) =>
            {
                let result = self.reflect_at(ALPHA, query).await?;
                self.reflection = Some(ALPHA);
                Ok(result)
            }
            Err(e) => Err(e),
        }
    }
    async fn fetch(&mut self, query: MessageRequest) -> Result<()> {
        let MessageResponse::FileDescriptorResponse(response) = self.reflect(query).await? else {
            bail!("Expected protobuf descriptors")
        };
        for bytes in response.file_descriptor_proto {
            let file = FileDescriptorProto::decode(bytes.as_slice())?;
            self.files.insert(
                file.name.clone().context("Descriptor has no filename")?,
                file,
            );
        }
        ensure!(self.files.len() <= 1024, "Too many protobuf files");
        Ok(())
    }
    async fn ensure_service(&mut self, name: &str) -> Result<()> {
        if self.pool.get_service_by_name(name).is_some() {
            return Ok(());
        }
        self.fetch(MessageRequest::FileContainingSymbol(name.into()))
            .await?;
        for _ in 0..64 {
            let missing = self
                .files
                .values()
                .flat_map(|file| file.dependency.iter())
                .filter(|n| !self.files.contains_key(*n))
                .cloned()
                .collect::<std::collections::BTreeSet<_>>();
            if missing.is_empty() {
                break;
            }
            for name in missing {
                self.fetch(MessageRequest::FileByFilename(name)).await?;
            }
        }
        self.pool.add_file_descriptor_set(FileDescriptorSet {
            file: self.files.values().cloned().collect(),
        })?;
        ensure!(
            self.pool.get_service_by_name(name).is_some(),
            "Service {name} was not found"
        );
        Ok(())
    }
    async fn services(&mut self) -> Result<Vec<String>> {
        if self.pool.services().len() > 0 {
            return Ok(self.pool.services().map(|s| s.full_name().into()).collect());
        }
        let MessageResponse::ListServicesResponse(response) = self
            .reflect(MessageRequest::ListServices(String::new()))
            .await?
        else {
            bail!("Expected gRPC service list")
        };
        let names = response
            .service
            .into_iter()
            .map(|s| s.name)
            .filter(|s| !s.starts_with("grpc.reflection."))
            .collect::<Vec<_>>();
        for name in &names {
            self.ensure_service(name).await?;
        }
        Ok(names)
    }
}
async fn load_proto_pool(
    request: &ApiRequest,
    vars: &BTreeMap<String, String>,
) -> Result<DescriptorPool> {
    if request.proto_files.is_empty() {
        return Ok(DescriptorPool::default());
    }
    let paths = request
        .proto_files
        .iter()
        .map(|path| crate::engine::file_path(request, &resolve(path, vars)))
        .collect::<Vec<_>>();
    let includes = paths
        .iter()
        .filter_map(|p| p.parent().map(std::path::Path::to_path_buf))
        .chain(
            request
                .proto_import_paths
                .iter()
                .map(|p| crate::engine::file_path(request, &resolve(p, vars))),
        )
        .collect::<Vec<_>>();
    let has_descriptors = paths.iter().any(|p| {
        p.extension()
            .is_some_and(|e| e == "protoset" || e == "pb" || e == "desc")
    });
    let set = if has_descriptors {
        let mut r = request.clone();
        r.proto_files = paths
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        r.proto_import_paths = includes
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        tokio::task::spawn_blocking(move || -> Result<_> {
            Ok(FileDescriptorSet::decode(
                crate::interchange::descriptor_bytes(&r)?.as_slice(),
            )?)
        })
        .await??
    } else {
        tokio::task::spawn_blocking(move || protox::compile(paths, includes)).await??
    };
    Ok(DescriptorPool::from_file_descriptor_set(set)?)
}

pub async fn discover(
    request: &ApiRequest,
    vars: &BTreeMap<String, String>,
) -> Result<Vec<MethodInfo>> {
    crate::engine::timed(request.timeout, async {
        let pool = if request.proto_files.is_empty() {
            let mut client = Client::connect(request, vars).await?;
            client.services().await?;
            client.pool
        } else {
            // A local schema can be browsed before connecting to a server.
            load_proto_pool(request, vars).await?
        };
        Ok(pool
            .services()
            .flat_map(|s| s.methods().collect::<Vec<_>>())
            .map(|method| MethodInfo {
                name: format!("{}/{}", method.parent_service().full_name(), method.name()),
                client_streaming: method.is_client_streaming(),
                server_streaming: method.is_server_streaming(),
                example: crate::grpc_message::example(method.input()),
            })
            .collect())
    })
    .await
    .context("gRPC reflection timed out")?
}
pub async fn execute(request: &ApiRequest, vars: &BTreeMap<String, String>) -> Result<Reply> {
    crate::engine::timed(request.timeout, async {
        let mut client = Client::connect(request, vars).await?;
        let full = resolve(&request.grpc_method, vars);
        let full = full.trim_start_matches('/');
        let (service, method) = full
            .rsplit_once('/')
            .context("Choose a gRPC method (package.Service/Method)")?;
        client.ensure_service(service).await?;
        let method = client
            .pool
            .get_service_by_name(service)
            .context("Service not found")?
            .methods()
            .find(|m| m.name() == method)
            .context("Method not found")?;
        let body = resolve(&request.body, vars);
        let value: serde_json::Value =
            crate::json_comments::value(if body.trim().is_empty() { "{}" } else { &body })
                .context("Invalid protobuf JSON message")?;
        let values = if method.is_client_streaming() {
            value
                .as_array()
                .context("Client streaming requires a JSON array of messages")?
                .clone()
        } else {
            vec![value]
        };
        let messages = values
            .into_iter()
            .map(|value| {
                crate::grpc_message::deserialize(method.input(), value)
                    .map(|m| Bytes::from(m.encode_to_vec()))
            })
            .collect::<Result<Vec<_>, _>>()
            .with_context(|| format!("Invalid protobuf JSON message for {full}"))?;
        let mut call = Request::new(tokio_stream::iter(messages));
        *call.metadata_mut() = client.headers.clone();
        if request.timeout > 0 {
            call.set_timeout(Duration::from_millis(request.timeout));
        }
        let response_limit = request.portable["grpc_max_response_mb"]
            .as_u64()
            .map(|n| {
                if n == 0 {
                    usize::MAX
                } else {
                    (n as usize).saturating_mul(1048576)
                }
            })
            .unwrap_or(crate::engine::MAX_RESPONSE_BYTES);
        let mut transport = Grpc::new(client.channel)
            .max_decoding_message_size(response_limit)
            .max_encoding_message_size(crate::engine::MAX_RESPONSE_BYTES);
        transport.ready().await?;
        let path = format!("/{}/{}", method.parent_service().full_name(), method.name()).parse()?;
        let response = transport.streaming(call, path, RawCodec).await?;
        let headers = metadata(response.metadata());
        let mut stream = response.into_inner();
        let mut output = vec![];
        let mut size = 0;
        while let Some(bytes) = stream.message().await? {
            size += bytes.len();
            ensure!(
                size <= response_limit && output.len() < 10000,
                "gRPC response limit exceeded"
            );
            let message = DynamicMessage::decode(method.output(), bytes)?;
            let options = prost_reflect::SerializeOptions::new()
                .skip_default_fields(request.portable["grpc_emit_defaults"] != true);
            output.push(message.serialize_with_options(serde_json::value::Serializer, &options)?);
        }
        let trailers = stream
            .trailers()
            .await?
            .map(|m| metadata(&m))
            .unwrap_or_default();
        let value = if method.is_server_streaming() {
            serde_json::Value::Array(output)
        } else {
            output.pop().context("gRPC server returned no message")?
        };
        let body = serde_json::to_string_pretty(&value)?;
        Ok(Reply {
            status: 0,
            reason: "OK".into(),
            size,
            bytes: body.as_bytes().to_vec(),
            body,
            headers,
            trailers,
            url: request.url.clone(),
            timeline: vec![
                format!("gRPC {full}"),
                format!("Received {size} protobuf bytes"),
            ],
            ..Default::default()
        })
    })
    .await
    .context("gRPC call timed out")?
}
fn metadata(map: &tonic::metadata::MetadataMap) -> Vec<(String, String)> {
    map.iter()
        .map(|entry| match entry {
            tonic::metadata::KeyAndValueRef::Ascii(k, v) => {
                (k.to_string(), v.to_str().unwrap_or("<binary>").into())
            }
            tonic::metadata::KeyAndValueRef::Binary(k, v) => (
                k.to_string(),
                base64::prelude::BASE64_STANDARD.encode(v.to_bytes().unwrap_or_default()),
            ),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonic::codegen::{BoxFuture, Service};
    #[derive(Clone)]
    struct Echo;
    impl tonic::server::NamedService for Echo {
        const NAME: &'static str = "qa.Echo";
    }
    impl tonic::server::UnaryService<Bytes> for Echo {
        type Response = Bytes;
        type Future = BoxFuture<tonic::Response<Bytes>, Status>;
        fn call(&mut self, request: Request<Bytes>) -> Self::Future {
            Box::pin(async move {
                let mut response = tonic::Response::new(request.into_inner());
                response
                    .metadata_mut()
                    .insert("x-echo", "yes".parse().unwrap());
                Ok(response)
            })
        }
    }
    impl tonic::server::StreamingService<Bytes> for Echo {
        type Response = Bytes;
        type ResponseStream = tonic::Streaming<Bytes>;
        type Future = BoxFuture<tonic::Response<Self::ResponseStream>, Status>;
        fn call(&mut self, request: Request<tonic::Streaming<Bytes>>) -> Self::Future {
            Box::pin(async move { Ok(tonic::Response::new(request.into_inner())) })
        }
    }
    impl Service<http::Request<tonic::body::Body>> for Echo {
        type Response = http::Response<tonic::body::Body>;
        type Error = std::convert::Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;
        fn poll_ready(
            &mut self,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }
        fn call(&mut self, request: http::Request<tonic::body::Body>) -> Self::Future {
            Box::pin(async move {
                let mut grpc = tonic::server::Grpc::new(RawCodec);
                Ok(if request.uri().path() == "/qa.Echo/Stream" {
                    grpc.streaming(Echo, request).await
                } else {
                    grpc.unary(Echo, request).await
                })
            })
        }
    }
    const WRAPPER_PROTO: &str = r#"
        syntax = "proto3";
        package qa;
        import "google/protobuf/wrappers.proto";
        message Payload {
            google.protobuf.Int64Value organization_id = 1;
            google.protobuf.StringValue payment_date = 2;
            google.protobuf.StringValue code = 3;
            google.protobuf.BoolValue paid = 4;
            Payload child = 5;
        }
        service Echo {
            rpc Say(Payload) returns (Payload);
            rpc Stream(stream Payload) returns (stream Payload);
        }
    "#;

    #[tokio::test]
    async fn wrapper_objects_round_trip_with_reflection_and_local_proto() {
        let _ = crate::engine::Engine::default();
        let temp = tempfile::tempdir().unwrap();
        let proto = temp.path().join("wrappers.proto");
        std::fs::write(&proto, WRAPPER_PROTO).unwrap();
        let descriptors = protox::compile([&proto], [temp.path()])
            .unwrap()
            .encode_to_vec();
        let reflection = tonic_reflection::server::Builder::configure()
            .register_encoded_file_descriptor_set(&descriptors)
            .build_v1()
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(Echo)
                .add_service(reflection)
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await
                .unwrap();
        });
        let wrapped = serde_json::json!({
            "organization_id": {"value": "9007199254740993"},
            "payment_date": {"value": "2026-10-30"},
            "code": {"value": "1"}, "paid": {"value": false},
            "child": {"organizationId": {"value": 384853}}
        });
        let canonical = serde_json::json!({
            "organizationId": "9007199254740993", "paymentDate": "2026-10-30",
            "code": "1", "paid": false, "child": {"organizationId": "384853"}
        });
        let mut request = ApiRequest {
            protocol: "grpc".into(),
            url: format!("http://{addr}"),
            grpc_method: "qa.Echo/Say".into(),
            timeout: 3000,
            ..Default::default()
        };
        for local in [false, true] {
            if local {
                request.proto_files = vec![proto.to_string_lossy().into_owned()];
            }
            request.grpc_method = "qa.Echo/Say".into();
            for body in [&wrapped, &canonical] {
                request.body = body.to_string();
                let reply = execute(&request, &BTreeMap::new()).await.unwrap();
                assert_eq!(reply.status, 0);
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(&reply.body).unwrap(),
                    canonical
                );
            }
            request.grpc_method = "qa.Echo/Stream".into();
            request.body = serde_json::json!([wrapped, canonical]).to_string();
            let reply = execute(&request, &BTreeMap::new()).await.unwrap();
            assert_eq!(reply.status, 0);
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&reply.body).unwrap(),
                serde_json::json!([canonical, canonical])
            );
        }
        server.abort();
    }

    #[tokio::test]
    async fn generated_examples_include_wrapper_fields_and_terminate_recursive_schemas() {
        let temp = tempfile::tempdir().unwrap();
        let proto = temp.path().join("wrappers.proto");
        std::fs::write(&proto, WRAPPER_PROTO).unwrap();
        let request = ApiRequest {
            proto_files: vec![proto.to_string_lossy().into_owned()],
            ..Default::default()
        };
        let methods = discover(&request, &BTreeMap::new()).await.unwrap();
        let example: serde_json::Value = serde_json::from_str(&methods[0].example).unwrap();
        assert_eq!(example["organization_id"], "0");
        assert_eq!(example["payment_date"], "");
        assert_eq!(example["paid"], false);
        let pool = load_proto_pool(&request, &BTreeMap::new()).await.unwrap();
        DynamicMessage::deserialize(pool.get_message_by_name("qa.Payload").unwrap(), &example)
            .unwrap();
    }

    #[tokio::test]
    async fn reflection_and_dynamic_unary_round_trip() {
        let _ = crate::engine::Engine::default();
        let temp = tempfile::tempdir().unwrap();
        let proto = temp.path().join("echo.proto");
        std::fs::write(&proto,"syntax = \"proto3\"; package qa; message Payload { string text = 1; int64 id = 2; } service Echo { rpc Say(Payload) returns (Payload); }").unwrap();
        let descriptors = protox::compile([&proto], [temp.path()])
            .unwrap()
            .encode_to_vec();
        let reflection = tonic_reflection::server::Builder::configure()
            .register_encoded_file_descriptor_set(&descriptors)
            .build_v1()
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(Echo)
                .add_service(reflection)
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await
                .unwrap();
        });
        let request = ApiRequest {
            protocol: "grpc".into(),
            url: format!("http://{addr}"),
            grpc_method: "qa.Echo/Say".into(),
            body: "{\n// Keep comments in the draft\n\"text\":\"Привет\", /* exact int64 */ \"id\":\"9007199254740993\",\n}".into(),
            ..Default::default()
        };
        let methods = discover(&request, &BTreeMap::new()).await.unwrap();
        assert!(methods.iter().any(|m| m.name == "qa.Echo/Say"));
        let reply = crate::engine::Engine::default()
            .execute(request.clone(), BTreeMap::new())
            .await;
        assert_eq!(reply.error, "");
        assert_eq!(reply.status, 0);
        let json: serde_json::Value = serde_json::from_str(&reply.body).unwrap();
        assert_eq!(json["text"], "Привет");
        assert_eq!(json["id"], "9007199254740993");
        assert!(reply.headers.contains(&("x-echo".into(), "yes".into())));
        let local = ApiRequest {
            proto_files: vec![proto.to_string_lossy().into_owned()],
            ..request
        };
        assert_eq!(execute(&local, &BTreeMap::new()).await.unwrap().status, 0);
        server.abort();
    }

    #[tokio::test]
    async fn tls_reflection_and_invocation_use_the_lock_and_verify_server_certificates() {
        let _ = crate::engine::Engine::default();
        let temp = tempfile::tempdir().unwrap();
        let proto = temp.path().join("echo-tls.proto");
        std::fs::write(&proto, "syntax = \"proto3\"; package qa; message Payload { string text = 1; } service Echo { rpc Say(Payload) returns (Payload); }").unwrap();
        let descriptors = protox::compile([&proto], [temp.path()])
            .unwrap()
            .encode_to_vec();
        let reflection = tonic_reflection::server::Builder::configure()
            .register_encoded_file_descriptor_set(&descriptors)
            .build_v1()
            .unwrap();
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into(), "127.0.0.1".into()])
            .unwrap();
        let ca = temp.path().join("ca.pem");
        std::fs::write(&ca, cert.cert.pem()).unwrap();
        let identity =
            tonic::transport::Identity::from_pem(cert.cert.pem(), cert.signing_key.serialize_pem());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .tls_config(tonic::transport::ServerTlsConfig::new().identity(identity))
                .unwrap()
                .add_service(Echo)
                .add_service(reflection)
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await
                .unwrap();
        });
        let mut request = ApiRequest {
            protocol: "grpc".into(),
            url: address.to_string(),
            grpc_tls: Some(true),
            grpc_method: "qa.Echo/Say".into(),
            body: r#"{"text":"TLS round trip"}"#.into(),
            ca_certificate: ca.to_string_lossy().into_owned(),
            timeout: 3000,
            ..Default::default()
        };
        let methods = discover(&request, &BTreeMap::new()).await.unwrap();
        assert!(methods.iter().any(|m| m.name == "qa.Echo/Say"));
        let reply = execute(&request, &BTreeMap::new()).await.unwrap();
        assert_eq!(reply.status, 0);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&reply.body).unwrap()["text"],
            "TLS round trip"
        );
        request.ca_certificate.clear();
        assert!(
            discover(&request, &BTreeMap::new()).await.is_err(),
            "An untrusted certificate must not be silently accepted"
        );
        request.verify_tls = false;
        assert_eq!(execute(&request, &BTreeMap::new()).await.unwrap().status, 0);
        request.set_grpc_tls(false);
        assert!(
            execute(&request, &BTreeMap::new()).await.is_err(),
            "Disabling the lock must use plaintext, not skip certificate checks"
        );
        server.abort();
    }
}
