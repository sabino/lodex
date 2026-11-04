import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).parents[1]
spec = importlib.util.spec_from_file_location("footprint", ROOT / "footprint.py")
footprint = importlib.util.module_from_spec(spec)
spec.loader.exec_module(footprint)


@unittest.skipUnless(shutil.which("cc") and Path("/proc/self/stat").exists(), "Linux C compiler required")
class ProcSamplerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory()
        cls.binary = Path(cls.directory.name) / "proc-sampler"
        subprocess.run(["cc", "-std=c99", "-O2", "-Wall", "-Wextra", "-Werror",
                        str(ROOT / "proc-sampler.c"), "-o", str(cls.binary)], check=True)

    @classmethod
    def tearDownClass(cls):
        cls.directory.cleanup()

    def test_live_capture_is_accepted_by_existing_parser(self):
        result = subprocess.run([self.binary, str(os.getpid()), "3", "10"],
                                capture_output=True, text=True, check=True, timeout=5)
        samples = footprint.parse_samples(result.stdout)
        self.assertEqual(len(samples), 3)
        self.assertTrue(all(row["pid"] == os.getpid() for row in samples))
        self.assertGreater(samples[-1]["rss_kib"], 0)

    def test_invalid_bounds_and_missing_process_fail(self):
        for args in [("0", "1", "10"), ("1", "0", "10"), ("1", "1", "1"),
                     ("1", "1", "60001"), ("-1", "1", "10"),
                     ("2147483647", "1", "10")]:
            result = subprocess.run([self.binary, *args], capture_output=True, timeout=5)
            self.assertNotEqual(result.returncode, 0, args)
            self.assertFalse(result.stdout, args)

    def test_capture_stops_cleanly_when_process_exits(self):
        child = subprocess.Popen(["sleep", "0.08"])
        try:
            result = subprocess.run([self.binary, str(child.pid), "100", "10"],
                                    capture_output=True, text=True, timeout=5)
            self.assertEqual(result.returncode, 0)
            self.assertGreater(len(footprint.parse_samples(result.stdout)), 0)
        finally:
            child.wait(timeout=5)


if __name__ == "__main__":
    unittest.main()
