"""Run a package against a temporary local gRPC server and keep its artifacts."""
import subprocess

from fixture import WorkflowServer
from package_fixture import build_package
from test_e2e import BINARY, ROOT


if __name__ == "__main__":
    directory = ROOT / "artifacts" / "package-demo"
    package = directory / "package"
    build_package(package)
    plan = subprocess.run(
        [str(BINARY), "plan", str(package)],
        capture_output=True, text=True, encoding="utf-8", timeout=15, check=True,
    )
    (directory / "plan.json").write_text(plan.stdout, encoding="utf-8")
    server = WorkflowServer()
    try:
        result = subprocess.run(
            [str(BINARY), "run", str(package), "--endpoint", server.endpoint,
             "--json", str(directory / "result.json"),
             "--junit", str(directory / "junit.xml")],
            capture_output=True, text=True, encoding="utf-8", timeout=15,
        )
        (directory / "events.ndjson").write_text(result.stdout, encoding="utf-8")
        print(result.stdout, end="")
        if result.stderr:
            print(result.stderr, end="")
        print(f"Artifacts: {directory}")
        raise SystemExit(result.returncode)
    finally:
        server.close()
