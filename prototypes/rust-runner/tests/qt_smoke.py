"""Real Qt event loop + QProcess IPC; no changes to the desktop application."""
import json
import sys
import tempfile
import time
from pathlib import Path

from PySide6.QtCore import QCoreApplication, QProcess, QTimer
from fixture import WorkflowServer, sample
from test_e2e import BINARY, ROOT


def main():
    app = QCoreApplication([])
    server = WorkflowServer()
    temp = tempfile.TemporaryDirectory(prefix="reqon-qt-smoke-")
    folder = Path(temp.name)
    scenario = sample(server.endpoint)
    scenario["steps"] = [{"type": "grpc", "id": "slow", "method": "poc.Workflow/Slow", "timeout_ms": 9000}]
    path = folder / "scenario.json"
    path.write_text(json.dumps(scenario), encoding="utf-8")
    process = QProcess()
    timer = QTimer()
    timer.setInterval(10)
    ticks = []
    events = []
    pending = bytearray()
    state = {"rpc_started": None, "cancel_sent": None, "error": None}

    def read():
        pending.extend(bytes(process.readAllStandardOutput()))
        while b"\n" in pending:
            line, _, remainder = pending.partition(b"\n")
            pending[:] = remainder
            events.append(json.loads(line))

    def tick():
        now = time.perf_counter()
        ticks.append(now)
        if server.slow_started.is_set() and state["rpc_started"] is None:
            state["rpc_started"] = now
        if state["rpc_started"] and not state["cancel_sent"] and now - state["rpc_started"] >= .3:
            state["cancel_sent"] = now
            process.write(b'{"type":"cancel"}\n')

    def finish(code, status):
        read()
        timer.stop()
        state["exit_code"] = code
        state["exit_status"] = status.name
        state["finished"] = time.perf_counter()
        app.quit()

    def watchdog():
        state["error"] = "Qt integration timed out"
        process.kill()
        app.quit()

    process.readyReadStandardOutput.connect(read)
    process.finished.connect(finish)
    timer.timeout.connect(tick)
    timer.start()
    QTimer.singleShot(6000, watchdog)
    process.start(str(BINARY), ["run", str(path), "--json", str(folder / "result.json"), "--junit", str(folder / "junit.xml"), "--control-stdin"])
    try:
        app.exec()
        assert not state["error"], state["error"]
        assert state["exit_code"] == 130, bytes(process.readAllStandardError())
        assert state["exit_status"] == "NormalExit"
        assert server.slow_cancelled.wait(2), "RPC was not cancelled on the server"
        report = json.loads((folder / "result.json").read_text())
        assert report["outcome"] == "cancelled"
        assert events[-1]["type"] == "run_finished"
        assert len(ticks) >= 10, "Qt timer did not run while RPC was pending"
        result = {"qt_ticks": len(ticks), "maximum_timer_gap_ms": max((b - a) * 1000 for a, b in zip(ticks, ticks[1:])), "cancel_to_process_exit_ms": (state["finished"] - state["cancel_sent"]) * 1000, "outcome": report["outcome"], "exit_code": state["exit_code"], "events": [e["type"] for e in events]}
        output = ROOT / "artifacts" / "qt-smoke.json"
        output.parent.mkdir(exist_ok=True)
        output.write_text(json.dumps(result, indent=2), encoding="utf-8")
        print(json.dumps(result, indent=2))
    finally:
        if process.state() != QProcess.ProcessState.NotRunning:
            process.kill()
            process.waitForFinished(2000)
        server.close()
        temp.cleanup()


if __name__ == "__main__":
    main()
