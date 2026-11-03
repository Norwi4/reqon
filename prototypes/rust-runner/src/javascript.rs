//! A reusable process per case, a fresh bounded QuickJS VM per script.
use anyhow::{Context as _, Result, bail};
use rquickjs::{Context, Function, Runtime};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{BufRead, Write},
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

const INPUT_LIMIT: usize = 8 * 1024 * 1024;
const OUTPUT_LIMIT: usize = 1024 * 1024;
static INPUT: std::sync::OnceLock<std::sync::Mutex<std::sync::mpsc::Receiver<String>>> =
    std::sync::OnceLock::new();

fn next_input() -> Result<String> {
    INPUT
        .get()
        .context("worker input not initialized")?
        .lock()
        .unwrap()
        .recv()
        .context("parent closed script input")
}

#[derive(Serialize, Deserialize)]
pub struct ScriptInput {
    pub script: String,
    pub vars: BTreeMap<String, Value>,
    pub response: Value,
    pub timeout_ms: u64,
    #[serde(default)]
    pub hook: bool,
    #[serde(default)]
    pub response_info: Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ScriptCheck {
    pub name: String,
    pub passed: bool,
    pub message: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct ScriptOutput {
    pub values: Option<BTreeMap<String, Value>>,
    pub error: Option<String>,
    pub kind: Option<String>,
    #[serde(default)]
    pub checks: Vec<ScriptCheck>,
    #[serde(default)]
    pub logs: Vec<String>,
}

fn read_line(limit: usize) -> Result<Option<String>> {
    let mut bytes = Vec::new();
    std::io::Read::take(std::io::stdin().lock(), limit as u64 + 1).read_until(b'\n', &mut bytes)?;
    if bytes.is_empty() {
        return Ok(None);
    }
    if bytes.len() > limit {
        bail!("worker input exceeds limit");
    }
    Ok(Some(String::from_utf8(bytes)?))
}

fn bridge(request: String) -> rquickjs::Result<String> {
    let run = || -> Result<String> {
        if request.len() > 256 * 1024 {
            bail!("script request exceeds 256 KiB");
        }
        let value: Value = serde_json::from_str(&request)?;
        let mut stdout = std::io::stdout().lock();
        writeln!(stdout, "{}", json!({"type":"send","request":value}))?;
        stdout.flush()?;
        let reply = next_input()?;
        if reply.len() > OUTPUT_LIMIT {
            bail!("script bridge response exceeds 1 MiB");
        }
        Ok(reply)
    };
    Ok(run().unwrap_or_else(|_| json!({"ok":false,"error":"script bridge failed"}).to_string()))
}

pub fn evaluate(input: ScriptInput) -> ScriptOutput {
    let run = || -> Result<ScriptOutput> {
        let runtime = Runtime::new()?;
        runtime.set_memory_limit(16 * 1024 * 1024);
        runtime.set_max_stack_size(256 * 1024);
        let deadline = Instant::now() + Duration::from_millis(input.timeout_ms);
        runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));
        let context = Context::full(&runtime)?;
        context.with(|ctx| -> Result<ScriptOutput> {
            ctx.globals().set("__reqonSend", Function::new(ctx.clone(), bridge)?)?;
            let data = serde_json::to_string(&serde_json::to_string(&input)?)?;
            let prelude = r#"JSON.stringify((function(input){'use strict';
                const vars=input.vars, response=input.response, updates=Object.create(null), checks=[], logs=[];
                const assert=(condition,message='assertion failed')=>{if(!condition)throw new Error('ASSERTION: '+String(message).slice(0,4096));};
                const reqon=Object.freeze({
                    vars:Object.freeze({get:(name)=>vars[name],set:(name,value)=>{if(typeof name!=='string'||!/^[A-Za-z_][A-Za-z0-9_]{0,127}$/.test(name)||['__proto__','prototype','constructor'].includes(name))throw new Error('invalid variable name'); vars[name]=value; updates[name]=value;}}),
                    response:input.response_info, assert,
                    test:(name,fn)=>{if(checks.length>=256)throw new Error('script assertion limit'); try{fn();checks.push({name:String(name).slice(0,256),passed:true,message:null});}catch(e){checks.push({name:String(name).slice(0,256),passed:false,message:String(e.message||e).slice(0,4096)});}},
                    log:(...args)=>{if(logs.length>=64)throw new Error('script log limit');logs.push(args.map(v=>typeof v==='string'?v:JSON.stringify(v)).join(' ').slice(0,1024));},
                    send:(request)=>{const result=JSON.parse(__reqonSend(JSON.stringify(request)));if(!result.ok)throw new Error(result.error);return result.response;}
                });
                const returned=(function(){"#;
            let tail = r#"
                })();
                if(!(input.hook&&returned===undefined)&&(returned===null||typeof returned!=='object'||Array.isArray(returned)))throw new Error('script must return a JSON object of variables');
                Object.assign(updates,returned||{});
                const failed=checks.some(c=>!c.passed);
                return {values:updates,checks,logs,error:failed?'script assertions failed':null,kind:failed?'assertion':null};
            })(JSON.parse("#;
            let source = format!("{prelude}{}\n{tail}{data})))", input.script);
            let encoded: String = match ctx.eval(source) {
                Ok(v) => v,
                Err(error) => {
                    let thrown=ctx.catch();
                    let detail=thrown.as_exception().and_then(|e|e.message()).unwrap_or_else(||error.to_string());
                    if Instant::now()>=deadline { bail!("script timeout"); }
                    bail!("{detail}");
                }
            };
            if encoded.len()>OUTPUT_LIMIT { bail!("script output exceeds 1 MiB"); }
            Ok(serde_json::from_str(&encoded)?)
        })
    };
    run().unwrap_or_else(|e| {
        let message = format!("{e:#}");
        let kind = if message.contains("ASSERTION:") {
            "assertion"
        } else if message.contains("script timeout") {
            "timeout"
        } else {
            "script_error"
        };
        ScriptOutput {
            values: None,
            error: Some(message),
            kind: Some(kind.into()),
            checks: Vec::new(),
            logs: Vec::new(),
        }
    })
}

