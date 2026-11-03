"""The CLI and the Qt QProcess adapter must produce the same assertion verdicts."""
import json
import subprocess
import tempfile
from pathlib import Path
from PySide6.QtCore import QCoreApplication,QProcess
from fixture import WorkflowServer
from package_fixture import write_json
from service_fixture import HttpFixture
from test_e2e import BINARY,ROOT


def verdict(report):
    return {'outcome':report['outcome'],'steps':[{'outcome':s['outcome'],'checks':[(c['target'],c['operation'],c['passed']) for c in s['checks']]} for s in report['steps']]}


def main():
    app=QCoreApplication([])
    http=HttpFixture();grpc=WorkflowServer();results=[]
    try:
        with tempfile.TemporaryDirectory(prefix='reqon-qt-parity-') as directory:
            root=Path(directory)
            for kind in ['http','grpc']:
                for positive in [True,False]:
                    if kind=='http':
                        step={'type':'http','id':'assertions','request':{'url':'/echo'},'expect':[{'target':'status','equals':200 if positive else 418},{'path':'/nullValue','equals':None},{'path':'/missing','exists':False},{'target':'headers','path':'/x-multi','contains':'two'},{'target':'elapsed_ms','less_than':1000},{'path':'','schema':{'type':'object','required':['method']}}]}
                    else:
                        step={'type':'grpc','id':'assertions','method':'poc.Workflow/Echo' if positive else 'poc.Workflow/BusinessError','expect':[{'path':'/serverCallInfo/success','equals':True},{'target':'trailers','path':'/x-finished','equals':['yes']},{'path':'/payload','exists':False}]}
                    path=root/'case.json';write_json(path,{'schema_version':'reqon-prototype/v1','id':'qt-parity','endpoint':http.endpoint if kind=='http' else grpc.endpoint,'steps':[step]})
                    args=['run',str(path),'--json',str(root/'cli.json'),'--junit',str(root/'cli.xml')]
                    cli=subprocess.run([str(BINARY),*args],capture_output=True,timeout=10)
                    args=['run',str(path),'--json',str(root/'qt.json'),'--junit',str(root/'qt.xml')]
                    process=QProcess();process.start(str(BINARY),args)
                    assert process.waitForFinished(10000),'Qt parity timed out'
                    assert process.exitStatus()==QProcess.ExitStatus.NormalExit
                    assert process.exitCode()==cli.returncode==(0 if positive else 1)
                    a=verdict(json.loads((root/'cli.json').read_text()));b=verdict(json.loads((root/'qt.json').read_text()))
                    assert a==b,(a,b)
                    results.append({'protocol':kind,'positive':positive,'verdict':a})
        out=ROOT/'artifacts/qt-parity.json';out.write_text(json.dumps(results,indent=2),encoding='utf8')
        print(json.dumps({'equivalent_scenarios':len(results),'passed':True}))
    finally:http.close();grpc.close()


if __name__=='__main__':main()
