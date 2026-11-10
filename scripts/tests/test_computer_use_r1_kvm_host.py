import importlib.util
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "check-computer-use-r1-kvm-host.py"
SPEC = importlib.util.spec_from_file_location("computer_use_r1_kvm_host", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
HOST = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(HOST)


class ComputerUseR1KvmHostTests(unittest.TestCase):
    def create_cgroup(self, root: Path, enabled: str = "cpu memory pids") -> Path:
        cgroup = root / "user.slice" / "runner.slice"
        cgroup.mkdir(parents=True)
        (cgroup / "cgroup.controllers").write_text("cpu memory pids\n", encoding="utf-8")
        (cgroup / "cgroup.subtree_control").write_text(enabled + "\n", encoding="utf-8")
        (cgroup / "cgroup.procs").write_text("", encoding="utf-8")
        return cgroup

    def test_accepts_one_canonical_delegated_cgroup_with_required_controllers(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            cgroup = self.create_cgroup(root)
            proc = root / "proc-cgroup"
            proc.write_text("0::/user.slice/runner.slice\n", encoding="utf-8")
            self.assertEqual(HOST.cgroup_root(proc, root), cgroup)

    def test_rejects_missing_controllers(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.create_cgroup(root, "cpu memory")
            proc = root / "proc-cgroup"
            proc.write_text("0::/user.slice/runner.slice\n", encoding="utf-8")
            with self.assertRaisesRegex(RuntimeError, "not enabled"):
                HOST.cgroup_root(proc, root)

    def test_rejects_ambiguous_and_traversing_cgroup_paths(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            proc = root / "proc-cgroup"
            proc.write_text("0::/../outside\n", encoding="utf-8")
            with self.assertRaisesRegex(RuntimeError, "unsafe component"):
                HOST.cgroup_root(proc, root)
            proc.write_text("0::/one\n0::/two\n", encoding="utf-8")
            with self.assertRaisesRegex(RuntimeError, "exactly one"):
                HOST.cgroup_root(proc, root)

    def test_rejects_missing_process_control_file(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            cgroup = self.create_cgroup(root)
            (cgroup / "cgroup.procs").unlink()
            proc = root / "proc-cgroup"
            proc.write_text("0::/user.slice/runner.slice\n", encoding="utf-8")
            with self.assertRaisesRegex(RuntimeError, "process control"):
                HOST.cgroup_root(proc, root)

    def test_rejects_symlinked_cgroup_root(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.create_cgroup(root)
            (root / "user.slice" / "runner.slice").rename(root / "actual")
            (root / "user.slice" / "runner.slice").symlink_to(root / "actual")
            proc = root / "proc-cgroup"
            proc.write_text("0::/user.slice/runner.slice\n", encoding="utf-8")
            with self.assertRaisesRegex(RuntimeError, "canonical directory"):
                HOST.cgroup_root(proc, root)
            self.assertTrue((root / "actual").exists())


if __name__ == "__main__":
    unittest.main()
