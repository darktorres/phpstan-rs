import importlib.util
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("extract_strlen_llvm_witness.py")
SPEC = importlib.util.spec_from_file_location("extract_strlen_llvm_witness", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
EXTRACTOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(EXTRACTOR)


class OperationInventoryTests(unittest.TestCase):
    def test_inventories_every_instruction_family_and_call_target(self):
        function = """define void @f() {
entry:
  %p = alloca ptr
  store ptr null, ptr %p
  %v = load ptr, ptr %p
  %q = getelementptr i8, ptr %v, i64 1
  %n = add i32 1, 2
  %c = icmp eq i32 %n, 3
  %z = zext i1 %c to i32
  %x = xor i32 %z, 1
  %s = sext i32 %x to i64
  %t = trunc i64 %s to i8
  br label %done
done:
  %r = phi i8 [ %t, %entry ]
  call void @known()
  call void %callback()
  ret void
}"""

        inventory = EXTRACTOR.operation_inventory(function)

        self.assertEqual(inventory["instruction_count"], 15)
        self.assertEqual(inventory["direct_call_targets"], ["known"])
        self.assertEqual(inventory["indirect_call_count"], 1)
        self.assertEqual(
            set(inventory["operation_group_counts"]),
            {"native_storage", "layout_access", "scalar_transfer", "calls", "control_flow", "exit"},
        )

    def test_unknown_instruction_fails_closed(self):
        with self.assertRaisesRegex(SystemExit, "unaccounted LLVM instruction.*freeze"):
            EXTRACTOR.operation_inventory("define void @f() {\n  %x = freeze i32 1\n  ret void\n}")

    def test_missing_return_fails_closed(self):
        with self.assertRaisesRegex(SystemExit, "lacks its single return"):
            EXTRACTOR.operation_inventory("define void @f() {\n  br label %x\n}")


if __name__ == "__main__":
    unittest.main()
