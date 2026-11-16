//! Versioned, stdio-only interface used by the IDE. No desktop profile is opened.
use crate::{engine::Engine, model::*, storage};
use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::PathBuf,
};

const MAX_INPUT: u64 = 16 * 1024 * 1024;

#[derive(Default, Deserialize)]
#[serde(default)]
struct Command {
    op: String,
    request: Request,
    source: String,
    path: PathBuf,
    environment: Vec<Pair>,
    globals: Vec<Pair>,
    runtime: BTreeMap<String, String>,
}

pub fn serve() {
    let result = (|| -> Result<Value> {
        let mut input = Vec::new();
        std::io::stdin()
            .take(MAX_INPUT + 1)
            .read_to_end(&mut input)?;
        ensure!(
            input.len() as u64 <= MAX_INPUT,
            "IDE command exceeds 16 MiB"
        );
        let command: Command = serde_json::from_slice(&input).context("Invalid IDE command")?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        runtime.block_on(dispatch(command))
    })();
    let response = match result {
        Ok(data) => json!({"api":1,"ok":true,"data":data}),
        Err(error) => json!({"api":1,"ok":false,"error":format!("{error:#}")}),
    };
    let mut output = std::io::stdout().lock();
    let _ = serde_json::to_writer(&mut output, &response);
    let _ = output.write_all(b"\n");
    let _ = output.flush();
}

