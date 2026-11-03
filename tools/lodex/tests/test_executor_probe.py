import base64
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from executor_probe import Executor


def page(data=b"", seq=1, *, closed=False, exited=False, code=None):
    return {"chunks": ([{"seq": seq, "stream": "stdout", "chunk": base64.b64encode(data).decode()}]
                       if data else []),
            "nextSeq": seq + 1, "exited": exited, "exitCode": code,
            "closed": closed, "failure": None, "sandboxDenied": False}


class ExecutorProbeTests(unittest.IsolatedAsyncioTestCase):
    def executor(self, pages):
        executor = Executor(None, "/synthetic fixture", 1)
        calls = []

        async def call(method, params):
            calls.append((method, params))
            return pages.pop(0)

        executor.call = call
        executor.live.add("fixture")
        return executor, calls

    async def test_drains_closed_paged_output_after_exit(self):
        executor, calls = self.executor([
            page(b"first", seq=1, closed=True, exited=True, code=0),
            page(b"last", seq=2, closed=True, exited=True, code=0),
            page(closed=True, exited=True, code=0),
        ])
        streams, _ = await executor.collect("fixture")
        self.assertEqual(streams["stdout"], b"firstlast")
        self.assertEqual([params["afterSeq"] for _, params in calls], [0, 1, 2])
        self.assertNotIn("fixture", executor.live)

    async def test_rejects_replayed_sequence_instead_of_duplicating_output(self):
        executor, _ = self.executor([page(b"first", seq=1), page(b"again", seq=1)])
        with self.assertRaisesRegex(AssertionError, "sequence"):
            await executor.collect("fixture")

    async def test_terminal_exit_does_not_mean_output_is_drained(self):
        executor, _ = self.executor([
            page(exited=True, code=0), page(b"late", seq=1, exited=True, closed=True, code=0),
            page(exited=True, closed=True, code=0),
        ])
        streams, _ = await executor.collect("fixture")
        self.assertEqual(streams["stdout"], b"late")

    async def test_output_limit_and_wrong_exit_fail(self):
        executor, _ = self.executor([page(b"x" * (1024 * 1024 + 1))])
        with self.assertRaisesRegex(AssertionError, "one MiB"):
            await executor.collect("fixture")
        executor, _ = self.executor([page(exited=True, closed=True, code=137)])
        with self.assertRaisesRegex(AssertionError, "expected exit 0, got 137"):
            await executor.collect("fixture")


if __name__ == "__main__":
    unittest.main()
