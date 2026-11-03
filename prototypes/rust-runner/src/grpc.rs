use anyhow::{Context, Result, bail};
use bytes::{Buf, BufMut, Bytes};
use prost::Message;
use prost_reflect::{DescriptorPool, DynamicMessage, MethodDescriptor};
use prost_types::FileDescriptorProto;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use tonic::{
    Request, Status,
    client::Grpc,
    codec::{Codec, DecodeBuf, Decoder, EncodeBuf, Encoder},
    transport::Channel,
};
use tonic_reflection::pb::v1alpha::{
    ServerReflectionRequest, ServerReflectionResponse, server_reflection_request::MessageRequest,
    server_reflection_response::MessageResponse,
};

// Wire framing is handled by tonic; arbitrary protobuf messages are encoded dynamically.
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

pub struct DynamicClient {
    channel: Channel,
    metadata: tonic::metadata::MetadataMap,
    pub pool: DescriptorPool,
    files: HashMap<String, Vec<u8>>,
    reflection_api: Option<&'static str>,
    pub reflection_requests: usize,
    max_response_bytes: usize,
    emit_defaults: bool,
    local_descriptors: bool,
    cache: crate::transport::Session,
    cache_key: String,
    gui_authority: Option<http::Uri>,
}

pub struct Reply {
    pub body: Value,
    pub headers: crate::http_client::Headers,
    pub trailers: crate::http_client::Headers,
    pub bytes: usize,
}

#[derive(Debug)]
pub struct GuiError {
    pub status: tonic::Status,
    pub headers: tonic::metadata::MetadataMap,
}
impl std::fmt::Display for GuiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.status.fmt(f)
    }
}
impl std::error::Error for GuiError {}

pub fn gui_metadata(map: &tonic::metadata::MetadataMap) -> Vec<(String, String)> {
    metadata(map)
        .into_iter()
        .filter(|(k, _)| {
            !matches!(
                k.as_str(),
                "content-type"
                    | "date"
                    | "grpc-status"
                    | "grpc-message"
                    | "grpc-encoding"
                    | "grpc-accept-encoding"
            )
        })
        .flat_map(|(key, values)| {
            values.into_iter().map(move |value| {
                (
                    key.clone(),
                    if key.ends_with("-bin") {
                        value.strip_prefix("base64:").unwrap_or(&value).to_owned()
                    } else {
                        value
                    },
                )
            })
        })
        .collect()
}

pub fn metadata(map: &tonic::metadata::MetadataMap) -> crate::http_client::Headers {
    use base64::Engine;
    let mut values = crate::http_client::Headers::new();
    for item in map.iter() {
        match item {
            tonic::metadata::KeyAndValueRef::Ascii(key, value) => {
                values
                    .entry(key.to_string())
                    .or_default()
                    .push(value.to_str().unwrap_or("[non-ascii]").into());
            }
            tonic::metadata::KeyAndValueRef::Binary(key, value) => {
                values.entry(key.to_string()).or_default().push(format!(
                    "base64:{}",
                    base64::engine::general_purpose::STANDARD
                        .encode(value.to_bytes().unwrap_or_default())
                ));
            }
        }
    }
    values
}

impl DynamicClient {
    pub async fn connect(
        endpoint: &str,
        metadata: &BTreeMap<String, String>,
        max_response_bytes: usize,
        transport: &crate::transport::Transport,
        cache: &crate::transport::Session,
    ) -> Result<Self> {
        let channel = cache.channel(transport, endpoint).await?;
        let cache_key = format!(
            "{}:{}",
            endpoint,
            crate::package::hash_bytes(&serde_json::to_vec(metadata)?)
        );
        let pool = transport
            .descriptors
            .clone()
            .or_else(|| cache.descriptors.lock().unwrap().get(&cache_key).cloned())
            .unwrap_or_default();
        let mut map = tonic::metadata::MetadataMap::new();
        for (key, value) in metadata {
            map.insert(
                key.parse::<tonic::metadata::MetadataKey<tonic::metadata::Ascii>>()?,
                value.parse()?,
            );
        }
        Ok(Self {
            channel,
            metadata: map,
            pool,
            files: HashMap::new(),
            reflection_api: None,
            reflection_requests: 0,
            max_response_bytes,
            emit_defaults: transport.config.grpc_emit_defaults,
            local_descriptors: transport.descriptors.is_some(),
            cache: cache.clone(),
            cache_key,
            gui_authority: None,
        })
    }

