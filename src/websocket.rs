use crate::model::{Reply, Request, resolve};
use anyhow::{Context, Result, bail};
use base64::Engine as _;
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use std::collections::BTreeMap;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message, client::IntoClientRequest},
};
pub type Socket = WebSocketStream<MaybeTlsStream<crate::network::Stream>>;
pub enum Command {
    Text(String),
    Binary(Vec<u8>),
    Ping,
    Close,
}
pub enum SocketEvent {
    Connected(Vec<(String, String)>),
    Message(Record),
    Closed(String),
}
#[derive(Clone)]
pub struct Record {
    pub outgoing: bool,
    pub kind: String,
    pub body: String,
    pub at: u64,
    pub lines: Vec<usize>,
}
impl Record {
    pub fn new(outgoing: bool, kind: &str, body: String) -> Self {
        let lines = crate::text::line_offsets(&body);
        Self {
            outgoing,
            kind: kind.into(),
            body,
            at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            lines,
        }
    }
}
pub async fn run(
    request: Request,
    vars: BTreeMap<String, String>,
    mut commands: tokio::sync::mpsc::UnboundedReceiver<Command>,
    emit: impl Fn(SocketEvent),
) -> Result<()> {
    let (mut socket, headers) = connect(&request, &vars).await?;
    emit(SocketEvent::Connected(headers));
    loop {
        tokio::select! {
            command=commands.recv()=>match command {
                Some(Command::Text(body))=>{socket.send(Message::Text(body.clone().into())).await?;emit(SocketEvent::Message(Record::new(true,"Text",body)));},
                Some(Command::Binary(bytes))=>{let body=base64::prelude::BASE64_STANDARD.encode(&bytes);socket.send(Message::Binary(bytes.into())).await?;emit(SocketEvent::Message(Record::new(true,"Binary",body)));},
                Some(Command::Ping)=>{socket.send(Message::Ping(Bytes::new())).await?;emit(SocketEvent::Message(Record::new(true,"Ping",String::new())));},
                Some(Command::Close)|None=>{let _=socket.close(None).await;break;},
            },
            incoming=socket.next()=>match incoming {
                Some(Ok(Message::Text(text)))=>emit(SocketEvent::Message(Record::new(false,"Text",text.to_string()))),
                Some(Ok(Message::Binary(bytes)))=>emit(SocketEvent::Message(Record::new(false,"Binary",base64::prelude::BASE64_STANDARD.encode(bytes)))),
                Some(Ok(Message::Ping(bytes)))=>{emit(SocketEvent::Message(Record::new(false,"Ping",base64::prelude::BASE64_STANDARD.encode(bytes))));socket.flush().await?;},
                Some(Ok(Message::Pong(bytes)))=>emit(SocketEvent::Message(Record::new(false,"Pong",base64::prelude::BASE64_STANDARD.encode(bytes)))),
                Some(Ok(Message::Close(close)))=>{let text=close.map(|c|format!("{} {}",u16::from(c.code),c.reason)).unwrap_or("Connection closed".into());emit(SocketEvent::Closed(text));return Ok(());},
                Some(Err(error))=>return Err(error.into()),
                None=>break,
                _=>{},
            }
        }
    }
    emit(SocketEvent::Closed("Connection closed".into()));
    Ok(())
}

