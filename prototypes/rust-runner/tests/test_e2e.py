"""Black-box verification against real local gRPC, no mock transport in Rust."""
import copy
import json
import os
import subprocess
import sys
import tempfile
import time
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

from fixture import WorkflowServer, sample

ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(os.environ.get("REQON_PROTOTYPE_BIN", ROOT / "target" / "release" / ("reqon-prototype.exe" if os.name == "nt" else "reqon-prototype")))


class RunnerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not BINARY.is_file():
            raise RuntimeError(f"Build release binary first: {BINARY}")

    def setUp(self):
        self.server = WorkflowServer()
        self.temp = tempfile.TemporaryDirectory(prefix="reqon-rust-poc-")
        self.directory = Path(self.temp.name)

    def tearDown(self):
        self.server.close()
        self.temp.cleanup()

    def command(self, scenario):
        path = self.directory / "scenario.json"
        path.write_text(json.dumps(scenario, ensure_ascii=False), encoding="utf-8")
        return [str(BINARY), "run", str(path), "--json", str(self.directory / "result.json"), "--junit", str(self.directory / "junit.xml")]

    def run_case(self, scenario, code=0, outcome="passed"):
        completed = subprocess.run(self.command(scenario), capture_output=True, text=True, encoding="utf-8", timeout=15)
        self.assertEqual(completed.returncode, code, completed.stdout + completed.stderr)
        report = json.loads((self.directory / "result.json").read_text(encoding="utf-8"))
        self.assertEqual(report["outcome"], outcome)
        events = [json.loads(line) for line in completed.stdout.splitlines()]
        self.assertEqual(events[0]["type"], "run_started")
        self.assertEqual(events[-1]["type"], "run_finished")
        self.assertEqual(events[-1]["exit_code"], code)
        suite = ET.parse(self.directory / "junit.xml").getroot().find("testsuite")
        self.assertEqual(int(suite.attrib["tests"]), len(scenario["steps"]))
        return report, suite, events

    def single(self, method="Echo", **kwargs):
        scenario = sample(self.server.endpoint)
        scenario["steps"] = [{"type": "grpc", "id": "one", "method": "poc.Workflow/" + method, **kwargs}]
        return scenario

    def js(self, script, timeout=100):
        scenario = sample(self.server.endpoint)
        scenario["steps"] = [{"type": "javascript", "id": "script", "script": script, "timeout_ms": timeout}]
        return scenario

    def test_chain_reflection_dependency_variables_poll_js_reports(self):
        scenario = sample(self.server.endpoint)
        scenario["metadata"] = {"x-reqon-prototype": "fixture"}
        report, suite, events = self.run_case(scenario)
        self.assertEqual(report["variables"]["pollCount"], 3)
        self.assertIs(report["variables"]["verified"], True)
        self.assertEqual(report["steps"][1]["attempts"], 3)
        self.assertEqual(report["reflection_requests"], 2)  # v1 unimplemented, then v1alpha; shared descriptor cached
        self.assertEqual(suite.attrib["failures"], "0")
        get_calls = [c for c in self.server.calls if c[0] == "Get"]
        self.assertEqual(len(get_calls), 3)
        self.assertTrue(all(c[1] == report["variables"]["objectId"] for c in get_calls))
        self.assertTrue(all(c[2]["x-reqon-prototype"] == "fixture" for c in self.server.calls))

    def test_business_failure_is_not_transport_failure(self):
        scenario = self.single("BusinessError", expect=[{"path": "/serverCallInfo/code", "equals": "OK"}])
        scenario["steps"].append({"type": "grpc", "id": "must-not-run", "method": "poc.Workflow/Echo"})
        report, suite, _ = self.run_case(scenario, 1, "failed")
        self.assertEqual(report["steps"][0]["grpc_code"], "OK")
        self.assertEqual(report["steps"][1]["outcome"], "skipped")
        self.assertEqual(suite.attrib["failures"], "1")
        self.assertEqual(suite.attrib["errors"], "0")
        self.assertEqual(len(self.server.calls), 1)

    def test_expected_business_error_passes(self):
        self.run_case(self.single("BusinessError", expect=[{"path": "/serverCallInfo/code", "equals": "INVALID_INPUT"}]))

    def test_transport_error_is_infrastructure_error(self):
        report, suite, _ = self.run_case(self.single("TransportError"), 2, "error")
        self.assertEqual(report["steps"][0]["grpc_code"], "Unavailable")
        self.assertEqual(suite.attrib["errors"], "1")

    def test_poll_timeout_is_failed_check_and_no_save(self):
        scenario = sample(self.server.endpoint)
        scenario["steps"][0]["request"]["name"] = "never"
        scenario["steps"][1]["poll"]["timeout_ms"] = 100
        # Let the condition budget expire between completed calls. A short
        # interval can instead start a call just before the deadline; that is
        # correctly a transport error, tested separately below.
        scenario["steps"][1]["poll"]["interval_ms"] = 300
        report, _, _ = self.run_case(scenario, 1, "failed")
        self.assertEqual(report["steps"][1]["attempts"], 1)
        self.assertNotIn("pollCount", report["variables"])
        self.assertEqual(report["steps"][2]["outcome"], "skipped")

    def test_poll_deadline_during_inflight_rpc_is_transport_error(self):
        scenario = self.single("Slow", timeout_ms=2000, poll={"read_only": True, "interval_ms": 10, "timeout_ms": 120, "until": [{"path": "/count", "equals": 7}]}, save={"value": "/count"})
        report, suite, _ = self.run_case(scenario, 2, "error")
        self.assertEqual(suite.attrib["errors"], "1")
        self.assertNotIn("value", report["variables"])
        self.assertTrue(self.server.slow_cancelled.wait(2))

    def test_rpc_deadline_cancels_transport(self):
        report, _, _ = self.run_case(self.single("Slow", timeout_ms=120), 2, "error")
        # Tonic's own deadline layer can surface Cancelled/Timeout expired;
        # the runner must still classify it as an error, not a user cancellation.
        self.assertIn(report["steps"][0]["grpc_code"], ("DeadlineExceeded", "Cancelled"))
        self.assertRegex(report["steps"][0]["error"].lower(), "timeout|deadline")
        self.assertTrue(self.server.slow_cancelled.wait(2))

    def test_ipc_cancel_interrupts_inflight_call_and_writes_reports(self):
        scenario = self.single("Slow", timeout_ms=9000)
        process = subprocess.Popen(self.command(scenario) + ["--control-stdin"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8")
        try:
            self.assertTrue(self.server.slow_started.wait(5))
            start = time.perf_counter()
            stdout, stderr = process.communicate('{"type":"cancel"}\n', timeout=5)
            latency = (time.perf_counter() - start) * 1000
            self.assertEqual(process.returncode, 130, stdout + stderr)
            report = json.loads((self.directory / "result.json").read_text())
            self.assertEqual(report["outcome"], "cancelled")
            self.assertLess(latency, 1500)
            self.assertTrue(self.server.slow_cancelled.wait(2))
            self.assertEqual(ET.parse(self.directory / "junit.xml").find("testsuite").attrib["errors"], "1")
        finally:
            if process.poll() is None:
                process.kill()
                process.communicate()

    def test_javascript_assertion_and_xml_escaping(self):
        scenario = self.js('assert(false, "<bad>&\\\" failure"); return {};')
        scenario["id"] = 'Сценарий <&"'
        report, suite, _ = self.run_case(scenario, 1, "failed")
        self.assertEqual(suite.attrib["name"], scenario["id"])
        self.assertIn("ASSERTION", report["steps"][0]["error"])

    def test_javascript_loop_has_time_limit_and_next_process_recovers(self):
        report, _, _ = self.run_case(self.js("while(true) {}", 50), 2, "error")
        self.assertLess(report["duration_ms"], 2000)
        self.assertIn("timeout", report["steps"][0]["error"])
        self.run_case(self.js("return {recovered: true};"))

    def test_javascript_heap_has_limit(self):
        report, _, _ = self.run_case(self.js("const a=[]; for(let i=0;i<1000000;i++)a.push({i,s:'x'.repeat(1000)}); return {};", 1000), 2, "error")
        self.assertLess(report["duration_ms"], 3000)

    @unittest.skipUnless(os.name == "nt", "Windows child process lifetime check")
    def test_cancel_javascript_kills_worker_process(self):
        import ctypes
        api = ctypes.WinDLL("kernel32", use_last_error=True)
        api.OpenProcess.argtypes = [ctypes.c_ulong, ctypes.c_int, ctypes.c_ulong]
        api.OpenProcess.restype = ctypes.c_void_p
        api.WaitForSingleObject.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
        api.CloseHandle.argtypes = [ctypes.c_void_p]
        handle = None
        process = subprocess.Popen(self.command(self.js("while(true) {}", 5000)) + ["--control-stdin"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8")
        try:
            # Keep the read bounded even if a regression stops emitting events.
            import queue
            import threading
            lines = queue.Queue()
            def read_events():
                for line in process.stdout:
                    event = json.loads(line)
                    if event["type"] == "script_worker_started":
                        lines.put(event)
                        return
            reader = threading.Thread(target=read_events, daemon=True)
            reader.start()
            event = lines.get(timeout=5)
            reader.join(timeout=1)
            handle = api.OpenProcess(0x00100000, False, event["pid"])  # SYNCHRONIZE
            self.assertTrue(handle, "JS child already exited before cancellation")
            self.assertEqual(api.WaitForSingleObject(handle, 20), 258)  # WAIT_TIMEOUT: child is alive
            stdout, stderr = process.communicate('{"type":"cancel"}\n', timeout=3)
            self.assertEqual(process.returncode, 130, stdout + stderr)
            self.assertEqual(api.WaitForSingleObject(handle, 1000), 0, "orphaned JS process")
            report = json.loads((self.directory / "result.json").read_text())
            self.assertEqual(report["outcome"], "cancelled")
        finally:
            if process.poll() is None:
                process.kill()
                process.communicate()
            if handle:
                api.CloseHandle(handle)

    def test_javascript_has_no_node_or_browser_host_apis(self):
        self.run_case(self.js("assert(typeof process === 'undefined'); assert(typeof require === 'undefined'); assert(typeof fetch === 'undefined'); return {isolated: true};"))

    def test_unknown_variable_and_invalid_message_are_errors(self):
        report, _, _ = self.run_case(self.single(request={"id": "${missing}"}), 2, "error")
        self.assertIn("unknown variable", report["steps"][0]["error"])
        self.assertEqual(self.server.calls, [])
        self.run_case(self.single(request={"notInContract": 1}), 2, "error")
        self.assertEqual(self.server.calls, [])

    def test_unimplemented_streaming_is_explicit_error(self):
        report, _, _ = self.run_case(self.single("Watch"), 2, "error")
        self.assertIn("streaming", report["steps"][0]["error"])

    def test_response_size_limit(self):
        scenario = self.single(request={"payload": "x" * 20000})
        scenario["max_response_bytes"] = 1024
        self.run_case(scenario, 2, "error")

    def test_missing_field_is_not_equal_to_null(self):
        report, _, _ = self.run_case(self.single(expect=[{"path": "/absent", "equals": None}]), 1, "failed")
        self.assertFalse(report["steps"][0]["checks"][0]["passed"])

    def test_failed_extraction_does_not_partially_commit_variables(self):
        report, _, _ = self.run_case(self.single(save={"first": "/count", "second": "/absent"}), 2, "error")
        self.assertEqual(report["variables"], {})

    def test_run_deadline_stops_javascript(self):
        scenario = self.js("while(true) {}", 100)
        scenario["timeout_ms"] = 100
        report, _, _ = self.run_case(scenario, 2, "error")
        self.assertLess(report["duration_ms"], 2000)

    def test_unreachable_endpoint_is_error(self):
        import socket
        # Keep a socket bound but not listening so no service can claim its port.
        with socket.socket() as reserved:
            reserved.bind(("127.0.0.1", 0))
            scenario = self.single(timeout_ms=500)
            scenario["endpoint"] = f"http://127.0.0.1:{reserved.getsockname()[1]}"
            self.run_case(scenario, 2, "error")

    def test_reflection_unavailable_is_explicit_error(self):
        server = WorkflowServer(reflection_enabled=False)
        try:
            scenario = self.single()
            scenario["endpoint"] = server.endpoint
            self.run_case(scenario, 2, "error")
        finally:
            server.close()

    def test_reflection_v1_is_used_when_available(self):
        server = WorkflowServer(reflection_v1=True)
        try:
            scenario = self.single()
            scenario["endpoint"] = server.endpoint
            report, _, _ = self.run_case(scenario)
            self.assertEqual(report["reflection_requests"], 1)
        finally:
            server.close()

    def test_invalid_scenarios_fail_before_network(self):
        cases = []
        empty = self.single(); empty["steps"] = []; cases.append(empty)
        unknown = self.single(); unknown["unknown"] = True; cases.append(unknown)
        duplicate = self.single(); duplicate["steps"] *= 2; cases.append(duplicate)
        remote = self.single(); remote["endpoint"] = "http://example.com:80"; cases.append(remote)
        poll = sample(self.server.endpoint); poll["steps"][1]["poll"]["read_only"] = False; cases.append(poll)
        for scenario in cases:
            with self.subTest(scenario=scenario):
                p = subprocess.run(self.command(scenario), capture_output=True, text=True, timeout=5)
                self.assertEqual(p.returncode, 2)
                self.assertEqual(p.stdout, "")
        self.assertEqual(self.server.calls, [])


if __name__ == "__main__":
    unittest.main(verbosity=2)