    pub fn connect_gui(
        channel: Channel,
        headers: &[(String, String)],
        max_response_bytes: usize,
        transport: &crate::transport::Transport,
        cache: &crate::transport::Session,
        authority: Option<String>,
    ) -> Result<Self> {
        use base64::Engine;
        let mut metadata = tonic::metadata::MetadataMap::new();
        for (key, value) in headers {
            if key.ends_with("-bin") {
                let bytes = base64::engine::general_purpose::STANDARD.decode(value)?;
                metadata.append_bin(
                    key.parse::<tonic::metadata::MetadataKey<tonic::metadata::Binary>>()?,
                    tonic::metadata::MetadataValue::from_bytes(&bytes),
                );
            } else {
                metadata.append(
                    key.parse::<tonic::metadata::MetadataKey<tonic::metadata::Ascii>>()?,
                    value.parse()?,
                );
            }
        }
        Ok(Self {
            channel,
            metadata,
            pool: transport
                .descriptors
                .clone()
                .or_else(|| cache.descriptors.lock().unwrap().get("gui").cloned())
                .unwrap_or_default(),
            files: HashMap::new(),
            reflection_api: None,
            reflection_requests: 0,
            max_response_bytes,
            emit_defaults: transport.config.grpc_emit_defaults,
            local_descriptors: transport.descriptors.is_some(),
            cache: cache.clone(),
            cache_key: "gui".into(),
            gui_authority: authority.map(|a| a.parse()).transpose()?,
        })
    }

    async fn reflect_at(
        &mut self,
        api: &'static str,
        query: MessageRequest,
    ) -> Result<MessageResponse> {
        self.reflection_requests += 1;
        let request = ServerReflectionRequest {
            host: String::new(),
            message_request: Some(query),
        };
        let mut request = Request::new(tokio_stream::iter([Bytes::from(request.encode_to_vec())]));
        *request.metadata_mut() = self.metadata.clone();
        let mut client = if let Some(origin) = &self.gui_authority {
            Grpc::with_origin(self.channel.clone(), origin.clone())
        } else {
            Grpc::new(self.channel.clone())
        }
        .max_decoding_message_size(8 * 1024 * 1024);
        client.ready().await?;
        let mut stream = client
            .streaming(request, api.parse()?, RawCodec)
            .await?
            .into_inner();
        let bytes = stream
            .message()
            .await?
            .context("reflection returned no response")?;
        let response = ServerReflectionResponse::decode(bytes)?
            .message_response
            .context("empty reflection response")?;
        match response {
            MessageResponse::ErrorResponse(error) => bail!(
                "reflection error {}: {}",
                error.error_code,
                error.error_message
            ),
            value => Ok(value),
        }
    }

    async fn reflect(&mut self, query: MessageRequest) -> Result<MessageResponse> {
        const V1: &str = "/grpc.reflection.v1.ServerReflection/ServerReflectionInfo";
        const ALPHA: &str = "/grpc.reflection.v1alpha.ServerReflection/ServerReflectionInfo";
        if let Some(api) = self.reflection_api {
            return self.reflect_at(api, query).await;
        }
        match self.reflect_at(V1, query.clone()).await {
            Ok(result) => {
                self.reflection_api = Some(V1);
                Ok(result)
            }
            Err(error)
                if error
                    .downcast_ref::<Status>()
                    .is_some_and(|s| s.code() == tonic::Code::Unimplemented) =>
            {
                let result = self.reflect_at(ALPHA, query).await?;
                self.reflection_api = Some(ALPHA);
                Ok(result)
            }
            Err(error) => Err(error),
        }
    }

    async fn fetch_files(&mut self, query: MessageRequest) -> Result<()> {
        let MessageResponse::FileDescriptorResponse(response) = self.reflect(query).await? else {
            bail!("unexpected reflection response");
        };
        for bytes in response.file_descriptor_proto {
            let file = FileDescriptorProto::decode(bytes.as_slice())?;
            let name = file.name.context("descriptor has no name")?;
            self.files.insert(name, bytes);
            if self.files.len() > 512
                || self.files.values().map(Vec::len).sum::<usize>() > 8 * 1024 * 1024
            {
                bail!("reflection descriptor budget exceeded");
            }
        }
        Ok(())
    }