pub async fn connect(
    request: &Request,
    vars: &BTreeMap<String, String>,
) -> Result<(Socket, Vec<(String, String)>)> {
    let mut url =
        url::Url::parse(&resolve(&request.url, vars)).context("Enter a ws:// or wss:// URL")?;
    if !["ws", "wss"].contains(&url.scheme()) {
        bail!("WebSocket URL must use ws:// or wss://");
    }
    if !request.params.is_empty() {
        let keys = request
            .params
            .iter()
            .filter(|p| !p.key.is_empty())
            .map(|p| resolve(&p.key, vars))
            .collect::<Vec<_>>();
        let keep = url
            .query_pairs()
            .filter(|(k, _)| !keys.iter().any(|n| n == k))
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect::<Vec<_>>();
        url.set_query(None);
        let mut query = url.query_pairs_mut();
        query.extend_pairs(keep);
        for p in request
            .params
            .iter()
            .filter(|p| p.enabled && !p.key.is_empty())
        {
            query.append_pair(&resolve(&p.key, vars), &resolve(&p.value, vars));
        }
    }
    let mut upgrade = url.as_str().into_client_request()?;
    for p in request
        .headers
        .iter()
        .filter(|p| p.enabled && !p.key.is_empty())
    {
        upgrade.headers_mut().append(
            resolve(&p.key, vars).parse::<http::header::HeaderName>()?,
            resolve(&p.value, vars).parse()?,
        );
    }
    match request.auth.kind.as_str() {
        "basic" => {
            upgrade.headers_mut().insert(
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
            upgrade.headers_mut().insert(
                "authorization",
                format!("Bearer {}", resolve(&request.auth.token, vars)).parse()?,
            );
        }
        "apikey" => {
            upgrade.headers_mut().insert(
                resolve(&request.auth.key, vars).parse::<http::header::HeaderName>()?,
                resolve(&request.auth.value, vars).parse()?,
            );
        }
        "" | "none" | "inherit" => {}
        kind => bail!("Authorization {kind} is not available for WebSocket"),
    }
    let config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(crate::engine::MAX_RESPONSE_BYTES))
        .max_frame_size(Some(crate::engine::MAX_RESPONSE_BYTES));
    let connector = if url.scheme() == "wss" {
        Some(tokio_tungstenite::Connector::Rustls(
            crate::network::tls_config(request, vars)?,
        ))
    } else {
        None
    };
    let (socket, response) = crate::engine::timed(request.timeout, async {
        let stream = crate::network::tcp_with_tls(
            url.host_str().context("WebSocket URL has no host")?,
            url.port_or_known_default().unwrap_or(80),
            &resolve(&request.proxy, vars),
            crate::network::proxy_tls_config(request, vars)?,
        )
        .await?;
        Ok::<_, anyhow::Error>(
            tokio_tungstenite::client_async_tls_with_config(
                upgrade,
                stream,
                Some(config),
                connector,
            )
            .await?,
        )
    })
    .await
    .context("WebSocket connection timed out")??;
    let headers = response
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").into()))
        .collect();
    Ok((socket, headers))
}
pub async fn exchange(request: &Request, vars: &BTreeMap<String, String>) -> Result<Reply> {
    crate::engine::timed(request.timeout, async {
        let (mut socket, headers) = connect(request, vars).await?;
        if !request.body.is_empty() {
            socket
                .send(Message::Text(resolve(&request.body, vars).into()))
                .await?;
        }
        while let Some(message) = socket.next().await {
            let message = message?;
            let bytes = match message {
                Message::Text(s) => s.as_bytes().to_vec(),
                Message::Binary(b) => b.to_vec(),
                Message::Close(_) => bail!("WebSocket closed before receiving a message"),
                _ => continue,
            };
            let _ = socket.close(None).await;
            return Ok(Reply {
                status: 101,
                reason: "Switching Protocols".into(),
                size: bytes.len(),
                body: String::from_utf8_lossy(&bytes).into_owned(),
                bytes,
                headers,
                url: request.url.clone(),
                ..Default::default()
            });
        }
        bail!("WebSocket stream ended")
    })
    .await
    .context("WebSocket receive timed out")?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn websocket_echo_uses_real_upgrade_and_frames() {
        let _ = crate::engine::Engine::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let message = socket.next().await.unwrap().unwrap();
            socket.send(message).await.unwrap();
        });
        let reply = exchange(
            &Request {
                protocol: "ws".into(),
                url: format!("ws://{addr}/echo"),
                body: "Привет, Rust!".into(),
                ..Default::default()
            },
            &BTreeMap::new(),
        )
        .await
        .unwrap();
        assert_eq!(reply.status, 101);
        assert_eq!(reply.body, "Привет, Rust!");
        server.await.unwrap();
    }
}
