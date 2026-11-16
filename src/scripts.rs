use crate::cancellation::{self, CancellationToken};
use crate::model::{Reply, Request, TestResult};
use anyhow::{Context as _, Result, bail};
use rquickjs::{Context, Promise, Runtime, Value};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Serialize, Deserialize)]
pub struct ScriptOutput {
    pub request: Request,
    pub vars: BTreeMap<String, String>,
    pub tests: Vec<TestResult>,
    pub logs: Vec<String>,
    #[serde(default)]
    pub effects: crate::model::ScriptEffects,
    #[serde(default)]
    pub scope: Option<crate::model::ScriptScope>,
}

#[cfg(test)]
pub fn run(
    source: &str,
    request: &Request,
    reply: &Reply,
    vars: &BTreeMap<String, String>,
) -> Result<ScriptOutput> {
    run_cancellable(source, request, reply, vars, &CancellationToken::new())
}

pub(crate) fn run_cancellable(
    source: &str,
    request: &Request,
    reply: &Reply,
    vars: &BTreeMap<String, String>,
    token: &CancellationToken,
) -> Result<ScriptOutput> {
    cancellation::check(token)?;
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("reqon-script".into())
            .stack_size(8 * 1024 * 1024)
            .spawn_scoped(scope, move || {
                run_inner(source, request, reply, vars, token)
            })?
            .join()
            .map_err(|_| anyhow::anyhow!("Script worker failed"))?
    })
}
fn run_inner(
    source: &str,
    request: &Request,
    reply: &Reply,
    vars: &BTreeMap<String, String>,
    token: &CancellationToken,
) -> Result<ScriptOutput> {
    let runtime = Runtime::new()?;
    runtime.set_memory_limit(128 * 1024 * 1024);
    runtime.set_max_stack_size(4 * 1024 * 1024);
    let deadline = Arc::new(Mutex::new(Instant::now() + Duration::from_secs(2)));
    let cpu_deadline = deadline.clone();
    let wall_deadline = Instant::now() + Duration::from_secs(60);
    let interrupt = token.clone();
    runtime.set_interrupt_handler(Some(Box::new(move || {
        interrupt.is_cancelled()
            || Instant::now() > *cpu_deadline.lock().unwrap()
            || Instant::now() > wall_deadline
    })));
    let context = Context::full(&runtime)?;
    let input = serde_json::json!({"request":request,"reply":{
        "status":reply.status,"reason":reply.reason,"body":reply.body,"headers":reply.headers,
        "elapsed_ms":reply.elapsed_ms,"size":reply.size,"url":reply.url
    },"vars":vars,"scope":request.script_scope,"iterationData":request.iteration_data,
        "iteration":request.run_iteration.saturating_sub(1),"iterationCount":request.run_iteration_count});
    let result = context.with(|ctx| {
        let io_deadline=deadline.clone();
        let http_cancel=token.clone();
        ctx.globals().set("__http",rquickjs::function::Func::from(move |config:String|->String{
            let start=Instant::now();let result=script_http(&config, &http_cancel);*io_deadline.lock().unwrap()+=start.elapsed();
            serde_json::to_string(&match result{Ok(response)=>serde_json::json!({"ok":true,"response":response}),Err(e)=>serde_json::json!({"ok":false,"error":format!("{e:#}")})}).unwrap()
        }))?;
        let sleep_deadline=deadline.clone();
        let sleep_cancel=token.clone();
        ctx.globals().set("__sleep",rquickjs::function::Func::from(move |ms:u32|{
            let start=Instant::now();
            let end=start+Duration::from_millis(ms.min(10000) as u64);
            while !sleep_cancel.is_cancelled() && Instant::now()<end {
                std::thread::sleep(end.saturating_duration_since(Instant::now()).min(Duration::from_millis(10)));
            }
            *sleep_deadline.lock().unwrap()+=start.elapsed();
        }))?;
        ctx.globals().set("__uuid",rquickjs::function::Func::from(||uuid::Uuid::new_v4().to_string()))?;
        ctx.globals().set("__random",rquickjs::function::Func::from(|count:usize|->String{
            let mut bytes=Vec::with_capacity(count.min(65536));while bytes.len()<count.min(65536){for (i,b) in uuid::Uuid::new_v4().as_bytes().iter().enumerate(){if i!=6&&i!=8&&bytes.len()<count.min(65536){bytes.push(*b);}}}serde_json::to_string(&bytes).unwrap()
        }))?;
        ctx.globals().set("__normalizeJson",rquickjs::function::Func::from(|body:String|crate::json_comments::normalize(&body).map(|v|v.into_owned()).unwrap_or(body)))?;
        ctx.globals().set("__input",serde_json::to_string(&input)?)?;
        ctx.globals().set("__processEnv",rquickjs::function::Func::from(|key:String|std::env::var(key).ok()))?;
        let nested_request=request.clone();let nested_deadline=deadline.clone();
        let nested_cancel=token.clone();
        ctx.globals().set("__runRequest",rquickjs::function::Func::from(move |path:String,vars:String|->String{
            let start=Instant::now();let result=run_request(&nested_request,&path,&vars,&nested_cancel);*nested_deadline.lock().unwrap()+=start.elapsed();
            serde_json::to_string(&match result{Ok(response)=>serde_json::json!({"ok":true,"response":response}),Err(e)=>serde_json::json!({"ok":false,"error":format!("{e:#}")})}).unwrap()
        }))?;
        ctx.eval::<(),_>(PRELUDE).context("Cannot initialize the script sandbox")?;
        ctx.eval::<(),_>(include_str!("../assets/runtime/bruno-libraries.js")).context("Cannot initialize assertion libraries")?;
        ctx.eval::<(),_>(include_str!("../assets/runtime/lodash.js")).context("Cannot initialize lodash")?;
        ctx.eval::<(),_>(LIBRARY_BRIDGE)?;
        let source=format!("(async () => {{\n{source}\nawait Promise.all(__pendingTests);\n}})()");
        let evaluated=ctx.eval::<Promise,_>(source).and_then(|promise|promise.finish::<Value>().map(|_|()));
        if let Err(err)=evaluated {
            let caught=ctx.catch();let message=caught.as_exception().map(|e|e.to_string()).or_else(||caught.as_string().and_then(|s|s.to_string().ok())).unwrap_or_else(||err.to_string());
            bail!("Script error: {message}");
        }
        let out:String=ctx.eval("JSON.stringify({ request: __state.request, vars: Object.fromEntries(Object.entries(__state.vars).map(([k,v])=>[k,typeof v==='string'?v:JSON.stringify(v)??''])), tests: __tests, logs: __logs, effects:__effects,scope:__scope })")?;
        let mut out:ScriptOutput=serde_json::from_str(&out)?;out.request.script_scope=out.scope.take().or_else(||request.script_scope.clone());out.request.script_depth=request.script_depth;out.request.inherited_collection_vars=request.inherited_collection_vars.clone();out.request.proto_import_paths=request.proto_import_paths.clone();out.request.proxy_disabled=request.proxy_disabled;Ok(out)
    });
    cancellation::check(token)?;
    let mut out = result?;
    out.request.iteration_data = request.iteration_data.clone();
    out.request.run_iteration = request.run_iteration;
    out.request.run_iteration_count = request.run_iteration_count;
    Ok(out)
}
const PRELUDE: &str = r#"
const __state = JSON.parse(__input);
const __scope=__state.scope||{environment:{...__state.vars},globals:{...__state.vars},collection:{...__state.vars}};
const __effects={environment:{},globals:{},collection:{},deleted_environment:[],deleted_globals:[],persist_environment:false,skip:false,stop:false,next:null};
globalThis.crypto={getRandomValues:array=>{const bytes=JSON.parse(__random(array.byteLength));new Uint8Array(array.buffer,array.byteOffset,array.byteLength).set(bytes);return array;},randomUUID:()=>__uuid()};
for(const row of __state.request.vars){if(row.enabled&&row.kind&&row.kind!=='string'&&row.key in __state.vars){try{__state.vars[row.key]=JSON.parse(__state.vars[row.key]);}catch{}}}
const __tests = [], __logs = [], __pendingTests = [];
const console = {log:(...x)=>__logs.push(x.map(v=>typeof v==='string'?v:JSON.stringify(v)).join(' ')),warn:(...x)=>console.log(...x),error:(...x)=>console.log(...x),info:(...x)=>console.log(...x)};
const bru = {
  getVar:k=>__state.vars[k], getEnvVar:k=>__state.vars[k], getGlobalEnvVar:k=>__state.vars[k], getCollectionVar:k=>__state.vars[k],
  setVar:(k,v)=>__state.vars[k]=v, setEnvVar:(k,v)=>__state.vars[k]=v, setGlobalEnvVar:(k,v)=>__state.vars[k]=v,
  deleteVar:k=>delete __state.vars[k], hasVar:k=>k in __state.vars,
  interpolate:s=>String(s).replace(/\{\{(.*?)\}\}/g,(m,k)=>__state.vars[k.trim()]??m),
  sleep:async ms=>__sleep(ms),
  getRequestVar:k=>__state.vars[k],getFolderVar:k=>__state.vars[k],
  getAllVars:()=>({...__state.vars}),hasEnvVar:k=>k in __state.vars,
  deleteEnvVar:k=>delete __state.vars[k],deleteGlobalEnvVar:k=>delete __state.vars[k],
  cwd:()=>__state.request.path?.replace(/[\\/][^\\/]*$/,'')||'',
  sendRequest:async(config,callback)=>{
    try{const result=JSON.parse(__http(JSON.stringify(config)));if(!result.ok)throw Error(result.error);
      const response=result.response;if(response.status>=400){const error=Error('Request failed with status code '+response.status);error.status=response.status;error.response=response;throw error;}
      if(callback)await callback(null,response);return response;
    }catch(error){if(callback){await callback(error,error.response);return error.response;}throw error;}
  }
};
Object.assign(bru,{
 getEnvVar:k=>__scope.environment[k],getGlobalEnvVar:k=>__scope.globals[k],getCollectionVar:k=>__scope.collection[k],
 setEnvVar:(k,v,options={})=>{if(!k)throw Error('Variable name is required');__scope.environment[k]=v;__state.vars[k]=v;__effects.environment[k]=v;__effects.deleted_environment=__effects.deleted_environment.filter(n=>n!==k);if(options.persist)__effects.persist_environment=true;},
 setGlobalEnvVar:(k,v)=>{__scope.globals[k]=v;__state.vars[k]=v;__effects.globals[k]=v;__effects.deleted_globals=__effects.deleted_globals.filter(n=>n!==k);},
 setCollectionVar:(k,v)=>{__scope.collection[k]=v;__state.vars[k]=v;__effects.collection[k]=v;},
 deleteEnvVar:k=>{delete __scope.environment[k];delete __state.vars[k];delete __effects.environment[k];__effects.deleted_environment.push(k);},
 deleteGlobalEnvVar:k=>{delete __scope.globals[k];delete __state.vars[k];delete __effects.globals[k];__effects.deleted_globals.push(k);},
 hasEnvVar:k=>Object.hasOwn(__scope.environment,k),hasGlobalEnvVar:k=>Object.hasOwn(__scope.globals,k),hasCollectionVar:k=>Object.hasOwn(__scope.collection,k),
 getAllEnvVars:()=>({...__scope.environment}),getAllGlobalEnvVars:()=>({...__scope.globals}),getAllCollectionVars:()=>({...__scope.collection}),
 runRequest:async path=>{const result=JSON.parse(__runRequest(path,JSON.stringify(__state.vars)));if(!result.ok)throw Error(result.error);const response=result.response;Object.assign(__state.vars,response.runtimeVariables||{});return response;},
 runner:{skipRequest:()=>__effects.skip=true,stopExecution:()=>__effects.stop=true,setNextRequest:name=>{if(name===null)__effects.stop=true;else __effects.next=String(name);}},
 setNextRequest:name=>bru.runner.setNextRequest(name),
 getTestResults:async()=>{await Promise.all(__pendingTests);const passed=__tests.filter(t=>t?.passed).length;return{results:__tests,summary:{total:__tests.length,passed,failed:__tests.length-passed}};},
 getAssertionResults:async()=>bru.getTestResults(),
 getProcessEnv:k=>__processEnv(k),
 interpolate:value=>{if(typeof value==='string'){for(let i=0;i<10;i++){const next=value.replace(/\{\{(.*?)\}\}/g,(m,k)=>__state.vars[k.trim()]??m);if(next===value)break;value=next;}return value;}if(Array.isArray(value))return value.map(bru.interpolate);if(value&&typeof value==='object')return Object.fromEntries(Object.entries(value).map(([k,v])=>[k,bru.interpolate(v)]));return value;},
});
const req = {
  getName:()=>__state.request.name,getTags:()=>[...__state.request.tags],getPath:()=>__state.request.path,
  getUrl:()=>__state.request.url, setUrl:v=>__state.request.url=String(v),
  getMethod:()=>__state.request.method, setMethod:v=>__state.request.method=String(v).toUpperCase(),
  getBody:()=>{try{return JSON.parse(__normalizeJson(__state.request.body))}catch{return __state.request.body}},
  setBody:v=>{__state.request.body=typeof v==='string'?v:JSON.stringify(v); if(typeof v!=='string') __state.request.body_kind='Json';},
  getHeader:k=>__state.request.headers.find(p=>p.enabled&&p.key.toLowerCase()===k.toLowerCase())?.value,
  getHeaders:()=>Object.fromEntries(__state.request.headers.filter(p=>p.enabled).map(p=>[p.key,p.value])),
  setHeader:(k,v)=>{let h=__state.request.headers.find(p=>p.key.toLowerCase()===k.toLowerCase());if(h){h.value=String(v);h.enabled=true}else __state.request.headers.push({key:k,value:String(v),enabled:true,secret:false})},
  setHeaders:hs=>Object.entries(hs).forEach(([k,v])=>req.setHeader(k,v)),
  deleteHeader:k=>__state.request.headers=__state.request.headers.filter(p=>p.key.toLowerCase()!==k.toLowerCase())
};
Object.assign(req,{
 getQueryParams:()=>Object.fromEntries(__state.request.params.filter(p=>p.enabled).map(p=>[p.key,p.value])),
 setQueryParam:(k,v)=>{let p=__state.request.params.find(p=>p.key===k);if(p){p.value=String(v);p.enabled=true;}else __state.request.params.push({key:k,value:String(v),enabled:true});},
 deleteQueryParam:k=>{__state.request.params=__state.request.params.filter(p=>p.key!==k);},
 getPathParams:()=>Object.fromEntries(__state.request.path_params.filter(p=>p.enabled).map(p=>[p.key,p.value])),
 setPathParam:(k,v)=>{let p=__state.request.path_params.find(p=>p.key===k);if(p){p.value=String(v);p.enabled=true;}else __state.request.path_params.push({key:k,value:String(v),enabled:true});},
 getAuth:()=>({...__state.request.auth}),setAuth:value=>{if(value.mode)value={...value,kind:value.mode};Object.assign(__state.request.auth,value);},
});
const res=Object.assign(path=>String(path).replace(/\[(\d+)\]/g,'.$1').split('.').reduce((v,k)=>v?.[k],res.body),{
  getStatus:()=>__state.reply.status, getStatusText:()=>__state.reply.reason,
  getBody:()=>{try{return JSON.parse(__state.reply.body)}catch{return __state.reply.body}},
  getHeader:k=>__state.reply.headers.find(([n])=>n.toLowerCase()===k.toLowerCase())?.[1],
  getHeaders:()=>Object.fromEntries(__state.reply.headers),
  getResponseTime:()=>__state.reply.elapsed_ms,
  getSize:()=>__state.reply.size, getUrl:()=>__state.reply.url,
  status:__state.reply.status, responseTime:__state.reply.elapsed_ms
});
res.body=res.getBody(); res.headers=res.getHeaders();
function test(name,fn){const index=__tests.length;__tests.push(null);const pending=(async()=>{try{await fn();__tests[index]={name:String(name),passed:true,error:''};}catch(e){__tests[index]={name:String(name),passed:false,error:String(e.stack||e.message||e)};}})();__pendingTests.push(pending);return pending;}

