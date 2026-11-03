"""Baseline 0.4 vs 0.10 and long-run RSS: all child processes included."""
import json
import hashlib
import os
import statistics
from pathlib import Path
from benchmark_cli import measure
from fixture import WorkflowServer
from package_fixture import write_json
from service_fixture import HttpFixture
from test_e2e import ROOT,BINARY


def main():
    root=(ROOT/'artifacts/limits-benchmark').resolve();root.mkdir(parents=True,exist_ok=True)
    baseline=ROOT/'artifacts/baseline/reqon-0.4.0.exe'
    grpc=WorkflowServer();http=HttpFixture()
    samples={}
    try:
        variants={
            'javascript_100':[{'type':'javascript','id':f'js-{i}','timeout_ms':1000,'script':"return {n:(vars.n||0)+1};"} for i in range(100)],
            'grpc_200_1k':[{'type':'grpc','id':f'grpc-{i}','method':'poc.Workflow/Echo','request':{'payload':'x'*1024},'expect':[{'path':'/count','equals':7}]} for i in range(200)],
            'grpc_100_1m':[{'type':'grpc','id':f'grpc-{i}','method':'poc.Workflow/Echo','request':{'payload':'${payload}'},'expect':[{'path':'/count','equals':7}]} for i in range(100)],
        }
        # Legacy vars are generated through a bounded script, keeping input <8 MiB.
        variants['grpc_100_1m'].insert(0,{'type':'javascript','id':'prepare','script':"return {payload:'x'.repeat(900000)};",'timeout_ms':1000})
        for label,steps in variants.items():
            path=root/f'{label}.json';write_json(path,{'schema_version':'reqon-prototype/v1','id':label,'endpoint':grpc.endpoint,'timeout_ms':120000,'steps':steps})
            for version,binary in [('0.4',baseline),('0.10',BINARY)]:
                key=label+'-'+version;samples[key]=[]
                for repeat in range(4):
                    result=measure([binary.resolve(),'run',path,'--json',root/'result.json','--junit',root/'junit.xml'],root,root/f'{key}-{repeat}.log')
                    if repeat:samples[key].append(result)
                    print(json.dumps({'case':key,'warmup':repeat==0,**result}),flush=True)
        package=root/'package';write_json(package/'catalog.json',{'schema_version':'reqon-package/v1','id':'memory','name':'Memory growth','requires':{'engine':'reqon','version':'=0.14.1','capabilities':['http']},'cases':['case.json'],'timeout_ms':120000})
        write_json(package/'case.json',{'schema_version':'reqon-case/v1','id':'LARGE','name':'Large reply','steps':[{'type':'http','id':'large','request':{'url':'/large?size=1048576'},'expect':[{'target':'status','equals':200}]}]})
        for count in [100,1000]:
            key=f'http_{count}_1m';samples[key]=[]
            for repeat in range(3):
                result=measure([BINARY.resolve(),'run',package,'--endpoint',http.endpoint,'--iterations',str(count),'--json',root/'result.json','--junit',root/'junit.xml'],root,root/f'{key}-{repeat}.log')
                samples[key].append(result);print(json.dumps({'case':key,**result}),flush=True)
        summaries={name:{'median_wall_ms':statistics.median(v['wall_ms'] for v in rows),'median_peak_process_tree_rss_mib':statistics.median(v['peak_process_tree_rss_mib'] for v in rows)} for name,rows in samples.items()}
        output={'response_bytes_large_grpc':900000,'response_bytes_large_http':1048576,'measurement':'wall process launch to exit; sampled sum parent+children RSS at 4 ms; first baseline run warmup','samples':samples,'summary':summaries}
        output['binary_sha256']={version:hashlib.sha256(binary.read_bytes()).hexdigest() for version,binary in [('0.4',baseline),('0.10',BINARY)]}
        output['measurement_environment']=os.environ.get('REQON_BENCH_ENV','Environment isolation not recorded; stop competing workloads before measuring')
        (ROOT/'results/limits-benchmark-v0.14.1.json').write_text(json.dumps(output,indent=2),encoding='utf8')
        print(json.dumps(summaries,indent=2))
    finally:grpc.close();http.close()


if __name__=='__main__':main()
