"""Local comparative experiment, not a language benchmark or full GUI profile."""
import argparse
import ctypes
import json
import math
import os
import platform
import statistics
import subprocess
import sys
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REPOSITORY = ROOT.parents[1]
BINARY = ROOT / "target" / "release" / ("reqon-prototype.exe" if os.name == "nt" else "reqon-prototype")


def python_worker(mode, path):
    sys.path.insert(0, str(REPOSITORY))
    import grpc_support
    from google.protobuf import json_format, message_factory

    spec = json.loads(Path(path).read_text(encoding="utf-8"))
    started = time.perf_counter()
    channel = None
    responses = []
    call_durations = []
    payload = spec["steps"][0]["request"]["payload"]
    if mode == "python_reuse":
        channel = grpc_support.channel_for(spec["endpoint"], False)
        pool = grpc_support._pool(channel, "")
        method = pool.FindServiceByName("poc.Workflow").FindMethodByName("Echo")
        input_cls = message_factory.GetMessageClass(method.input_type)
        output_cls = message_factory.GetMessageClass(method.output_type)
        call = channel.unary_unary("/poc.Workflow/Echo")
    try:
        for step in spec["steps"]:
            call_started = time.perf_counter()
            request = {"payload": payload}
            if mode == "python_current":
                result = grpc_support.invoke({"server": spec["endpoint"], "service": "poc.Workflow", "rpc_method": "Echo", "message": json.dumps(request)}, {}, lambda value, _: value)
                response = json.loads(result["body"])
            else:
                raw = json_format.ParseDict(request, input_cls()).SerializeToString()
                response = json_format.MessageToDict(output_cls.FromString(call(raw, timeout=2)), preserving_proto_field_name=True)
            assert response["status"] == "CREATED"
            responses.append(response)
            call_durations.append((time.perf_counter() - call_started) * 1000)
    finally:
        if channel is not None:
            channel.close()
    duration_ms = (time.perf_counter() - started) * 1000
    assert all(response["payload"] == payload for response in responses)
    # Preserve response output work in both Python baselines; this is still not
    # equivalent to the Rust runner's NDJSON, JUnit and complete execution model.
    Path(path).with_suffix(".python-result.json").write_text(json.dumps(responses, indent=2), encoding="utf-8")
    print(json.dumps({"duration_ms": duration_ms, "steady_ms": sum(call_durations[1:]) if len(call_durations) > 1 else None}))


def sample_process(command):
    started = time.perf_counter()
    process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    stop = threading.Event()
    peaks = []

    def monitor():
        # Windows peak working set, sampled every 2 ms. No server or child VM RAM.
        class Counters(ctypes.Structure):
            _fields_ = [("cb", ctypes.c_ulong), ("PageFaultCount", ctypes.c_ulong)] + [(name, ctypes.c_size_t) for name in ("PeakWorkingSetSize", "WorkingSetSize", "QuotaPeakPagedPoolUsage", "QuotaPagedPoolUsage", "QuotaPeakNonPagedPoolUsage", "QuotaNonPagedPoolUsage", "PagefileUsage", "PeakPagefileUsage")]
        if os.name != "nt":
            return
        api = ctypes.WinDLL("psapi", use_last_error=True).GetProcessMemoryInfo
        api.argtypes = [ctypes.c_void_p, ctypes.POINTER(Counters), ctypes.c_ulong]
        api.restype = ctypes.c_int
        while not stop.is_set():
            counters = Counters()
            counters.cb = ctypes.sizeof(counters)
            if api(int(process._handle), ctypes.byref(counters), counters.cb):
                peaks.append(counters.PeakWorkingSetSize)
            stop.wait(0.002)

    thread = threading.Thread(target=monitor, daemon=True)
    thread.start()
    try:
        stdout, stderr = process.communicate(timeout=60)
    except BaseException:
        process.kill()
        process.communicate()
        raise
    finally:
        stop.set()
        thread.join()
    wall_ms = (time.perf_counter() - started) * 1000
    if process.returncode:
        raise RuntimeError(stderr.decode("utf-8", errors="replace") + stdout.decode("utf-8", errors="replace"))
    return {"wall_ms": wall_ms, "peak_working_set_mib": max(peaks) / 1048576 if peaks else None}, stdout


