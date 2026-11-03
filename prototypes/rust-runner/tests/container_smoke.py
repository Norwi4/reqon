"""Linux runtime container acceptance: real HTTP/gRPC/PostgreSQL and SIGTERM."""
import argparse
import json
import os
import subprocess
import time
import uuid
import xml.etree.ElementTree as ET
import grpc as grpc_lib
from pathlib import Path
from fixture import WorkflowServer
from package_fixture import write_json
from service_fixture import HttpFixture,PostgresStore,certificates
from test_e2e import ROOT


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--image',default='reqon-runner:0.14.1')
    parser.add_argument('--sudo',action='store_true')
    args=parser.parse_args()
    assert os.name!='nt','Run in Linux with a Linux Docker daemon'
    docker=['sudo','-n','docker'] if args.sudo else ['docker']
    def command(*arguments,check=True):
        return subprocess.run([*docker,*map(str,arguments)],capture_output=True,text=True,check=check,timeout=90)
    root=(ROOT/'artifacts/container-smoke').resolve();root.mkdir(parents=True,exist_ok=True)
    out=root/'output';out.mkdir(exist_ok=True);out.chmod(0o777)
    common=['run','--network','host','--read-only','--cap-drop','ALL','--security-opt','no-new-privileges','--memory','256m','--pids-limit','64','--group-add',str(os.getgid()),'-v',f'{root}:/input:ro','-v',f'{out}:/output:rw']
    # Atomic reports are private (0600) and belong to the container UID. Read
    # them using that same UID without weakening their filesystem permissions.
    def read_output(name):
        return command(*common,'--rm','--entrypoint','/bin/cat',args.image,f'/output/{name}').stdout
    def read_json(name):
        return json.loads(read_output(name))
    inspect=json.loads(command('image','inspect',args.image).stdout)[0]
    assert inspect['Config']['User']=='65532:65532'
    assert command(*common,'--rm',args.image,'--version').stdout.strip()=='reqon-runner-prototype 0.14.1'
    store=PostgresStore();http=None;grpc=None;plain=None
    try:
        certs=certificates(root)
        # Grant the container's explicit supplementary group access to its
        # generated client credentials. Server private keys stay host-only.
        for name in ['ca.pem','client_cert.pem','client_key.pem']:
            (root/name).chmod(0o640)
        http=HttpFixture(store=store,tls_dir=root,mtls=True)
        credentials=grpc_lib.ssl_server_credentials([(certs['server_key'],certs['server_cert'])],root_certificates=certs['ca'],require_client_auth=True)
        grpc=WorkflowServer(store=store,credentials=credentials)
        write_json(root/'catalog.json',{'schema_version':'reqon-package/v1','id':'container','name':'Container PostgreSQL','requires':{'engine':'reqon','version':'=0.14.1','capabilities':['http','grpc.unary','poll','javascript']},'cases':['case.json'],'timeout_ms':60000})
        case={'schema_version':'reqon-case/v1','id':'WORKFLOW','name':'Real workflow','timeout_ms':30000,'steps':[
            {'type':'http','id':'create','request':{'method':'POST','url':http.endpoint+'/objects','body':{'type':'json','value':{'name':'container'}}},'expect':[{'target':'status','equals':201}],'save':{'objectId':'/id'}},
            {'type':'grpc','id':'poll','method':'poc.Workflow/Get','request':{'id':'${objectId}'},'poll':{'read_only':True,'interval_ms':20,'timeout_ms':10000,'until':[{'path':'/status','equals':'CREATED'}]}},
            {'type':'javascript','id':'script','timeout_ms':5000,'script':"reqon.test('stored',()=>reqon.assert(response.status==='CREATED'));return {done:true};"}
        ]}
        write_json(root/'case.json',case)
        write_json(root/'env.json',{'schema_version':'reqon-environment/v1','id':'local','endpoint':grpc.endpoint,'transport':{'ca_cert':'ca.pem','client_cert':'client_cert.pem','client_key':'client_key.pem'}})
        def run(label,expected):
            result=command(*common,'--rm',args.image,'run','/input/catalog.json','--env','/input/env.json','--json',f'/output/{label}.json','--junit',f'/output/{label}.xml','--html',f'/output/{label}.html',check=False)
            (root/f'{label}.log').write_text(result.stdout+result.stderr)
            assert result.returncode==expected,(result.returncode,result.stdout,result.stderr)
            report=read_json(f'{label}.json')
            assert read_json(f'{label}.manifest.json')['state']=='complete'
            return report
        assert run('passed',0)['outcome']=='passed'
        assert store.count()==1
        case['steps'][0]['expect'][0]['equals']=418
        write_json(root/'case.json',case)
        assert run('failed',1)['outcome']=='failed'
        assert ET.fromstring(read_output('failed.xml')).find('.//failure') is not None
        write_json(root/'invalid.json',{'invalid':True})
        assert command(*common,'--rm',args.image,'validate','/input/invalid.json','--json',check=False).returncode==2
        plain=HttpFixture()
        write_json(root/'cancel.json',{'schema_version':'reqon-prototype/v1','id':'cancel','endpoint':plain.endpoint,'timeout_ms':60000,'steps':[{'type':'http','id':'request','request':{'url':'/echo'}},{'type':'pause','id':'waiting','duration_ms':30000}]})
        name='reqon-signal-'+uuid.uuid4().hex
        with (root/'cancel.log').open('w') as log:
            process=subprocess.Popen([*docker,*common,'--name',name,args.image,'run','/input/cancel.json','--json','/output/cancel.json','--junit','/output/cancel.xml'],stdout=log,stderr=subprocess.STDOUT)
            try:
                deadline=time.monotonic()+60
                while True:
                    text=(root/'cancel.log').read_text()
                    events=[json.loads(line) for line in text.splitlines() if line.startswith('{') and line.endswith('}')]
                    if any(e.get('type')=='step_started' and e.get('step_id')=='waiting' for e in events):break
                    assert process.poll() is None,text
                    assert time.monotonic()<deadline,'container startup timed out'
                    time.sleep(.1)
                command('kill','--signal','TERM',name)
                assert process.wait(timeout=30)==130
                assert read_json('cancel.json')['outcome']=='cancelled'
                assert read_json('cancel.manifest.json')['state']=='complete'
            finally:
                command('rm','--force',name,check=False)
                if process.poll() is None:process.wait(timeout=10)
        result={'passed':True,'image_id':inspect['Id'],'user':inspect['Config']['User'],'read_only_root':True,'memory_limit_mib':256,'exit_codes_verified':[0,1,2,130],'protocols':['HTTP','gRPC'],'tls':'mTLS with custom CA on HTTP and gRPC','database':'local PostgreSQL','artifacts':['JSON','JUnit','HTML','journal','manifest'],'sigterm':'graceful cancellation with reports'}
        (root/'result.json').write_text(json.dumps(result,indent=2))
        print(json.dumps(result))
    finally:
        if grpc:grpc.close()
        if http:http.close()
        if plain:plain.close()
        store.close()


if __name__=='__main__':main()
