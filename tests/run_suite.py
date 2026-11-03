"""Run Qt test modules in separate processes, with logs and bounded timeouts."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time

root=Path(__file__).resolve().parents[1]
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--pattern',default='test_*.py')
parser.add_argument('--output',type=Path,default=root/'build/optimization-tests.json')
args=parser.parse_args();args.output.parent.mkdir(parents=True,exist_ok=True)
logs=args.output.parent/(args.output.stem+'-logs');logs.mkdir(exist_ok=True)
environment=dict(os.environ);environment.setdefault('QT_QPA_PLATFORM','offscreen')
results=[]
for module in sorted((root/'tests').glob(args.pattern)):
    start=time.monotonic();log=logs/(module.stem+'.log')
    with log.open('wb') as output:
        try:
            result=subprocess.run([sys.executable,'-X','faulthandler','-m','unittest','discover','-s','tests','-p',module.name,'-v'],cwd=root,env=environment,stdout=output,stderr=subprocess.STDOUT,timeout=240)
            code=result.returncode
        except subprocess.TimeoutExpired:code='timeout'
    text=log.read_text(encoding='utf-8',errors='replace');count=re.search(r'Ran (\d+) tests?',text)
    row=dict(module=module.name,exit_code=code,tests=int(count[1]) if count else None,seconds=round(time.monotonic()-start,2),log=str(log))
    results.append(row);args.output.write_text(json.dumps(results,indent=2),encoding='utf-8')
    print(json.dumps(row),flush=True)
raise SystemExit(0 if all(row['exit_code']==0 for row in results) else 1)