def benchmark(repeats):
    from fixture import WorkflowServer
    directory = ROOT / "artifacts"
    directory.mkdir(exist_ok=True)
    server = WorkflowServer()
    rows = []
    cases = [(1, 1024), (100, 1024), (100, 102400)]
    modes = ["rust", "python_current", "python_reuse"]
    try:
        for count, size in cases:
            path = directory / "benchmark-scenario.json"
            spec = {"schema_version": "reqon-prototype/v1", "id": "benchmark", "endpoint": server.endpoint, "timeout_ms": 60000, "steps": [{"type": "grpc", "id": f"echo-{i}", "method": "poc.Workflow/Echo", "request": {"payload": "x" * size}, "expect": [{"path": "/status", "equals": "CREATED"}]} for i in range(count)]}
            # 100 x 100 KiB exceeds the prototype's input file cap; use a single
            # JS-free seed call and save/substitute the payload for later calls.
            spec["steps"][0]["save"] = {"payload": "/payload"}
            for step in spec["steps"][1:]:
                step["request"]["payload"] = "${payload}"
            path.write_text(json.dumps(spec), encoding="utf-8")
            samples = {mode: [] for mode in modes}
            for repetition in range(repeats + 1):
                # Rotate execution order to reduce systematic warmup/order bias.
                order = modes[repetition % len(modes):] + modes[:repetition % len(modes)]
                for mode in order:
                    if mode == "rust":
                        result = directory / "benchmark-result.json"
                        command = [str(BINARY), "run", str(path), "--json", str(result), "--junit", str(directory / "benchmark-junit.xml")]
                    else:
                        command = [sys.executable, str(Path(__file__).resolve()), "--python-worker", mode, "--scenario", str(path)]
                    metrics, stdout = sample_process(command)
                    if mode == "rust":
                        report = json.loads(result.read_text(encoding="utf-8"))
                        assert report["outcome"] == "passed"
                        assert all(step["response"]["payload"] == "x" * size for step in report["steps"])
                        metrics["core_ms"] = report["duration_ms"]
                        metrics["steady_ms"] = sum(step["duration_ms"] for step in report["steps"][1:]) if count > 1 else None
                    else:
                        worker = json.loads(stdout)
                        metrics["core_ms"] = worker["duration_ms"]
                        metrics["steady_ms"] = worker["steady_ms"]
                    if repetition:
                        samples[mode].append(metrics)
                server.calls.clear()
            for mode in modes:
                summary = {key + "_median": statistics.median(s[key] for s in samples[mode] if s[key] is not None) for key in ("wall_ms", "core_ms", "steady_ms", "peak_working_set_mib") if any(s[key] is not None for s in samples[mode])}
                summary["wall_ms_p95"] = sorted(s["wall_ms"] for s in samples[mode])[math.ceil(repeats * .95) - 1]
                row = {"mode": mode, "calls": count, "payload_bytes": size, **summary, "samples": samples[mode]}
                rows.append(row)
                print(json.dumps({k: v for k, v in row.items() if k != "samples"}), flush=True)
    finally:
        server.close()
    output = {"platform": platform.platform(), "python": platform.python_version(), "processor": platform.processor(), "logical_cpus": os.cpu_count(), "repetitions": repeats, "warmups_per_case_mode": 1, "binary_bytes": BINARY.stat().st_size, "methodology": "Loopback independent Python gRPC fixture; sequential Echo, real reflection, fresh process each sample, no GUI. Python current uses unchanged grpc_support.invoke; reuse uses its channel/pool helpers once. Core excludes initial imports and final report writes, but includes Python lazy imports on first channel creation. Wall includes startup/imports/reports/process shutdown. Steady sums call/step durations after the first call (99 calls for a 100-call case); excludes startup/initial reflection/lazy imports, but current Python still reconnects and reflects each call. Different runner features, libraries and bookkeeping; not a pure language comparison. Windows sampled peak working set excludes fixture and child processes. Warm filesystem caches; not a cold boot.", "results": rows}
    target = directory / "benchmark.json"
    target.write_text(json.dumps(output, indent=2), encoding="utf-8")
    print(f"Saved {target}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--repeats", type=int, default=7)
    parser.add_argument("--python-worker", choices=["python_current", "python_reuse"])
    parser.add_argument("--scenario")
    args = parser.parse_args()
    if args.python_worker:
        python_worker(args.python_worker, args.scenario)
    else:
        if args.repeats < 1:
            parser.error("--repeats must be positive")
        benchmark(args.repeats)
