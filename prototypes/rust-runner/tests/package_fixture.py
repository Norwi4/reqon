"""Synthetic package, also used to generate the documented example."""
import json
from pathlib import Path
from fixture import sample


def write_json(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def build_package(root):
    root = Path(root)
    chain = sample("http://127.0.0.1:1")
    chain.pop("endpoint")
    chain.update(schema_version="reqon-case/v1", id="CASE-001", name="Create and wait for completion", tags=["smoke"], annotations={"vziskaniya": {"bf": "example", "domain": "example", "kind": "positive", "zephyr": "EXAMPLE-T1"}})
    isolation = {"schema_version": "reqon-case/v1", "id": "CASE-002", "name": "Independent case variables", "steps": [{"type": "javascript", "id": "isolation", "script": "assert(typeof vars.objectId === 'undefined', 'variables leaked between cases'); return {isolated: true};", "timeout_ms": 100}]}
    disabled = {"schema_version": "reqon-case/v1", "id": "CASE-003", "name": "Disabled example", "disabled": True, "steps": [{"type": "javascript", "id": "never", "script": "assert(false, 'disabled case executed'); return {};"}]}
    manifest = {"schema_version": "reqon-package/v1", "id": "example-regression", "name": "Example regression package", "requires": {"engine": "reqon", "version": ">=0.3.0, <0.15.0", "capabilities": ["grpc.unary", "javascript", "poll"]}, "cases": ["cases/chain.json", "cases/isolation.json", "cases/disabled.json"], "suites": ["suites/smoke.json", "suites/reverse.json"], "default_suite": "smoke", "timeout_ms": 60000}
    files = {"catalog.json": manifest, "cases/chain.json": chain, "cases/isolation.json": isolation, "cases/disabled.json": disabled, "suites/smoke.json": {"schema_version": "reqon-suite/v1", "id": "smoke", "name": "Smoke", "cases": ["CASE-001", "CASE-002", "CASE-003"]}, "suites/reverse.json": {"schema_version": "reqon-suite/v1", "id": "reverse", "name": "Reverse", "cases": ["CASE-002", "CASE-001"]}}
    for name, value in files.items():
        write_json(root / name, value)
    return files