pub fn worker_main() -> Result<()> {
    // Watch pipe EOF independently of the VM. A killed parent closes its write
    // handle, so even a CPU-bound JS worker exits immediately instead of living
    // until the script deadline. A single bounded channel also handles send replies.
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    INPUT
        .set(std::sync::Mutex::new(receiver))
        .map_err(|_| anyhow::anyhow!("worker input already initialized"))?;
    std::thread::spawn(move || {
        loop {
            match read_line(INPUT_LIMIT) {
                Ok(Some(line)) => {
                    if sender.send(line).is_err() {
                        std::process::exit(2)
                    }
                }
                _ => std::process::exit(2),
            }
        }
    });
    loop {
        let line = next_input()?;
        let input: ScriptInput = serde_json::from_str(&line)?;
        if input.timeout_ms == 0 || input.timeout_ms > 60_000 || input.script.len() > 256 * 1024 {
            bail!("invalid worker script budget");
        }
        let output = evaluate(input);
        let mut stdout = std::io::stdout().lock();
        serde_json::to_writer(&mut stdout, &json!({"type":"result","result":output}))?;
        writeln!(stdout)?;
        stdout.flush()?;
    }
}

pub struct Worker {
    child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    stdout: BufReader<tokio::process::ChildStdout>,
}

impl Worker {
    pub fn start(executable: &Path, on_started: impl FnOnce(u32)) -> Result<Self> {
        let mut command = tokio::process::Command::new(executable);
        command
            .arg("js-worker")
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        if let Some(root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", root);
        }
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let mut child = command.spawn().context("start JS worker")?;
        on_started(child.id().context("JS worker PID unavailable")?);
        Ok(Self {
            stdin: child.stdin.take().unwrap(),
            stdout: BufReader::new(child.stdout.take().unwrap()),
            child,
        })
    }

    pub async fn run<F, Fut>(&mut self, input: &ScriptInput, mut send: F) -> Result<ScriptOutput>
    where
        F: FnMut(crate::http_client::RequestSpec) -> Fut,
        Fut: std::future::Future<Output = Result<Value>>,
    {
        let bytes = serde_json::to_vec(input)?;
        if bytes.len() >= INPUT_LIMIT {
            bail!("script input exceeds 8 MiB");
        }
        let work = async {
            self.stdin.write_all(&bytes).await?;
            self.stdin.write_all(b"\n").await?;
            self.stdin.flush().await?;
            let mut calls = 0;
            loop {
                let mut line = Vec::new();
                (&mut self.stdout)
                    .take(OUTPUT_LIMIT as u64 + 1)
                    .read_until(b'\n', &mut line)
                    .await?;
                if line.len() > OUTPUT_LIMIT {
                    bail!("JS worker output exceeds 1 MiB");
                }
                if line.is_empty() {
                    bail!("JS worker exited before producing a result");
                }
                let message: Value =
                    serde_json::from_slice(&line).context("invalid JS worker message")?;
                match message["type"].as_str() {
                    Some("result") => {
                        return Ok(serde_json::from_value(message["result"].clone())?);
                    }
                    Some("send") => {
                        calls += 1;
                        // Hard protocol ceiling even when a custom adapter is used.
                        if calls > 16 {
                            bail!("script additional-request limit exceeded");
                        }
                        let reply = match serde_json::from_value(message["request"].clone()) {
                            Ok(request) => send(request).await,
                            Err(_) => Err(anyhow::anyhow!("invalid script HTTP request")),
                        };
                        let reply = match reply {
                            Ok(value) => json!({"ok":true,"response":value}),
                            Err(e) => json!({"ok":false,"error":e.to_string()}),
                        };
                        let mut bytes = serde_json::to_vec(&reply)?;
                        if bytes.len() > OUTPUT_LIMIT {
                            bytes =
                                br#"{"ok":false,"error":"script response exceeds 1 MiB"}"#.to_vec();
                        }
                        bytes.push(b'\n');
                        self.stdin.write_all(&bytes).await?;
                        self.stdin.flush().await?;
                    }
                    _ => bail!("unexpected JS worker message"),
                }
            }
        };
        let result = tokio::time::timeout(
            Duration::from_millis(input.timeout_ms.saturating_add(1500)),
            work,
        )
        .await
        .context("JS worker wall timeout")?;
        if result.is_err() {
            let _ = self.child.start_kill();
        }
        result
    }
}