"#;

const LIBRARY_BRIDGE: &str = r#"
globalThis.require=name=>{if(globalThis.requireObject[name])return globalThis.requireObject[name];throw Error('Cannot find module '+name);};
globalThis.requireObject.uuid={v4:()=>__uuid()};
globalThis.requireObject.lodash=globalThis._;
const __pmScope=(get,set,unset,all)=>({get,set,unset,has:k=>get(k)!==undefined,toObject:all,replaceIn:bru.interpolate});
const __pmHeaders={get:res.getHeader,has:k=>res.getHeader(k)!==undefined,toObject:res.getHeaders,all:()=>__state.reply.headers.map(([key,value])=>({key,value}))};
const __pmResponse={code:res.status,status:res.getStatusText(),responseTime:res.responseTime,responseSize:res.getSize(),json:()=>JSON.parse(__state.reply.body),text:()=>__state.reply.body,headers:__pmHeaders};
__pmResponse.to={have:{status:code=>expect(__pmResponse.code).equal(code),header:(key,value)=>{const actual=res.getHeader(key);expect(actual).not.undefined;if(value!==undefined)expect(actual).equal(value);},jsonBody:()=>expect(__pmResponse.json()).not.undefined},be:{}};
for(const [name,code] of Object.entries({ok:200,created:201,accepted:202,noContent:204,badRequest:400,unauthorized:401,forbidden:403,notFound:404,serverError:500})){Object.defineProperty(__pmResponse.to.be,name,{get:()=>expect(__pmResponse.code).equal(code)});}
Object.defineProperty(__pmResponse.to.be,'success',{get:()=>expect(__pmResponse.code).within(200,299)});
globalThis.pm={
 test,expect,assert:globalThis.assert,
 iterationData:Object.freeze({get:k=>__copyData(__state.iterationData[k]),has:k=>Object.hasOwn(__state.iterationData,k),toObject:()=>__copyData(__state.iterationData),toJSON:()=>__copyData(__state.iterationData)}),
 info:Object.freeze({iteration:__state.iteration,iterationCount:__state.iterationCount,requestName:__state.request.name}),
 variables:__pmScope(bru.getVar,bru.setVar,bru.deleteVar,bru.getAllVars),
 environment:__pmScope(bru.getEnvVar,(k,v)=>bru.setEnvVar(k,v,{persist:true}),bru.deleteEnvVar,bru.getAllEnvVars),
 globals:__pmScope(bru.getGlobalEnvVar,bru.setGlobalEnvVar,bru.deleteGlobalEnvVar,bru.getAllGlobalEnvVars),
 collectionVariables:__pmScope(bru.getCollectionVar,bru.setCollectionVar,k=>{delete __scope.collection[k];delete __state.vars[k];},bru.getAllCollectionVars),
 response:__pmResponse,execution:{setNextRequest:bru.runner.setNextRequest,skipRequest:bru.runner.skipRequest},
 request:{headers:{get:req.getHeader,has:k=>req.getHeader(k)!==undefined,add:row=>req.setHeader(row.key,row.value),upsert:row=>req.setHeader(row.key,row.value),remove:req.deleteHeader,toObject:req.getHeaders},url:{toString:req.getUrl,getQueryString:()=>req.getUrl().split('?')[1]||''},body:{get raw(){return __state.request.body},update:body=>req.setBody(body)},get method(){return req.getMethod()}},
 sendRequest:async(config,callback)=>{try{if(typeof config==='string')config={url:config};config={...config};if(Array.isArray(config.header))config.headers=Object.fromEntries(config.header.filter(h=>!h.disabled).map(h=>[h.key,h.value]));else if(config.header&&typeof config.header==='object')config.headers=config.header;if(config.body?.mode==='raw')config.data=config.body.raw;const response=JSON.parse(__http(JSON.stringify(config)));if(!response.ok)throw Error(response.error);const data=response.response;const result={...data,code:data.status,json:()=>data.data,text:()=>typeof data.data==='string'?data.data:JSON.stringify(data.data),headers:{get:key=>data.headers[key.toLowerCase()]}};if(callback)await callback(null,result);return result;}catch(error){if(callback){await callback(error);return;}throw error;}}
};
function __copyData(value){return value===undefined?undefined:JSON.parse(JSON.stringify(value));}
bru.getIterationData=()=>__copyData(__state.iterationData);
globalThis.postman={setEnvironmentVariable:pm.environment.set,getEnvironmentVariable:pm.environment.get,clearEnvironmentVariable:pm.environment.unset,setGlobalVariable:pm.globals.set,getGlobalVariable:pm.globals.get,clearGlobalVariable:pm.globals.unset,setNextRequest:bru.runner.setNextRequest};
const axios=async config=>bru.sendRequest(typeof config==='string'?{url:config}:config);
for(const method of ['post','put','patch'])axios[method]=(url,data,config)=>bru.sendRequest({...(config||{}),url,method,data});
for(const method of ['get','delete','head','options'])axios[method]=(url,config)=>bru.sendRequest({...(config||{}),url,method});
globalThis.requireObject.axios=axios;
globalThis.fetch=async(url,config={})=>{const result=JSON.parse(__http(JSON.stringify({...config,url,data:config.body})));if(!result.ok)throw Error(result.error);const r=result.response;return{status:r.status,ok:r.status>=200&&r.status<300,headers:r.headers,json:async()=>r.data,text:async()=>typeof r.data==='string'?r.data:JSON.stringify(r.data)};};
globalThis.__literal=value=>{try{return JSON.parse(value)}catch{return value}};
globalThis.__assert=(expr,source)=>{
 const ops=['eq','neq','gt','gte','lt','lte','in','notIn','contains','notContains','length','matches','notMatches','startsWith','endsWith','between','isEmpty','isNotEmpty','isNull','isUndefined','isDefined','isTruthy','isFalsy','isJson','isNumber','isString','isBoolean','isArray','isObject'];
 const split=source.trim().indexOf(' '),first=source.trim().split(' ')[0],op=ops.includes(first)?first:'eq',value=op===first?(split<0?'':source.trim().slice(split+1)):source;
 const a=eval(expr),b=__literal(value),e=expect(a),list=()=>value.replace(/^\[/,'').replace(/\]$/,'').split(',').map(v=>__literal(v.trim()));
 switch(op){
 case 'neq':e.not.equal(b);break;case 'gt':e.above(b);break;case 'gte':e.least(b);break;case 'lt':e.below(b);break;case 'lte':e.most(b);break;
 case 'in':e.oneOf(list());break;case 'notIn':e.not.oneOf(list());break;case 'contains':e.include(b);break;case 'notContains':e.not.include(b);break;case 'length':e.lengthOf(b);break;
 case 'matches':e.match(new RegExp(value.replace(/^\//,'').replace(/\/$/,'')));break;case 'notMatches':e.not.match(new RegExp(value.replace(/^\//,'').replace(/\/$/,'')));break;
 case 'startsWith':expect(String(a).startsWith(String(b))).true;break;case 'endsWith':expect(String(a).endsWith(String(b))).true;break;
 case 'between':{const [low,high]=list();e.within(low,high);break;}
 case 'isEmpty':e.empty;break;case 'isNotEmpty':e.not.empty;break;case 'isNull':e.null;break;case 'isUndefined':e.undefined;break;case 'isDefined':e.not.undefined;break;
 case 'isTruthy':expect(Boolean(a)).true;break;case 'isFalsy':expect(Boolean(a)).false;break;case 'isJson':e.json;break;case 'isNumber':e.a('number');break;case 'isString':e.a('string');break;case 'isBoolean':e.a('boolean');break;case 'isArray':e.an('array');break;case 'isObject':e.an('object');break;
 default:e.equal(b);
 }
};
(function(){
const proto=Object.getPrototypeOf(expect(null));
Object.defineProperty(proto,'json',{get(){let obj=this._obj;this.assert(obj!==null&&typeof obj==='object','expected #{this} to be JSON','expected #{this} not to be JSON');return this;},configurable:true});
proto.jsonSchema=function(schema){const ajv=new Ajv({allErrors:true,strict:false});addFormats(ajv);const check=ajv.compile(schema);this.assert(check(this._obj),'JSON schema validation failed: '+ajv.errorsText(check.errors),'expected JSON schema mismatch');return this;};
})();
"#;

fn script_http(config: &str, token: &CancellationToken) -> Result<serde_json::Value> {
    cancellation::check(token)?;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(cancellation::run(token, script_http_async(config)))
}
async fn script_http_async(config: &str) -> Result<serde_json::Value> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let cfg: serde_json::Value = serde_json::from_str(config)?;
    let url = cfg["url"].as_str().context("sendRequest requires a URL")?;
    let method = reqwest::Method::from_bytes(
        cfg["method"]
            .as_str()
            .unwrap_or("GET")
            .to_uppercase()
            .as_bytes(),
    )?;
    let timeout = cfg["timeout"].as_u64().unwrap_or(10000).clamp(1, 30000);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout))
        .build()?;
    let mut request = client.request(method, url);
    if let Some(headers) = cfg["headers"].as_object() {
        for (k, v) in headers {
            request = request.header(
                k,
                v.as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| v.to_string()),
            );
        }
    }
    if let Some(params) = cfg["params"].as_object() {
        request = request.query(params);
    }
    let data = cfg.get("data").or_else(|| cfg.get("body"));
    if let Some(data) = data {
        request = if let Some(text) = data.as_str() {
            request.body(text.to_owned())
        } else {
            request.json(data)
        };
    }
    if let Some(username) = cfg["auth"]["username"].as_str() {
        request = request.basic_auth(username, cfg["auth"]["password"].as_str());
    }
    let mut response = request.send().await?;
    let status = response.status();
    let headers = response
        .headers()
        .iter()
        .map(|(k, v)| (k.as_str().to_owned(), v.to_str().unwrap_or("").to_owned()))
        .collect::<BTreeMap<_, _>>();
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len().saturating_add(chunk.len()) > 8 * 1024 * 1024 {
            bail!("Script HTTP response exceeds 8 MiB");
        }
        bytes.extend_from_slice(&chunk);
    }
    let text = String::from_utf8_lossy(&bytes);
    let data = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| serde_json::Value::String(text.into_owned()));
    Ok(
        serde_json::json!({"status":status.as_u16(),"statusText":status.canonical_reason().unwrap_or(""),"data":data,"headers":headers}),
    )
}
fn run_request(
    parent: &Request,
    path: &str,
    vars: &str,
    token: &CancellationToken,
) -> Result<serde_json::Value> {
    cancellation::check(token)?;
    anyhow::ensure!(
        parent.script_depth < 8,
        "bru.runRequest nesting exceeds 8 requests"
    );
    let folder = parent
        .path
        .as_ref()
        .and_then(|p| p.parent())
        .context("Save this request in a collection before using bru.runRequest")?;
    let root = folder
        .ancestors()
        .find(|p| crate::native_format::is_collection(p))
        .context("Cannot find collection root")?
        .canonicalize()?;
    let mut path = root.join(path);
    if path.extension().is_none() {
        path.set_extension(if root.join("opencollection.yml").is_file() {
            "yml"
        } else {
            "req"
        });
    }
    let path = crate::native_format::source_path(&path)
        .canonicalize()
        .context("Cannot find requested collection file")?;
    anyhow::ensure!(
        path.starts_with(&root),
        "bru.runRequest must reference a file inside this collection"
    );
    let mut request = crate::yaml::read_request(&path)?;
    request.path = Some(path);
    request.collection = parent.collection.clone();
    request.script_depth = parent.script_depth + 1;
    request.iteration_data = parent.iteration_data.clone();
    request.run_iteration = parent.run_iteration;
    request.run_iteration_count = parent.run_iteration_count;
    request.script_scope = parent.script_scope.clone();
    let mut overrides = BTreeMap::new();
    let supplied: serde_json::Value = serde_json::from_str(vars)?;
    if let Some(values) = supplied.as_object() {
        overrides.extend(values.iter().map(|(k, v)| {
            (
                k.clone(),
                v.as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| v.to_string()),
            )
        }));
    }
    let request = crate::inheritance::prepare_scoped(request, &[], &[], &overrides)?;
    let mut variables =
        crate::model::variables(&[], &request.inherited_collection_vars, &[], &request.vars);
    variables.extend(overrides);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let response = runtime.block_on(
        crate::engine::Engine::default()
            .with_cancellation(token.clone())
            .execute(request, variables),
    );
    cancellation::check(token)?;
    anyhow::ensure!(response.error.is_empty(), "{}", response.error);
    let data = serde_json::from_str::<serde_json::Value>(&response.body)
        .unwrap_or(serde_json::Value::String(response.body));
    Ok(
        serde_json::json!({"status":response.status,"statusText":response.reason,"data":data,"headers":response.headers.into_iter().collect::<BTreeMap<_,_>>(),"runtimeVariables":response.runtime_vars,"responseTime":response.elapsed_ms}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn script_scopes_effects_and_runner_controls_are_preserved() {
        let request = Request {
            script_scope: Some(crate::model::ScriptScope {
                environment: BTreeMap::from([("value".into(), serde_json::json!(12))]),
                globals: BTreeMap::from([("value".into(), serde_json::json!("global"))]),
                collection: BTreeMap::new(),
            }),
            ..Default::default()
        };
        let out=run("expect(bru.getEnvVar('value')).equal(12);expect(bru.getGlobalEnvVar('value')).equal('global');bru.setEnvVar('token',{id:7},{persist:true});bru.setEnvVar('nullable',null);bru.deleteEnvVar('value');bru.setGlobalEnvVar('shared',true);bru.runner.setNextRequest('Finish');bru.runner.skipRequest();",&request,&Reply::default(),&BTreeMap::from([("value".into(),"12".into())])).unwrap();
        assert_eq!(
            out.effects.environment["token"],
            serde_json::json!({"id":7})
        );
        assert!(out.effects.environment["nullable"].is_null());
        assert_eq!(out.effects.deleted_environment, ["value"]);
        assert!(out.effects.persist_environment);
        assert!(out.effects.skip);
        assert_eq!(out.effects.next.as_deref(), Some("Finish"));
        assert!(!out.vars.contains_key("value"));
    }
    #[test]
    fn run_request_executes_another_saved_request_in_the_collection() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0; 2048];
            stream.read(&mut buf).unwrap();
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"token\":7}").unwrap();
        });
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("reqon.json"), "{\"name\":\"Nested\"}").unwrap();
        let child = Request {
            url: format!("http://{address}/token"),
            path: Some(dir.path().join("Login.req")),
            ..Default::default()
        };
        crate::storage::save_request(&child).unwrap();
        let parent = Request {
            path: Some(dir.path().join("Parent.req")),
            ..Default::default()
        };
        let out=run("const login=await bru.runRequest('Login');bru.setVar('token',login.data.token);test('nested request',()=>expect(login.status).equal(200));",&parent,&Reply::default(),&BTreeMap::new()).unwrap();
        assert_eq!(out.vars["token"], "7");
        assert!(out.tests[0].passed);
        server.join().unwrap();
    }
    #[test]
    fn script_can_await_http_and_preserve_callback_results() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut buf = [0; 4096];
            let n = stream.read(&mut buf).unwrap();
            let request = String::from_utf8_lossy(&buf[..n]).into_owned();
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"value\":7}").unwrap();
            request
        });
        let source = format!(
            "const result=await require('axios').get('http://{address}/token',{{headers:{{'X-Script':'yes'}}}});bru.setVar('value',result.data.value);test('async HTTP',()=>expect(result.status).to.equal(200));"
        );
        let out = run(
            &source,
            &Request::default(),
            &Reply::default(),
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(out.vars["value"], "7");
        assert!(out.tests[0].passed);
        assert!(
            server
                .join()
                .unwrap()
                .to_lowercase()
                .contains("x-script: yes")
        );
    }
    #[test]
    fn native_quickjs_runs_async_tests_and_bruno_bundled_libraries() {
        let out=run(r#"
            const crypto=require('crypto-js');
            bru.setVar('digest',crypto.SHA256('abc').toString());
            await Promise.resolve();
            test('async schema',async()=>{await Promise.resolve();expect({id:7}).to.have.jsonSchema({type:'object',properties:{id:{type:'integer'}},required:['id']});});
            test('deep members',()=>expect([{id:1},{id:2}]).to.have.deep.members([{id:2},{id:1}]));
            test('buffer',()=>expect(Buffer.from('hello').toString('base64')).to.equal('aGVsbG8='));
        "#,&Request::default(),&Reply::default(),&BTreeMap::new()).unwrap();
        assert_eq!(
            out.vars["digest"],
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(out.tests.len(), 3);
        assert!(out.tests.iter().all(|t| t.passed), "{:?}", out.tests);
        assert_eq!(out.tests[0].name, "async schema");
    }
    #[test]
    fn script_can_change_request_extract_vars_and_report_failed_tests() {
        let out=run("req.setHeader('X-Test','yes'); bru.setVar('token',res.getBody().token); test('ok',()=>expect(res.status).to.equal(200)); test('bad',()=>expect(2).to.equal(3)); console.log('done')",&Request::default(),&Reply{status:200,body:r#"{"token":"abc"}"#.into(),..Default::default()},&BTreeMap::new()).unwrap();
        assert_eq!(out.vars["token"], "abc");
        assert_eq!(out.request.headers[0].value, "yes");
        assert!(out.tests[0].passed);
        assert!(!out.tests[1].passed);
        assert_eq!(out.logs, ["done"]);
    }
    #[test]
    fn infinite_script_is_interrupted() {
        assert!(
            run(
                "while(true) {}",
                &Request::default(),
                &Reply::default(),
                &BTreeMap::new()
            )
            .is_err()
        );
    }
}
