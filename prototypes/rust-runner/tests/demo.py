"""Run the complete example against an automatically managed local fixture."""
import json
import subprocess
from fixture import WorkflowServer, sample
from test_e2e import BINARY, ROOT


if __name__ == "__main__":
    directory = ROOT / "artifacts" / "demo"
    directory.mkdir(parents=True, exist_ok=True)
    server = WorkflowServer()
    try:
        path = directory / "scenario.json"
        path.write_text(json.dumps(sample(server.endpoint), ensure_ascii=False, indent=2), encoding="utf-8")
        result = subprocess.run([str(BINARY), "run", str(path), "--json", str(directory / "result.json"), "--junit", str(directory / "junit.xml")], capture_output=True, text=True, encoding="utf-8", timeout=15)
        (directory / "events.ndjson").write_text(result.stdout, encoding="utf-8")
        print(result.stdout, end="")
        if result.stderr:
            print(result.stderr, end="")
        print(f"Artifacts: {directory}")
        raise SystemExit(result.returncode)
    finally:
        server.close()
