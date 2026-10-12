import argparse
import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
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


REGENERATOR_SPEC = importlib.util.spec_from_file_location(
    "regenerate_correspondence", Path(__file__).with_name("regenerate_correspondence.py"))
assert REGENERATOR_SPEC is not None and REGENERATOR_SPEC.loader is not None
REGENERATOR = importlib.util.module_from_spec(REGENERATOR_SPEC)
REGENERATOR_SPEC.loader.exec_module(REGENERATOR)
LOCK = json.loads((HERE / "results" / "lock.json").read_text())
LOCKED_SOURCE = Path(os.environ.get(
    "PHP_CATALOG_SOURCE",
    Path.home() / ".cache/php-catalog-evaluation/php-src-gcc/Zend/zend_builtin_functions.c"))
CHECKED_IN = HERE / "results" / "evaluation" / "strlen-representation-correspondence.json"


def correspondence_arguments(source, **overrides):
    values = {name: path for name, path in REGENERATOR.EVIDENCE.items()}
    values.update(source=source, output=Path("unused.json"), lock=HERE / "results" / "lock.json",
                  recorded_root=None)
    values.update(overrides)
    return argparse.Namespace(**values)


class RecordedRootTests(unittest.TestCase):
    def test_recorded_paths_map_to_the_same_relative_file_under_any_root(self):
        relative = "php-src/Zend/zend_builtin_functions.c"
        self.assertEqual(
            BUILDER.root_relative("/recorded/root/" + relative, "/recorded/root"),
            relative)
        self.assertEqual(
            BUILDER.root_relative("/srv/moved/root/" + relative, "/srv/moved/root"), relative)
        self.assertEqual(
            BUILDER.root_relative("/srv/moved/root/php-src/./Zend//zend_builtin_functions.c",
                                  "/srv/moved/root/"), relative)
        self.assertIsNone(BUILDER.root_relative("/usr/bin/cc", "/recorded/root"))

    def test_a_path_that_leaves_its_root_after_normalisation_is_refused(self):
        with self.assertRaisesRegex(ValueError, "escapes its root"):
            BUILDER.root_relative("/recorded/root/../secret.c", "/recorded/root")

    def test_a_wrong_recorded_root_fails_the_command_checks(self):
        with self.assertRaisesRegex(SystemExit, "does not name the pinned PHP source"):
            BUILDER.build_report(
                correspondence_arguments(LOCKED_SOURCE, recorded_root="/srv/elsewhere"), LOCK)


class SwappedArchiveTests(unittest.TestCase):
    def test_two_archives_with_swapped_contents_are_each_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            ast = root / "php-type-ast-0.out"
            llvm = root / "php-type-llvm-read-0.out"
            ast.write_bytes(b"clang ast summary\n")
            llvm.write_bytes(b"clang llvm summary\n")
            records = [
                {"name": "php-type-ast-0", "exit_code": 0,
                 "stdout_path": str(ast.resolve()), "stdout_sha256": BUILDER.sha(ast)},
                {"name": "php-type-llvm-read-0", "exit_code": 0,
                 "stdout_path": str(llvm.resolve()), "stdout_sha256": BUILDER.sha(llvm)},
            ]
            ast.write_bytes(b"clang llvm summary\n")
            llvm.write_bytes(b"clang ast summary\n")
            for name, path in (("php-type-ast-0", ast), ("php-type-llvm-read-0", llvm)):
                with self.assertRaisesRegex(ValueError, "digest differs"):
                    BUILDER.validate_measurement_output(records, name, path)

    def test_an_archive_cannot_stand_in_for_another_recorded_output(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            ast = root / "php-type-ast-0.out"
            llvm = root / "php-type-llvm-read-0.out"
            ast.write_bytes(b"same bytes\n")
            llvm.write_bytes(b"same bytes\n")
            records = [
                {"name": "php-type-ast-0", "exit_code": 0,
                 "stdout_path": str(ast.resolve()), "stdout_sha256": BUILDER.sha(ast)},
            ]
            with self.assertRaisesRegex(ValueError, "does not bind"):
                BUILDER.validate_measurement_output(records, "php-type-ast-0", llvm)


class InputPinTests(unittest.TestCase):
    def test_an_input_that_differs_from_its_locked_digest_is_refused_before_parsing(self):
        with tempfile.TemporaryDirectory() as temporary:
            tampered = Path(temporary) / "php-type-ast-0.out"
            tampered.write_bytes(REGENERATOR.EVIDENCE["clang_ast"].read_bytes() + b"\n")
            args = correspondence_arguments(LOCKED_SOURCE, clang_ast=tampered)
            with self.assertRaisesRegex(SystemExit, "clang_ast differs from its locked digest"):
                BUILDER.verify_inputs(args, LOCK)

    def test_every_input_has_a_pin_and_the_regenerator_names_each_one(self):
        self.assertEqual(
            set(LOCK["representation_correspondence_inputs"]),
            set(REGENERATOR.EVIDENCE) | {"source"})
        for name, path in REGENERATOR.EVIDENCE.items():
            if name != "source":
                self.assertTrue(path.is_file(), path)


@unittest.skipUnless(LOCKED_SOURCE.is_file(),
                     "the locked PHP 8.5.4 zend_builtin_functions.c is not available here")
class RegenerationTests(unittest.TestCase):
    def test_regenerated_correspondence_is_byte_identical_to_the_checked_in_artifact(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "strlen-representation-correspondence.json"
            subprocess.run(
                [sys.executable, str(HERE / "regenerate_correspondence.py"),
                 "--source", str(LOCKED_SOURCE), "--output", str(output)],
                check=True)
            self.assertEqual(output.read_bytes(), CHECKED_IN.read_bytes())


if __name__ == "__main__":
    unittest.main()
