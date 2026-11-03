import copy
import hashlib
import json
import os
import shutil
import subprocess
import tempfile
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

import jsonschema
from fixture import WorkflowServer
from package_fixture import build_package, write_json
from test_e2e import BINARY, ROOT


class PackageTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="reqon-package-")
        self.root = Path(self.temp.name)
        self.package = self.root / "package"
        self.files = build_package(self.package)
        self.server = WorkflowServer()

    def tearDown(self):
        self.server.close()
        self.temp.cleanup()

    def update(self, relative, **fields):
        self.files[relative].update(fields)
        write_json(self.package / relative, self.files[relative])

    def invoke(self, *args, code=0):
        result = subprocess.run([str(BINARY), *map(str, args)], capture_output=True, text=True, encoding="utf-8", timeout=15)
        self.assertEqual(result.returncode, code, result.stdout + result.stderr)
        return result

    def validate(self, *args, code=0):
        result = self.invoke("validate", self.package, "--json", *args, code=code)
        report = json.loads(result.stdout)
        self.assertEqual(report["valid"], code == 0)
        return report

    def plan(self, *args, path=None):
        return json.loads(self.invoke("plan", path or self.package, *args).stdout)

    def command(self, *args):
        return [str(BINARY), "run", str(self.package), "--endpoint", self.server.endpoint, "--json", str(self.root / "result.json"), "--junit", str(self.root / "junit.xml"), *args]

    def run_package(self, *args, code=0):
        result = subprocess.run(self.command(*args), capture_output=True, text=True, encoding="utf-8", timeout=15)
        self.assertEqual(result.returncode, code, result.stdout + result.stderr)
        report = json.loads((self.root / "result.json").read_text(encoding="utf-8"))
        junit = ET.parse(self.root / "junit.xml").getroot()
        events = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertEqual(events[0]["type"], "package_started")
        self.assertEqual(events[-1]["type"], "package_finished")
        self.assertEqual(events[-1]["exit_code"], code)
        return report, junit, events

    def test_offline_validation_and_full_chain_run(self):
        validation = self.validate()
        self.assertEqual(validation["case_count"], 3)
        self.assertEqual(self.server.calls, [])
        plan = self.plan()
        report, junit, events = self.run_package()
        self.assertEqual(report["schema_version"], "reqon-package-result/v1")
        self.assertEqual(report["plan"], plan)
        self.assertEqual(report["outcome"], "passed")
        self.assertEqual([c["outcome"] for c in report["cases"]], ["passed", "passed", "skipped"])
        self.assertTrue(report["cases"][0]["report"]["variables"]["verified"])
        self.assertEqual(report["cases"][1]["report"]["variables"], {"isolated": True, "iteration": 1})
        self.assertEqual(report["cases"][2]["skip_reason"], "disabled")
        self.assertEqual(junit.findall("testsuite")[2].attrib["skipped"], "1")
        self.assertEqual(junit.find("testsuite/testcase").attrib["classname"], "example-regression/CASE-001")
        self.assertTrue(all(e["case_id"] == "CASE-001" for e in events if e["type"] == "attempt_started"))
        self.assertEqual(plan["cases"][0]["annotations"]["vziskaniya"]["zephyr"], "EXAMPLE-T1")

    def test_same_snapshot_in_another_directory_has_same_plan(self):
        before = self.plan()
        other = self.root / "other-checkout"
        shutil.copytree(self.package, other)
        self.assertEqual(before, self.plan(path=other))
        self.assertEqual(before, self.plan(path=self.package / "catalog.json"))
        for item in before["files"]:
            self.assertEqual(item["sha256"], hashlib.sha256((self.package / item["path"]).read_bytes()).hexdigest())

    def test_plan_digest_is_independently_reproducible(self):
        plan = self.plan()
        expected = plan.pop("plan_sha256")
        raw = json.dumps(plan, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
        self.assertEqual(hashlib.sha256(raw).hexdigest(), expected)

    def test_rename_preserves_case_id_and_case_digest(self):
        before = self.plan()
        source = self.package / "cases/chain.json"
        target = self.package / "cases/renamed.json"
        source.rename(target)
        paths = self.files["catalog.json"]["cases"]
        paths[0] = "cases/renamed.json"
        self.update("catalog.json", cases=paths)
        after = self.plan()
        self.assertEqual(before["cases"][0]["id"], after["cases"][0]["id"])
        self.assertEqual(before["cases"][0]["sha256"], after["cases"][0]["sha256"])
        self.assertNotEqual(before["package_sha256"], after["package_sha256"])

    def test_formatting_changes_snapshot_but_not_case_digest(self):
        before = self.plan()
        (self.package / "cases/chain.json").write_text(json.dumps(self.files["cases/chain.json"], sort_keys=True), encoding="utf-8-sig")
        after = self.plan()
        self.assertEqual(before["cases"][0]["sha256"], after["cases"][0]["sha256"])
        self.assertNotEqual(before["package_sha256"], after["package_sha256"])

    def test_content_change_updates_case_and_plan_digests(self):
        before = self.plan()
        self.update("cases/chain.json", name="Changed display name")
        after = self.plan()
        for field in ("package_sha256", "plan_sha256"):
            self.assertNotEqual(before[field], after[field])
        self.assertNotEqual(before["cases"][0]["sha256"], after["cases"][0]["sha256"])

    def test_suite_and_case_selection_order(self):
        reverse = self.plan("--suite", "reverse")
        self.assertEqual([c["id"] for c in reverse["cases"]], ["CASE-002", "CASE-001"])
        explicit = self.plan("--case", "CASE-002", "--case", "CASE-001")
        self.assertIsNone(explicit["suite_id"])
        self.assertEqual([c["id"] for c in explicit["cases"]], ["CASE-002", "CASE-001"])
        report, _, _ = self.run_package("--case", "CASE-002")
        self.assertEqual(len(report["cases"]), 1)
        self.assertEqual(self.server.calls, [])

    def test_no_default_suite_uses_manifest_case_order(self):
        self.files["catalog.json"].pop("default_suite")
        self.update("catalog.json", cases=list(reversed(self.files["catalog.json"]["cases"])))
        self.assertEqual([c["id"] for c in self.plan()["cases"]], ["CASE-003", "CASE-002", "CASE-001"])

    def test_invalid_selection_is_not_silent_or_successful(self):
        for args in (("--suite", "missing"), ("--case", "missing"), ("--case", "CASE-002", "--case", "CASE-002"), ("--case", "CASE-003")):
            with self.subTest(args=args):
                self.invoke("plan", self.package, *args, code=2)
                self.validate(*args, code=2)
        self.invoke("plan", self.package, "--suite", "smoke", "--case", "CASE-001", code=2)

    def test_disabled_only_package_is_valid_but_not_runnable(self):
        self.update("cases/chain.json", disabled=True)
        self.update("cases/isolation.json", disabled=True)
        self.validate()
        self.invoke("plan", self.package, code=2)
        result = subprocess.run(self.command(), capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 2)
        self.assertEqual(result.stdout, "")
        self.assertEqual(self.server.calls, [])

    def test_invalid_unselected_case_prevents_partial_run(self):
        self.update("cases/disabled.json", unsupported_field=True)
        self.validate("--case", "CASE-002", code=2)
        result = subprocess.run(self.command("--case", "CASE-001"), capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 2)
        self.assertEqual(result.stdout, "")
        self.assertFalse((self.root / "result.json").exists())
        self.assertEqual(self.server.calls, [])

    def test_duplicate_case_ids_and_suite_references_rejected(self):
        self.update("cases/isolation.json", id="CASE-001")
        self.assertIn("case.id", [e["code"] for e in self.validate(code=2)["diagnostics"]])
        self.update("cases/isolation.json", id="CASE-002")
        for ids in (["CASE-001", "CASE-001"], ["missing"], ["smoke"], []):
            self.update("suites/smoke.json", cases=ids)
            self.validate(code=2)

    def test_unknown_versions_and_engine_capabilities_rejected(self):
        original = copy.deepcopy(self.files)
        changes = [("catalog.json", {"schema_version": "reqon-package/v2"}), ("cases/chain.json", {"schema_version": "reqon-case/v2"}), ("suites/smoke.json", {"schema_version": "reqon-suite/v2"}), ("catalog.json", {"default_suite": "missing"}), ("catalog.json", {"requires": {"engine": "reqon", "version": ">=999", "capabilities": []}}), ("catalog.json", {"requires": {"engine": "legacy-java", "version": "*", "capabilities": []}}), ("catalog.json", {"requires": {"engine": "reqon", "version": "*", "capabilities": ["kafka"]}}), ("catalog.json", {"requires": {"engine": "reqon", "version": "*", "capabilities": ["grpc.unary"]}})]
        for name, fields in changes:
            with self.subTest(name=name, fields=fields):
                self.update(name, **fields)
                self.validate(code=2)
                self.files[name] = copy.deepcopy(original[name])
                write_json(self.package / name, self.files[name])

    def test_unsafe_or_nonportable_file_paths_rejected(self):
        original = list(self.files["catalog.json"]["cases"])
        for path in ("../external.json", "/absolute.json", "C:/outside.json", "cases\\chain.json", "cases/./chain.json", "cases//chain.json", "cases/CON.json", "cases/chain.yaml", "cases/chain.json "):
            with self.subTest(path=path):
                self.update("catalog.json", cases=[path, *original[1:]])
                errors = self.validate(code=2)["diagnostics"]
                self.assertIn("package.path", [e["code"] for e in errors])

    def test_duplicate_and_missing_file_references_rejected(self):
        original = self.files["catalog.json"]["cases"]
        for paths in ([*original, original[0]], [*original, "CASES/CHAIN.JSON"], [*original, "missing.json"]):
            self.update("catalog.json", cases=paths)
            self.validate(code=2)

    def test_symlink_cannot_escape_package(self):
        outside = self.root / "outside.json"
        write_json(outside, self.files["cases/chain.json"])
        linked = self.package / "linked.json"
        try:
            os.symlink(outside, linked)
        except OSError as error:
            self.skipTest(f"Symlink creation unavailable: {error}")
        self.update("catalog.json", cases=["linked.json", *self.files["catalog.json"]["cases"][1:]])
        self.assertIn("package.path", [e["code"] for e in self.validate(code=2)["diagnostics"]])

    def test_input_count_and_total_byte_limits(self):
        self.update("catalog.json", cases=["cases/chain.json"] * 1001)
        self.assertIn("package.files", [e["code"] for e in self.validate(code=2)["diagnostics"]])
        cases = []
        for i in range(5):
            name = f"cases/large-{i}.json"
            case = copy.deepcopy(self.files["cases/isolation.json"])
            case.update(id=f"BIG-{i}", description="x" * (7*1024*1024))
            write_json(self.package / name, case)
            cases.append(name)
        self.update("catalog.json", cases=cases, suites=[], default_suite=None)
        self.assertIn("package.size", [e["code"] for e in self.validate(code=2)["diagnostics"]])

    def test_suite_reference_count_is_bounded(self):
        self.update("suites/smoke.json", cases=["CASE-001"] * 1001)
        diagnostics = self.validate(code=2)["diagnostics"]
        self.assertEqual([e["code"] for e in diagnostics], ["suite.empty"])

    def test_failure_continues_independent_cases_and_preserves_exit_code(self):
        self.update("cases/chain.json", steps=[{"type": "grpc", "id": "negative", "method": "poc.Workflow/BusinessError", "expect": [{"path": "/serverCallInfo/code", "equals": "OK"}]}])
        report, junit, _ = self.run_package(code=1)
        self.assertEqual([c["outcome"] for c in report["cases"]], ["failed", "passed", "skipped"])
        self.assertEqual(junit.find("testsuite").attrib["failures"], "1")

    def test_infrastructure_error_has_precedence_over_failed_check(self):
        self.update("cases/chain.json", steps=[{"type": "grpc", "id": "unavailable", "method": "poc.Workflow/TransportError"}])
        self.update("cases/isolation.json", steps=[{"type": "javascript", "id": "failure", "script": "assert(false); return {};"}])
        report, _, _ = self.run_package(code=2)
        self.assertEqual([c["outcome"] for c in report["cases"][:2]], ["error", "failed"])
        self.assertEqual(report["outcome"], "error")

    def test_package_timeout_cancels_inflight_rpc_and_skips_remainder(self):
        self.update("cases/chain.json", steps=[{"type": "grpc", "id": "slow", "method": "poc.Workflow/Slow", "timeout_ms": 9000}])
        self.update("catalog.json", timeout_ms=150)
        report, _, _ = self.run_package(code=2)
        self.assertEqual(report["error"], "package deadline exceeded")
        self.assertEqual([c["outcome"] for c in report["cases"]], ["error", "skipped", "skipped"])
        self.assertTrue(self.server.slow_cancelled.wait(2))

    def test_package_cancel_writes_partial_reports(self):
        self.update("cases/chain.json", steps=[{"type": "grpc", "id": "slow", "method": "poc.Workflow/Slow", "timeout_ms": 9000}])
        process = subprocess.Popen(self.command("--control-stdin"), stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8")
        try:
            self.assertTrue(self.server.slow_started.wait(5))
            stdout, stderr = process.communicate('{"type":"cancel"}\n', timeout=5)
            self.assertEqual(process.returncode, 130, stdout + stderr)
            report = json.loads((self.root / "result.json").read_text())
            self.assertEqual(report["outcome"], "cancelled")
            self.assertEqual(report["cases"][1]["outcome"], "skipped")
            self.assertTrue(self.server.slow_cancelled.wait(2))
            self.assertEqual(ET.parse(self.root / "junit.xml").find("testsuite").attrib["errors"], "1")
        finally:
            if process.poll() is None:
                process.kill()
                process.communicate()

    def test_runtime_endpoint_is_required_and_not_stored_in_plan(self):
        self.invoke("run", self.package, "--json", self.root / "result.json", "--junit", self.root / "junit.xml", code=2)
        report, _, _ = self.run_package("--case", "CASE-002")
        self.assertNotIn(self.server.endpoint, json.dumps(report["plan"]))
        self.assertEqual(self.server.calls, [])

    def test_reports_cannot_overwrite_source_or_share_a_destination(self):
        source = self.package / "cases/chain.json"
        before = source.read_bytes()
        alias = self.root / "alias.json"
        os.link(source, alias)
        for target in (source, source.parent / ".." / "cases" / "chain.json", alias):
            self.invoke("run", self.package, "--endpoint", self.server.endpoint, "--json", target, "--junit", self.root / "junit.xml", code=2)
        self.invoke("run", self.package, "--endpoint", self.server.endpoint, "--json", self.root / "same", "--junit", self.root / "same", code=2)
        self.assertEqual(source.read_bytes(), before)
        self.assertEqual(self.server.calls, [])

    def test_schemas_match_exported_definitions(self):
        for kind, name in (("package", "catalog.json"), ("case", "cases/chain.json"), ("suite", "suites/smoke.json")):
            schema = json.loads(self.invoke("schema", "--kind", kind).stdout)
            jsonschema.Draft202012Validator.check_schema(schema)
            validator = jsonschema.Draft202012Validator(schema)
            validator.validate(self.files[name])
            self.assertFalse(validator.is_valid({**self.files[name], "schema_version": "future/v2"}))
            self.assertFalse(validator.is_valid({**self.files[name], "unknown": True}))
            snapshot = json.loads((ROOT / "schemas" / f"reqon-{kind}-v1.schema.json").read_text())
            self.assertEqual(schema, snapshot)

    def test_duplicate_json_keys_in_case_rejected(self):
        path = self.package / "cases/chain.json"
        path.write_text(path.read_text().replace('"id": "CASE-001"', '"id": "CASE-001", "id": "CASE-002"'))
        error = self.validate(code=2)["diagnostics"][0]
        self.assertEqual(error["file"], "cases/chain.json")
        self.assertIn("duplicate", error["message"])


if __name__ == "__main__":
    unittest.main(verbosity=2)
