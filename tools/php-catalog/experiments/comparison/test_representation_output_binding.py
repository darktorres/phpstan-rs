import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("build_strlen_representation_correspondence.py")
SPEC = importlib.util.spec_from_file_location("strlen_correspondence", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
BUILDER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BUILDER)
RECORDER_SCRIPT = Path(__file__).with_name("record_representation_outputs.py")
RECORDER_SPEC = importlib.util.spec_from_file_location("record_representation_outputs", RECORDER_SCRIPT)
assert RECORDER_SPEC is not None and RECORDER_SPEC.loader is not None
RECORDER = importlib.util.module_from_spec(RECORDER_SPEC)
RECORDER_SPEC.loader.exec_module(RECORDER)


class RepresentationOutputBindingTests(unittest.TestCase):
    def test_recorder_binds_stdout_and_generated_artifact_bytes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            capture = root / "capture"
            capture.mkdir()
            (capture / "producer.out").write_bytes(b"captured compiler summary\n")
            artifact = root / "generated.dump"
            artifact.write_bytes(b"compiler sidecar\n")
            archive = root / "archive" / "generated.dump"
            measurements = root / "measurements.json"
            measurements.write_text(json.dumps([{"name": "producer", "exit_code": 0}]))

            RECORDER.record_outputs(measurements, capture, [("producer", artifact, archive)])

            [record] = json.loads(measurements.read_text())
            self.assertEqual(record["stdout_capture_path"], str((capture / "producer.out").resolve()))
            self.assertEqual(record["stdout_archive_path"], str((archive.parent / "producer.out").resolve()))
            self.assertEqual(record["stdout_sha256"], BUILDER.sha(capture / "producer.out"))
            self.assertEqual(record["output_artifacts"], [{
                "path": str(artifact.resolve()), "archive_path": str(archive.resolve()),
                "sha256": BUILDER.sha(archive),
            }])
            self.assertEqual(
                BUILDER.validate_measurement_output([record], "producer", archive.parent / "producer.out"),
                BUILDER.sha(capture / "producer.out"),
            )
            (archive.parent / "producer.out").write_bytes(b"tampered archive\n")
            with self.assertRaisesRegex(ValueError, "archived stdout digest differs"):
                BUILDER.validate_measurement_output([record], "producer", archive.parent / "producer.out")

    def test_recorder_rejects_failed_or_ambiguous_producer_runs(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            measurements = root / "measurements.json"
            artifact = root / "summary.json"
            artifact.write_text("{}\n")
            capture = root / "capture"
            capture.mkdir()
            (capture / "producer.out").write_text("producer output\n")
            measurements.write_text(json.dumps([
                {"name": "producer", "exit_code": 1},
                {"name": "producer", "exit_code": 0},
            ]))
            with self.assertRaisesRegex(ValueError, "exactly one successful"):
                RECORDER.record_outputs(measurements, capture, [("producer", artifact, root / "archive.json")])

    def test_every_bound_output_requires_exact_path_and_digest(self):
        runs = ["ast", "llvm", "gcc", "cleanup_ast", "cleanup_gimple", "cleanup_llvm"]
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            records = []
            paths = {}
            for run in runs:
                path = root / f"{run}.json"
                path.write_text(f'{{"run":"{run}"}}\n')
                paths[run] = path
                records.append({
                    "name": run,
                    "exit_code": 0,
                    "stdout_path": str(path.resolve()),
                    "stdout_sha256": BUILDER.sha(path),
                })
            for run in runs:
                self.assertEqual(
                    BUILDER.validate_measurement_output(records, run, paths[run]),
                    BUILDER.sha(paths[run]),
                )
                original = paths[run].read_bytes()
                paths[run].write_bytes(original + b"changed")
                with self.assertRaisesRegex(ValueError, "digest differs"):
                    BUILDER.validate_measurement_output(records, run, paths[run])
                paths[run].write_bytes(original)
                wrong_path = root / f"wrong-{run}.json"
                wrong_path.write_bytes(original)
                with self.assertRaisesRegex(ValueError, "does not bind"):
                    BUILDER.validate_measurement_output(records, run, wrong_path)

    def test_gcc_sidecar_must_match_compiler_dump_path_and_digest(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "Zend/zend_builtin_functions.c"
            source.parent.mkdir()
            source.write_text("source")
            output = root / "php-type.o"
            expected = root / "php-type.c.007t.gimple"
            expected.write_text("pinned dump")
            command = ["gcc", "-fdump-tree-gimple", "-c", str(source), "-o", str(output)]
            record = {
                "name": "php-type-gimple",
                "exit_code": 0,
                "command": command,
                "output_artifacts": [{
                    "path": str(expected.resolve()),
                    "archive_path": str(expected.resolve()),
                    "sha256": BUILDER.sha(expected),
                }],
            }
            self.assertEqual(
                BUILDER.validate_measurement_output([record], "php-type-gimple", expected),
                BUILDER.sha(expected),
            )
            forged = root / "forged.007t.gimple"
            forged.write_text(expected.read_text())
            record["output_artifacts"][0]["path"] = str(forged.resolve())
            record["output_artifacts"][0]["archive_path"] = str(forged.resolve())
            with self.assertRaisesRegex(ValueError, "sidecar path is not the compiler output"):
                BUILDER.validate_measurement_output([record], "php-type-gimple", forged)


if __name__ == "__main__":
    unittest.main()
