"""Reproducible local functional benchmark; synthetic inputs only, no cloud login."""
import argparse
import hashlib
import json
import os
import platform
import random
import shutil
import statistics
import subprocess
import threading
import time
from pathlib import Path

import psutil
from package_fixture import write_json
from service_fixture import HttpFixture
from test_e2e import BINARY,ROOT


def measure(command,cwd,log,timeout=180):
    started=time.perf_counter()
    with log.open('wb') as output:
        process=subprocess.Popen(list(map(str,command)),cwd=cwd,stdout=output,stderr=subprocess.STDOUT)
        stop=threading.Event()
        samples=[]
        def sample():
            root=psutil.Process(process.pid)
            while not stop.is_set():
                try:
                    procs=[root,*root.children(recursive=True)]
                    rss=0
                    for proc in procs:
                        try:rss+=proc.memory_info().rss
                        except psutil.Error:pass
                    samples.append(rss)
                except psutil.Error:pass
                stop.wait(.004)
        thread=threading.Thread(target=sample,daemon=True);thread.start()
        try:
            code=process.wait(timeout=timeout)
        finally:
            if process.poll() is None:process.kill();process.wait()
            stop.set();thread.join()
    result={'wall_ms':(time.perf_counter()-started)*1000,'peak_process_tree_rss_mib':max(samples,default=0)/1024**2,'exit_code':code}
    if code:raise RuntimeError(f'Benchmark failed ({code}), see {log}')
    return result


