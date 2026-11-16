use crate::model::*;
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
pub type Routes = BTreeMap<String, Reply>;
pub async fn serve(listener: TcpListener, routes: Routes, fixture: bool) {
    let routes = Arc::new(routes);
    let mut jobs = tokio::task::JoinSet::new();
    loop {
        tokio::select! {incoming=listener.accept()=>{let Ok((stream,_))=incoming else{break};let routes=routes.clone();jobs.spawn(async move{let _=tokio::time::timeout(Duration::from_secs(15),connection(stream,routes,fixture)).await;});},Some(_)=jobs.join_next()=>{}}
    }
}
async fn connection(
    mut stream: tokio::net::TcpStream,
    routes: Arc<Routes>,
    fixture: bool,
) -> std::io::Result<()> {
    let mut request = vec![];
    let mut chunk = [0u8; 4096];
    let mut end = None;
    let mut body_length = 0;
    loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Ok(());
        }
        request.extend_from_slice(&chunk[..n]);
        if request.len() > 1024 * 1024 {
            return Ok(());
        }
        if end.is_none() {
            end = request
                .windows(4)
                .position(|s| s == b"\r\n\r\n")
                .map(|i| i + 4);
            if let Some(e) = end {
                let head = String::from_utf8_lossy(&request[..e]);
                body_length = head
                    .lines()
                    .find_map(|line| {
                        line.split_once(':')
                            .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                            .and_then(|(_, v)| v.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
            }
        }
        if end.is_some_and(|e| request.len() >= e + body_length) {
            break;
        }
    }
    let end = end.unwrap();
    let head = String::from_utf8_lossy(&request[..end]);
    let mut words = head.lines().next().unwrap_or("").split_whitespace();
    let method = words.next().unwrap_or("GET");
    let raw_path = words.next().unwrap_or("/");
    let path = raw_path.split('?').next().unwrap_or(raw_path);
    let key = format!("{method} {path}");
    let mut reply = routes.get(&key).cloned().unwrap_or_else(|| Reply {
        status: 404,
        reason: "Not Found".into(),
        body: "{\"error\":\"No recorded response for this route\"}".into(),
        ..Default::default()
    });
    if fixture {
        if path == "/delay" {
            tokio::time::sleep(Duration::from_secs(8)).await;
        }
        reply.status = if method == "POST" { 201 } else { 200 };
        reply.reason = if method == "POST" { "Created" } else { "OK" }.into();
        reply.body = if path == "/users" {
            serde_json::to_string_pretty(&serde_json::json!([{"id":1,"name":"Reqon Fixture","username":"rust-client","email":"fixture@example.test","address":{"street":"Local test server","suite":"Rust","city":"Desktop","zipcode":"00000"},"website":"example.test","company":{"name":"Reqon","catchPhrase":"Native API workspace"}},{"id":2,"name":"Second Test User","username":"demo","email":"demo@example.test"}])).unwrap()
        } else {
            let headers: BTreeMap<_, _> = head
                .lines()
                .skip(1)
                .filter_map(|line| {
                    line.split_once(':')
                        .map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string()))
                })
                .collect();
            serde_json::to_string_pretty(&serde_json::json!({"method":method,"path":raw_path,"headers":headers,"body":String::from_utf8_lossy(&request[end..]),"fixture":true})).unwrap()
        };
    }
    let body = if reply.bytes.is_empty() {
        reply.body.as_bytes()
    } else {
        &reply.bytes
    };
    let content_type = reply
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.as_str())
        .unwrap_or("application/json");
    let content_type = content_type.replace(['\r', '\n'], "");
    let reason = reply.reason.replace(['\r', '\n'], "");
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nX-Reqon-Mock: true\r\n\r\n",
        reply.status,
        reason,
        content_type,
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.shutdown().await
}
