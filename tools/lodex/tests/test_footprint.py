import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("footprint", Path(__file__).parents[1] / "footprint.py")
footprint = importlib.util.module_from_spec(spec)
spec.loader.exec_module(footprint)


def sample(timestamp=100, start=80, pid=7, major=3):
    fields = ["S"] + ["0"] * 21
    fields[7], fields[9], fields[11], fields[12], fields[19] = "9", str(major), "12", "5", str(start)
    return (f"sample {timestamp}\nstat {pid} (name with ) spaces) {' '.join(fields)}\n"
            "status VmRSS: 40\nstatus VmHWM: 60\nstatus VmSwap: 8\nstatus Threads: 2\nend\n")


class FootprintTests(unittest.TestCase):
    def test_proc_comm_parentheses_and_counter_offsets(self):
        rows = footprint.parse_samples(sample() + sample(timestamp=102, major=6))
        self.assertEqual(rows[0]["cpu_ticks"], 17)
        self.assertEqual(footprint.summarize(rows)["major_fault_delta"], 3)
        self.assertEqual(footprint.summarize(rows)["elapsed_seconds"], 2)

    def test_pid_reuse_rejected(self):
        with self.assertRaisesRegex(ValueError, "identity"):
            footprint.parse_samples(sample() + sample(start=81))

    def test_different_pid_rejected(self):
        with self.assertRaisesRegex(ValueError, "identity"):
            footprint.parse_samples(sample() + sample(pid=8))

    def test_partial_capture_rejected(self):
        with self.assertRaises(ValueError):
            footprint.parse_samples(sample().replace("end\n", ""))

    def test_missing_fields_rejected(self):
        with self.assertRaises(ValueError):
            footprint.parse_samples(sample().replace("status VmSwap: 8\n", ""))

    def test_clock_regression_rejected(self):
        with self.assertRaises(ValueError):
            footprint.parse_samples(sample() + sample(timestamp=99))


if __name__ == "__main__":
    unittest.main()