def make_inputs(root,endpoint,count):
    reqon=root/'reqon';reqon.mkdir(exist_ok=True)
    steps=[{'type':'http','id':f'request-{i}','request':{'method':'POST','url':'/echo','body':{'type':'json','value':{'value':7}}},'expect':[{'target':'status','equals':200},{'path':'/body/value','equals':7}]} for i in range(count)]
    write_json(reqon/'catalog.json',{'schema_version':'reqon-package/v1','id':'benchmark','name':'Benchmark','requires':{'engine':'reqon','version':'>=0.14.0,<0.15.0','capabilities':['http','hooks','javascript']},'cases':['case.json'],'timeout_ms':120000})
    write_json(reqon/'case.json',{'schema_version':'reqon-case/v1','id':'HTTP','name':'HTTP','timeout_ms':120000,'steps':steps})
    hooks=root/'reqon-js';hooks.mkdir(exist_ok=True)
    shutil.copy(reqon/'catalog.json',hooks/'catalog.json')
    js="reqon.test('status',()=>reqon.assert(reqon.response.status===200));reqon.test('value',()=>reqon.assert(response.body.value===7));"
    write_json(hooks/'case.json',{'schema_version':'reqon-case/v1','id':'HTTP','name':'HTTP with JS','timeout_ms':120000,'steps':[{**step,'expect':[],'hooks':{'tests':[{'script':js}]}} for step in steps]})
    bruno=root/'bruno';bruno.mkdir(exist_ok=True)
    for generated in bruno.glob('[0-9][0-9][0-9][0-9].bru'):
        generated.unlink()
    write_json(bruno/'bruno.json',{'version':'1','name':'Reqon comparison','type':'collection','ignore':['node_modules','.git']})
    for i in range(count):
        (bruno/f'{i:04d}.bru').write_text(f'''meta {{
  name: request-{i}
  type: http
  seq: {i+1}
}}
post {{
  url: {endpoint}/echo
  body: json
  auth: none
}}
body:json {{
  {{"value":7}}
}}
tests {{
  test("status", function() {{ expect(res.status).to.equal(200); }});
  test("value", function() {{ expect(res.body.body.value).to.equal(7); }});
}}
''',encoding='utf8')
    items=[{'name':f'request-{i}','request':{'method':'POST','header':[{'key':'Content-Type','value':'application/json'}],'url':endpoint+'/echo','body':{'mode':'raw','raw':'{"value":7}'}},'event':[{'listen':'test','script':{'type':'text/javascript','exec':["pm.test('status',()=>pm.response.to.have.status(200));","pm.test('value',()=>pm.expect(pm.response.json().body.value).to.eql(7));"]}}]} for i in range(count)]
    write_json(root/'postman.json',{'info':{'name':'Reqon comparison','schema':'https://schema.getpostman.com/json/collection/v2.1.0/collection.json'},'item':items})


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--requests',type=int,default=200);parser.add_argument('--repeats',type=int,default=5);parser.add_argument('--engines',default='reqon,reqon-js,bruno,bruno-developer,newman,postman');parser.add_argument('--output',type=Path,default=ROOT/'results/cli-benchmark-v0.14.1.json');args=parser.parse_args()
    root=(ROOT/'artifacts/cli-benchmark').resolve();root.mkdir(parents=True,exist_ok=True)
    node=shutil.which('node');modules=(ROOT/'artifacts/competitors/node_modules').resolve()
    server=HttpFixture()
    try:
        make_inputs(root,server.endpoint,args.requests)
        commands={}
        for name in ['reqon','reqon-js']:
            commands[name]=([BINARY.resolve(),'run',root/name,'--endpoint',server.endpoint,'--json',root/f'{name}.json','--junit',root/f'{name}.xml'],root)
        commands['bruno']=([node,modules/'@usebruno/cli/bin/bru.js','run','--noproxy','--reporter-json',root/'bruno.json','--reporter-junit',root/'bruno.xml'],root/'bruno')
        commands['bruno-developer']=([*commands['bruno'][0],'--sandbox','developer'],root/'bruno')
        commands['newman']=([node,modules/'newman/bin/newman.js','run',root/'postman.json','--silent','-r','json,junit','--reporter-json-export',root/'newman.json','--reporter-junit-export',root/'newman.xml'],root)
        commands['postman']=([node,modules/'postman-cli/bin/postman.js','collection','run',root/'postman.json','--silent','--no-report-events'],root)
        engines=args.engines.split(',');runs={name:[] for name in engines}
        versions={name:json.loads((modules/path/'package.json').read_text())['version'] for name,path in [('bruno','@usebruno/cli'),('newman','newman'),('postman','postman-cli')]}
        versions['reqon']=subprocess.check_output([str(BINARY),'--version'],text=True).strip()
        rng=random.Random(20261103)
        for repeat in range(args.repeats+1):
            order=engines.copy();rng.shuffle(order)
            for engine in order:
                command,cwd=commands[engine];before=len(server.calls);server.ports.clear()
                result=measure(command,cwd,root/f'{engine}-{repeat}.log')
                result['requests_received']=len(server.calls)-before;result['tcp_connections']=len(server.ports)
                if result['requests_received']!=args.requests:raise RuntimeError(f'{engine} sent wrong number of requests: {result}')
                if repeat:runs[engine].append(result)
                print(json.dumps({'engine':engine,'warmup':repeat==0,**result}),flush=True)
        summaries={name:{'median_wall_ms':statistics.median(r['wall_ms'] for r in rows),'median_peak_rss_mib':statistics.median(r['peak_process_tree_rss_mib'] for r in rows),'min_ms':min(r['wall_ms'] for r in rows),'max_ms':max(r['wall_ms'] for r in rows)} for name,rows in runs.items()}
        result={'schema_version':'reqon-benchmark/v1','platform':platform.platform(),'processor':platform.processor(),'logical_cpus':os.cpu_count(),'versions':versions,'node':subprocess.check_output([node,'--version'],text=True).strip(),'requests':args.requests,'repeats':args.repeats,'rss_sampling_ms':4,'fixture':'HTTP/1.1 loopback, TCP_NODELAY, POST JSON value=7, status=200 + response JSON body.value=7','reports':{'reqon':'JSON + JUnit + durable journal','reqon-js':'JSON + JUnit + durable journal','bruno':'JSON + JUnit','newman':'JSON + JUnit','postman':'no file reporters: --output requires login in 1.84.0; --silent --no-report-events; this variant performs less report work'},'warmup_per_engine':1,'summary':summaries,'samples':runs}
        result['binary_sha256']=hashlib.sha256(BINARY.read_bytes()).hexdigest()
        result['input_sha256']={str(path.relative_to(root)):hashlib.sha256(path.read_bytes()).hexdigest() for path in [root/'reqon/case.json',root/'reqon-js/case.json',root/'postman.json']}
        result['measurement_environment']=os.environ.get('REQON_BENCH_ENV','Environment isolation not recorded; stop competing workloads before measuring')
        result['reports']['bruno-developer']='JSON + JUnit; explicit developer/Node VM sandbox instead of default safe/QuickJS'
        out=args.output;out.parent.mkdir(exist_ok=True);out.write_text(json.dumps(result,indent=2),encoding='utf8')
        print(json.dumps(summaries,indent=2))
    finally:server.close()


if __name__=='__main__':main()