    pub async fn compatibility_method(
        &mut self,
        service: &str,
        method: &str,
    ) -> Result<MethodDescriptor> {
        let mut name = service.to_owned();
        if !service.contains('.') {
            let names = if self.local_descriptors {
                self.pool
                    .services()
                    .map(|s| s.full_name().to_owned())
                    .collect::<Vec<_>>()
            } else {
                let MessageResponse::ListServicesResponse(response) = self
                    .reflect(MessageRequest::ListServices(String::new()))
                    .await?
                else {
                    bail!("expected reflection service list");
                };
                response.service.into_iter().map(|s| s.name).collect()
            };
            let matches: Vec<_> = names
                .into_iter()
                .filter(|s| {
                    s.rsplit('.')
                        .next()
                        .is_some_and(|v| v.eq_ignore_ascii_case(service))
                })
                .collect();
            if matches.len() != 1 {
                bail!("compatibility service must resolve unambiguously");
            }
            name = matches[0].clone();
        }
        match self.method(&format!("{name}/{method}")).await {
            Ok(descriptor) => Ok(descriptor),
            Err(error) => self
                .pool
                .get_service_by_name(&name)
                .and_then(|s| s.methods().find(|m| m.name().eq_ignore_ascii_case(method)))
                .ok_or(error),
        }
    }

    pub async fn method(&mut self, full: &str) -> Result<MethodDescriptor> {
        let desc = self.method_any(full).await?;
        if desc.is_client_streaming() || desc.is_server_streaming() {
            bail!("regression steps support unary methods only; use request mode for streaming");
        }
        Ok(desc)
    }

    pub async fn method_any(&mut self, full: &str) -> Result<MethodDescriptor> {
        let (service, method) = full.split_once('/').context("expected Service/Method")?;
        if self.pool.get_service_by_name(service).is_none() {
            if self.local_descriptors {
                bail!("service not found in local descriptors");
            }
            self.fetch_files(MessageRequest::FileContainingSymbol(service.to_owned()))
                .await?;
            for _ in 0..64 {
                let mut missing = Vec::new();
                for bytes in self.files.values() {
                    for name in FileDescriptorProto::decode(bytes.as_slice())?.dependency {
                        if !self.files.contains_key(&name)
                            && self.pool.get_file_by_name(&name).is_none()
                            && !missing.contains(&name)
                        {
                            missing.push(name);
                        }
                    }
                }
                if missing.is_empty() {
                    break;
                }
                for name in missing {
                    self.fetch_files(MessageRequest::FileByFilename(name))
                        .await?;
                }
            }
            let files = self
                .files
                .values()
                .map(|bytes| FileDescriptorProto::decode(bytes.as_slice()))
                .collect::<Result<Vec<_>, _>>()?;
            self.pool.add_file_descriptor_protos(files)?;
            if self
                .pool
                .file_descriptor_protos()
                .map(prost::Message::encoded_len)
                .sum::<usize>()
                > 8 * 1024 * 1024
            {
                bail!("merged reflection descriptor budget exceeded");
            }
            let mut cache = self.cache.descriptors.lock().unwrap();
            let bytes = cache
                .values()
                .map(|pool| {
                    pool.file_descriptor_protos()
                        .map(prost::Message::encoded_len)
                        .sum::<usize>()
                })
                .sum::<usize>();
            if bytes > 8 * 1024 * 1024 {
                cache.clear();
            }
            if cache.len() < 128 {
                cache.insert(self.cache_key.clone(), self.pool.clone());
            }
        }
        let desc = self
            .pool
            .get_service_by_name(service)
            .context("service not found in reflection")?
            .methods()
            .find(|m| m.name() == method)
            .context("method not found in reflection")?;
        Ok(desc)
    }

