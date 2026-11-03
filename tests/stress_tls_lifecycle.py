"""Repeat TLS discovery/send while forcing frequent automatic cycle collections."""
import argparse
import gc
import json
import os
from pathlib import Path
import sys
import threading
import unittest

sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
os.environ.setdefault('QT_QPA_PLATFORM','offscreen')
from test_tls_snippets import TLSAndSnippetUI

parser=argparse.ArgumentParser()
parser.add_argument('--repeats',type=int,default=20)
parser.add_argument('--output',type=Path,required=True)
args=parser.parse_args()
owner=threading.get_ident();counts={'ui_collections':0,'worker_collections':0}
def collected(phase,info):
    if phase=='start':counts['ui_collections' if threading.get_ident()==owner else 'worker_collections']+=1
gc.callbacks.append(collected);gc.set_threshold(100,5,5)
suite=unittest.TestSuite(TLSAndSnippetUI('test_lock_enables_real_tls_reflection_and_invocation') for _ in range(args.repeats))
result=unittest.TextTestRunner(verbosity=2).run(suite)
gc.callbacks.remove(collected)
args.output.write_text(json.dumps(dict(repeats=args.repeats,passed=result.wasSuccessful(),**counts),indent=2),encoding='utf-8')
raise SystemExit(0 if result.wasSuccessful() else 1)