async fn dispatch(mut command: Command) -> Result<Value> {
    match command.op.as_str() {
        "hello" => Ok(
            json!({"engine":env!("CARGO_PKG_VERSION"),"api":1,"protocol":"grpc",
            "features":["reflection-v1","reflection-v1alpha","proto","unary","server-streaming","client-streaming-batch","bidi-streaming-batch","tls","mtls","scripts"]}),
        ),
        "parse" => {
            let mut request = if crate::yaml::is_yaml(&command.path) {
                crate::yaml::parse(&command.source)?
            } else {
                storage::parse_bru(&command.source)?
            };
            request.path = Some(command.path);
            Ok(serde_json::to_value(request)?)
        }
        "serialize" => {
            ensure!(
                command.request.protocol == "grpc",
                "This plugin edits gRPC requests only"
            );
            Ok(json!({"source":storage::source(&command.request)?}))
        }
        "collection" => {
            ensure!(
                crate::native_format::is_collection(&command.path),
                "Select a folder containing reqon.json"
            );
            let (collection, requests, warnings) = storage::read_collection(&command.path)?;
            Ok(json!({"collection":collection,"requests":requests,"warnings":warnings}))
        }
        "environment" => {
            let mut vars = Vec::new();
            for (name, block) in storage::blocks(&command.source) {
                if name == "vars" {
                    vars.extend(crate::bru::pairs(&block));
                }
            }
            for key in storage::secret_names(&command.source) {
                let mut pair = Pair::new(key, "");
                pair.secret = true;
                vars.push(pair);
            }
            Ok(json!({"vars":vars}))
        }
        "serialize_environment" => {
            let public: Vec<_> = command
                .environment
                .iter()
                .filter(|p| !p.secret)
                .cloned()
                .collect();
            let secret: Vec<_> = command
                .environment
                .iter()
                .filter(|p| p.secret && !p.key.is_empty())
                .map(|p| p.key.clone())
                .collect();
            ensure!(
                secret.iter().all(|s| !s.contains(['\n', '\r', ']'])),
                "Invalid secret variable name"
            );
            let mut source = format!("vars {{\n{}\n}}\n", crate::bru::rows_text(&public));
            if !secret.is_empty() {
                source.push_str(&format!("\nvars:secret [\n  {}\n]\n", secret.join("\n  ")));
            }
            Ok(json!({"source":source}))
        }
        "discover" => {
            ensure!(
                command.request.protocol == "grpc",
                "Expected a gRPC request"
            );
            let engine = Engine::default();
            let mut request = crate::inheritance::prepare_scoped(
                command.request,
                &command.globals,
                &command.environment,
                &command.runtime,
            )?;
            let mut vars = variables(
                &command.globals,
                &request.inherited_collection_vars,
                &command.environment,
                &request.vars,
            );
            vars.extend(command.runtime);
            crate::preference_network::prepare(&Preferences::default(), &mut request, &vars)
                .await?;
            let methods = crate::grpc::discover(&request, &vars).await?;
            drop(engine);
            Ok(
                json!({"methods":methods.into_iter().map(|m| json!({"name":m.name,"clientStreaming":m.client_streaming,"serverStreaming":m.server_streaming,"example":m.example})).collect::<Vec<_>>()}),
            )
        }
        "execute" => {
            ensure!(
                command.request.protocol == "grpc",
                "This plugin runs gRPC requests only"
            );
            // The desktop default timeout must not overwrite the editor's explicit value.
            command.request.timeout_inherit = false;
            let mut reply = Engine::default()
                .execute_scoped(
                    command.request,
                    command.globals,
                    command.environment,
                    command.runtime,
                )
                .await;
            reply.bytes.clear();
            Ok(serde_json::to_value(reply)?)
        }
        _ => bail!("Unknown IDE operation"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn ide_round_trip_preserves_grpc_fields_and_unknown_sections() {
        let source = "meta {\n  name: Echo\n  type: grpc\n  seq: 1\n}\ngrpc {\n  url: localhost:50051\n  method: qa.Echo/Say\n}\nbody:json {\n  {\"text\":\"hello\"}\n}\ncustom:vendor {\n  keep: yes\n}\n";
        let parsed = dispatch(Command {
            op: "parse".into(),
            source: source.into(),
            path: "Echo.req".into(),
            ..Default::default()
        })
        .await
        .unwrap();
        let mut request: Request = serde_json::from_value(parsed).unwrap();
        assert_eq!(request.protocol, "grpc");
        request.name = "Updated".into();
        let serialized = dispatch(Command {
            op: "serialize".into(),
            request,
            ..Default::default()
        })
        .await
        .unwrap();
        let output = serialized["source"].as_str().unwrap();
        assert!(output.contains("custom:vendor"));
        let restored = storage::parse_bru(output).unwrap();
        assert_eq!(restored.name, "Updated");
        assert_eq!(restored.grpc_method, "qa.Echo/Say");
    }
    #[tokio::test]
    async fn environment_serialization_never_writes_secret_values() {
        let mut secret = Pair::new("token", "do-not-write-this");
        secret.secret = true;
        let result = dispatch(Command {
            op: "serialize_environment".into(),
            environment: vec![Pair::new("endpoint", "localhost:50051"), secret],
            ..Default::default()
        })
        .await
        .unwrap();
        let source = result["source"].as_str().unwrap();
        assert!(!source.contains("do-not-write-this"));
        let parsed = dispatch(Command {
            op: "environment".into(),
            source: source.into(),
            ..Default::default()
        })
        .await
        .unwrap();
        assert_eq!(parsed["vars"][1]["secret"], true);
    }
    #[tokio::test]
    async fn local_proto_discovery_does_not_require_network() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("echo.proto");
        std::fs::write(&path,"syntax=\"proto3\"; package qa; message Text { string value=1; } service Echo { rpc Say(Text) returns(Text); rpc Many(Text) returns(stream Text); }").unwrap();
        let request = Request {
            protocol: "grpc".into(),
            url: "127.0.0.1:1".into(),
            proto_files: vec![path.to_string_lossy().into()],
            ..Default::default()
        };
        let result = dispatch(Command {
            op: "discover".into(),
            request,
            ..Default::default()
        })
        .await
        .unwrap();
        assert_eq!(result["methods"].as_array().unwrap().len(), 2);
        assert_eq!(result["methods"][1]["serverStreaming"], true);
    }
}
