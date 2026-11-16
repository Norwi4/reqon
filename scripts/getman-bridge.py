"""Exercise the real getman collection store in an isolated directory (no GUI)."""
import json
import sys
from pathlib import Path

source, incoming, outgoing, workspace = map(Path, sys.argv[1:])
sys.path.insert(0, str(source))
from main import Store

store = Store(str(workspace))
try:
    ids = store.import_documents(json.loads(incoming.read_text(encoding="utf-8")))
    document = store.export_documents(ids)
    assert not store.all("PRAGMA foreign_key_check")
    outgoing.write_text(json.dumps(document, ensure_ascii=False, indent=2), encoding="utf-8")
finally:
    store.db.close()
