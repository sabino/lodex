import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from hardware_job import HardwareJob


class HardwareJobTests(unittest.TestCase):
    def test_excludes_other_process_and_releases_after_exception(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "job.lock"
            script = ("from hardware_job import HardwareJob; "
                      "import sys; "
                      "lock=HardwareJob(sys.argv[1]); lock.__enter__()")
            command = [sys.executable, "-c", script, str(path)]
            env = {**os.environ, "PYTHONPATH": str(Path(__file__).resolve().parents[1])}
            with self.assertRaisesRegex(ValueError, "fixture failure"):
                with HardwareJob(path):
                    result = subprocess.run(command, env=env, capture_output=True)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(b"another hardware job", result.stderr)
                    raise ValueError("fixture failure")
            self.assertEqual(subprocess.run(command, env=env, capture_output=True).returncode, 0)

    def test_rejects_symlink_lock(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "target"
            target.write_text("untouched")
            link = Path(directory) / "link"
            link.symlink_to(target)
            with self.assertRaises(OSError):
                with HardwareJob(link):
                    self.fail("followed symlink")
            self.assertEqual(target.read_text(), "untouched")

    def test_process_death_releases_kernel_lock(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "job.lock"
            env = {**os.environ, "PYTHONPATH": str(Path(__file__).resolve().parents[1])}
            command = [sys.executable, "-c", "from hardware_job import HardwareJob; import sys; "
                       "lock=HardwareJob(sys.argv[1]); lock.__enter__(); "
                       "print('ready', flush=True); sys.stdin.read()", str(path)]
            child = subprocess.Popen(command, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE)
            try:
                self.assertEqual(child.stdout.readline(), b"ready\n")
                with self.assertRaisesRegex(RuntimeError, "another hardware job"):
                    with HardwareJob(path):
                        self.fail("concurrent entry")
                child.kill()
                child.wait(timeout=5)
                with HardwareJob(path):
                    pass
            finally:
                if child.poll() is None:
                    child.kill()
                    child.wait(timeout=5)
                child.stdin.close()
                child.stdout.close()


if __name__ == "__main__":
    unittest.main()