    /// GUI preserves proto field names and accepts a JSON array for client streams.
    pub async fn call_gui(
        &mut self,
        method: &MethodDescriptor,
        payload: &Value,
        timeout_ms: u64,
    ) -> Result<Reply> {
        let values = if method.is_client_streaming() {
            payload
                .as_array()
                .context("Client streaming requires a JSON array of messages")?
                .iter()
                .collect::<Vec<_>>()
        } else {
            vec![payload]
        };
        let messages = values
            .into_iter()
            .map(|value| {
                DynamicMessage::deserialize(method.input(), value)
                    .map(|m| Bytes::from(m.encode_to_vec()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut request = Request::new(tokio_stream::iter(messages));
        *request.metadata_mut() = self.metadata.clone();
        if timeout_ms > 0 {
            request.set_timeout(std::time::Duration::from_millis(timeout_ms));
        }
        let mut client = if let Some(origin) = &self.gui_authority {
            Grpc::with_origin(self.channel.clone(), origin.clone())
        } else {
            Grpc::new(self.channel.clone())
        }
        .max_decoding_message_size(self.max_response_bytes)
        .max_encoding_message_size(512 * 1024 * 1024);
        client.ready().await?;
        let path = format!("/{}/{}", method.parent_service().full_name(), method.name()).parse()?;
        let response = client.streaming(request, path, RawCodec).await?;
        let headers = metadata(response.metadata());
        let initial = response.metadata().clone();
        let mut stream = response.into_inner();
        let mut items = Vec::new();
        let mut count = 0usize;
        let mut json_bytes = 0usize;
        let options = prost_reflect::SerializeOptions::new()
            .skip_default_fields(!self.emit_defaults)
            .use_proto_field_name(true);
        while let Some(bytes) = stream.message().await.map_err(|status| GuiError {
            status: if status.code() == tonic::Code::OutOfRange
                && status
                    .message()
                    .contains("decoded message length too large")
            {
                tonic::Status::resource_exhausted(status.message())
            } else {
                status
            },
            headers: initial.clone(),
        })? {
            count = count
                .checked_add(bytes.len())
                .context("response size overflow")?;
            if count > self.max_response_bytes {
                bail!(
                    "Maximum response size exceeded. Adjust the response size limit in Settings."
                );
            }
            let message = DynamicMessage::decode(method.output(), bytes)?;
            let value = message.serialize_with_options(serde_json::value::Serializer, &options)?;
            // Empty protobuf messages must not create an unbounded JSON array.
            json_bytes = json_bytes.saturating_add(serde_json::to_vec(&value)?.len() + 1);
            if json_bytes > self.max_response_bytes {
                bail!(
                    "Maximum response size exceeded. Adjust the response size limit in Settings."
                );
            }
            items.push(value);
            if !method.is_server_streaming() && items.len() > 1 {
                bail!("unary RPC returned multiple messages");
            }
        }
        let trailers = stream
            .trailers()
            .await?
            .as_ref()
            .map(metadata)
            .unwrap_or_default();
        let body = if method.is_server_streaming() {
            Value::Array(items)
        } else {
            items.pop().context("unary RPC returned no message")?
        };
        Ok(Reply {
            body,
            headers,
            trailers,
            bytes: count,
        })
    }

    pub async fn unary(
        &mut self,
        method: &MethodDescriptor,
        payload: &Value,
        timeout_ms: u64,
    ) -> Result<Reply> {
        let input = DynamicMessage::deserialize(method.input(), payload)?;
        let mut request = Request::new(Bytes::from(input.encode_to_vec()));
        *request.metadata_mut() = self.metadata.clone();
        request.set_timeout(std::time::Duration::from_millis(timeout_ms));
        let mut client = Grpc::new(self.channel.clone())
            .max_decoding_message_size(self.max_response_bytes)
            .max_encoding_message_size(8 * 1024 * 1024);
        client.ready().await?;
        let path = format!("/{}/{}", method.parent_service().full_name(), method.name()).parse()?;
        let response = client.server_streaming(request, path, RawCodec).await?;
        let headers = metadata(response.metadata());
        let mut stream = response.into_inner();
        let bytes = stream
            .message()
            .await?
            .context("unary RPC returned no message")?;
        let count = bytes.len();
        if stream.message().await?.is_some() {
            bail!("unary RPC returned multiple messages");
        }
        let trailers = stream
            .trailers()
            .await?
            .as_ref()
            .map(metadata)
            .unwrap_or_default();
        let response = DynamicMessage::decode(method.output(), bytes)?;
        let options =
            prost_reflect::SerializeOptions::new().skip_default_fields(!self.emit_defaults);
        let body = response.serialize_with_options(serde_json::value::Serializer, &options)?;
        Ok(Reply {
            body,
            headers,
            trailers,
            bytes: count,
        })
    }
}
