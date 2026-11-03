"""MR-facing CLI contract: offline checks, schema and machine-readable capabilities."""
import json
import subprocess
import tempfile
import unittest
from pathlib import Path

import jsonschema
from fixture import sample
from test_e2e import BINARY, ROOT


class ValidationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="reqon-validation-")
        self.directory = Path(self.temp.name)
        self.path = self.directory / "scenario.json"

    def tearDown(self):
        self.temp.cleanup()

    def write(self, spec):
        self.path.write_text(json.dumps(spec, ensure_ascii=False), encoding="utf-8")

    def invoke(self, *args):
        return subprocess.run([str(BINARY), *map(str, args)], capture_output=True, text=True, encoding="utf-8", timeout=5)

    def validate(self, code=0):
        result = self.invoke("validate", self.path, "--json")
        self.assertEqual(result.returncode, code, result.stdout + result.stderr)
        self.assertEqual(result.stderr, "")
        report = json.loads(result.stdout)
        self.assertEqual(report["type"], "validation_result")
        self.assertEqual(report["schema_version"], "reqon-prototype/validation-v1")
        self.assertEqual(report["valid"], code == 0)
        return report

    def assert_run_rejected(self):
        result = self.invoke("run", self.path, "--json", self.directory / "result.json", "--junit", self.directory / "junit.xml")
        self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
        self.assertEqual(result.stdout, "")
        self.assertEqual(json.loads(result.stderr)["type"], "fatal_error")
        self.assertFalse((self.directory / "result.json").exists())

    def test_valid_offline_without_server_or_script_execution(self):
        spec = sample("http://127.0.0.1:1")
        spec["steps"][-1]["script"] = "throw new Error('must not execute'); while(true) {}"
        self.write(spec)
        report = self.validate()
        self.assertEqual(report["step_count"], 3)
        self.assertEqual(report["diagnostics"], [])
        self.assertEqual(sorted(p.name for p in self.directory.iterdir()), ["scenario.json"])

    def test_human_output(self):
        self.write(sample("http://127.0.0.1:1"))
        result = self.invoke("validate", self.path)
        self.assertEqual(result.returncode, 0)
        self.assertIn("Offline validation only", result.stdout)
        self.assertEqual(result.stderr, "")

    def test_collects_semantic_errors_with_paths(self):
        spec = sample("http://127.0.0.1:1")
        spec["id"] = " "
        spec["steps"][1]["id"] = spec["steps"][0]["id"]
        spec["steps"][1]["poll"]["read_only"] = False
        spec["steps"][0]["expect"][0]["path"] = "/bad~2escape"
        self.write(spec)
        errors = self.validate(2)["diagnostics"]
        self.assertTrue({"scenario.id", "step.id", "poll.configuration", "check.pointer"}.issubset({e["code"] for e in errors}))
        self.assertIn("$.steps[0].expect[0].path", [e["path"] for e in errors])
        self.assert_run_rejected()

    def test_duplicate_keys_rejected_at_root_and_in_payload(self):
        content = json.dumps(sample("http://127.0.0.1:1"))
        for broken in [content.replace('"id": "create-and-check"', '"id": "a", "id": "b"'), content.replace('"name": "example"', '"name": "a", "name": "b"')]:
            with self.subTest(content=broken):
                self.path.write_text(broken, encoding="utf-8")
                errors = self.validate(2)["diagnostics"]
                self.assertIn("duplicate object key", errors[0]["message"])
                self.assert_run_rejected()

    def test_rejects_unknown_fields_and_types_with_shared_loader(self):
        for mutation in (lambda s: s.update(extra=True), lambda s: s.update(timeout_ms="1000"), lambda s: s["steps"][0].update(extra=True)):
            spec = sample("http://127.0.0.1:1")
            mutation(spec)
            self.write(spec)
            error = self.validate(2)["diagnostics"][0]
            self.assertEqual(error["code"], "input.shape")
            self.assertTrue(error["path"].startswith("$"))
            self.assert_run_rejected()

    def test_missing_or_invalid_json(self):
        self.assertEqual(self.validate(2)["diagnostics"][0]["code"], "input.io")
        for raw in ("{", "{} {}", "null", '"string"', "[1,2]", '{"id": NaN}'):
            with self.subTest(raw=raw):
                self.path.write_text(raw, encoding="utf-8")
                self.validate(2)
                self.assert_run_rejected()

    def test_utf8_bom_and_unicode_are_accepted(self):
        spec = sample("http://127.0.0.1:1")
        spec["id"] = "Проверка контракта"
        self.path.write_text(json.dumps(spec, ensure_ascii=False), encoding="utf-8-sig")
        self.assertEqual(self.validate()["scenario_id"], spec["id"])

    def test_input_limit_before_parse(self):
        self.path.write_bytes(b" " * (8 * 1024 * 1024 + 1))
        self.assertEqual(self.validate(2)["diagnostics"][0]["code"], "input.size")
        self.assert_run_rejected()

    def test_version_endpoint_and_metadata_are_checked_offline(self):
        mutations = [
            (lambda s: s.update(schema_version="future/v9"), "scenario.version"),
            (lambda s: s.update(endpoint="ftp://127.0.0.1:1"), "scenario.endpoint"),
            (lambda s: s.update(endpoint="http://127.0.0.1:1/path"), "scenario.endpoint"),
            (lambda s: s.update(metadata={"x-test": "bad\nvalue"}), "scenario.metadata"),
            (lambda s: s.update(metadata={"invalid-bin": "not-binary"}), "scenario.metadata"),
        ]
        for mutation, code in mutations:
            spec = sample("http://127.0.0.1:1")
            mutation(spec)
            self.write(spec)
            self.assertIn(code, [e["code"] for e in self.validate(2)["diagnostics"]])

    def test_json_pointer_escape_validation(self):
        spec = sample("http://127.0.0.1:1")
        spec["steps"][0]["expect"][0]["path"] = "/a~1b/~0c/"
        self.write(spec)
        self.validate()
        spec["steps"][0]["save"] = {"value": "/trailing~"}
        self.write(spec)
        self.assertIn("save.pointer", [e["code"] for e in self.validate(2)["diagnostics"]])

    def test_schema_is_valid_and_matches_published_snapshot(self):
        result = self.invoke("schema")
        self.assertEqual(result.returncode, 0, result.stderr)
        schema = json.loads(result.stdout)
        jsonschema.Draft202012Validator.check_schema(schema)
        validator = jsonschema.Draft202012Validator(schema)
        valid = sample("http://127.0.0.1:1")
        validator.validate(valid)
        invalid = {**valid, "unknown": True}
        self.assertFalse(validator.is_valid(invalid))
        self.assertFalse(validator.is_valid({**valid, "schema_version": "future/v9"}))
        self.assertFalse(validator.is_valid({**valid, "timeout_ms": "1000"}))
        snapshot = json.loads((ROOT / "schemas" / "reqon-prototype-v1.schema.json").read_text(encoding="utf-8"))
        self.assertEqual(schema, snapshot, "Regenerate schema with the schema command")

    def test_capabilities_do_not_advertise_unimplemented_features(self):
        result = self.invoke("capabilities")
        self.assertEqual(result.returncode, 0, result.stderr)
        data = json.loads(result.stdout)
        self.assertEqual(data["schema_version"], "reqon-prototype/capabilities-v1")
        self.assertIn("reqon-prototype/v1", data["scenario_formats"])
        self.assertFalse(data["production_ready"])
        self.assertFalse(data["grpc"]["streaming"])
        self.assertFalse(data["javascript"]["os_sandbox"])
        self.assertIn("http", data["step_types"])
        self.assertTrue(data['grpc']['tls'])
        self.assertEqual(data["exit_codes"]["cancelled"], 130)
        for command in ("validate", "schema", "capabilities"):
            self.assertEqual(self.invoke(command, "--help").returncode, 0)


if __name__ == "__main__":
    unittest.main(verbosity=2)
