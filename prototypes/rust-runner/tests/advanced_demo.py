"""One command: PostgreSQL + mTLS HTTP/gRPC + authenticated proxy + CI reports."""
import json
import subprocess
import grpc
from fixture import WorkflowServer
from package_fixture import write_json
from service_fixture import PostgresStore,HttpFixture,ProxyFixture,certificates
from test_e2e import BINARY,ROOT


def main():
    root=ROOT/'artifacts/advanced-demo';root.mkdir(parents=True,exist_ok=True)
    certs=certificates(root)
    store=PostgresStore();http=None;server=None;proxy=None
    try:
        http=HttpFixture(store=store,tls_dir=root,mtls=True)
        credentials=grpc.ssl_server_credentials([(certs['server_key'],certs['server_cert'])],root_certificates=certs['ca'],require_client_auth=True)
        server=WorkflowServer(store=store,credentials=credentials)
        proxy=ProxyFixture(authorization='Bearer generated-local-fixture')
        package=root/'package'
        write_json(package/'catalog.json',{'schema_version':'reqon-package/v1','id':'local-postgres-regression','name':'Local PostgreSQL regression','requires':{'engine':'reqon','version':'=0.14.1','capabilities':['http','grpc.unary','poll','pause','javascript','hooks','variables']},'cases':['workflow.json'],'timeout_ms':30000})
        write_json(package/'workflow.json',{'schema_version':'reqon-case/v1','id':'CREATE-AND-VERIFY','name':'Create via HTTP, poll gRPC, verify stored state','tags':['smoke'],'steps':[
            {'type':'http','id':'create','request':{'method':'POST','url':http.endpoint+'/objects','body':{'type':'json','value':{'name':'${name}'}}},'hooks':{'pre':[{'script':"reqon.vars.set('name',vars.data.name);"}]},'expect':[{'target':'status','equals':201}],'save':{'objectId':'/id'}},
            {'type':'grpc','id':'wait','method':'poc.Workflow/Get','request':{'id':'${objectId}'},'poll':{'read_only':True,'interval_ms':10,'timeout_ms':2000,'until':[{'path':'/status','equals':'CREATED'}]},'expect':[{'target':'headers','path':'/x-fixture','equals':['grpc']},{'target':'trailers','path':'/x-finished','equals':['yes']}]},
            {'type':'pause','id':'pause','duration_ms':5},
            {'type':'http','id':'verify','request':{'url':http.endpoint+'/objects/${objectId}'},'expect':[{'path':'/status','equals':'CREATED'},{'path':'/count','greater_than':2},{'path':'','schema':{'type':'object','required':['id','status','count']}}],'hooks':{'tests':[{'script':"reqon.test('stored id',()=>reqon.assert(response.id===vars.objectId));"}]}},
        ]})
        write_json(root/'environment.json',{'schema_version':'reqon-environment/v1','id':'local-postgres','endpoint':server.endpoint,'transport':{'ca_cert':'ca.pem','client_cert':'client_cert.pem','client_key':'client_key.pem','proxy':proxy.endpoint,'proxy_authorization':'Bearer generated-local-fixture'}})
        write_json(root/'data.json',[{'name':'first'},{'name':'second'}])
        command=[str(BINARY),'run',str(package),'--env',str(root/'environment.json'),'--data',str(root/'data.json'),'--iterations','2','--tag','smoke','--json',str(root/'result.json'),'--junit',str(root/'junit.xml'),'--html',str(root/'report.html')]
        result=subprocess.run(command,timeout=30)
        assert result.returncode==0,'runner failed'
        assert store.count()==4,'unexpected PostgreSQL rows'
        print(json.dumps({'passed':True,'case_instances':4,'postgres_rows':store.count(),'report':str(root/'report.html'),'transport':'HTTP + gRPC, mTLS + authenticated HTTP CONNECT proxy'}))
    finally:
        if server:server.close()
        if http:http.close()
        if proxy:proxy.close()
        store.close()


if __name__=='__main__':main()
