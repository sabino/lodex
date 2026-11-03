#!/usr/bin/env python3
"""Qualify a native executor over pinned SSH stdio using only synthetic data.

No model, auth store, microphone or production home is used. Results contain
counts, hashes and timings, never process output or executor metadata.
"""
import argparse
import asyncio
from contextlib import asynccontextmanager
import base64
import hashlib
import json
from pathlib import Path
import shlex
import sys
import time
from urllib.parse import quote

from hardware_job import DEFAULT_LOCK, HardwareJob
from native_benchmark import remote
from rpc_probe import Probe


def encoded(data):
    return base64.b64encode(data).decode("ascii")


def digest(data):
    return {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


class Executor:
    def __init__(self, process, cwd, timeout):
        self.probe = Probe(process, timeout)
        self.cwd = "file://" + quote(cwd, safe="/")
        self.timeout = timeout
        self.methods = {}
        self.live = set()

    async def call(self, method, params):
        started = time.monotonic()
        response = await self.probe.request(method, params)
        self.methods.setdefault(method, []).append(time.monotonic() - started)
        if "error" in response:
            # Avoid including remote output or arbitrary error text in reports.
            raise AssertionError(f"{method} returned protocol error {response['error']['code']}")
        return response["result"]

    async def initialize(self, resume_session_id=None):
        response = await self.call("initialize", {"clientName": "lodex-synthetic-executor-probe",
                                                  "resumeSessionId": resume_session_id})
        if not response.get("sessionId"):
            raise AssertionError("missing executor session")
        await self.probe.send({"method": "initialized", "params": {}})
        status = await self.call("environment/status", {})
        if status["status"] != "ready":
            raise AssertionError("executor not ready")
        return response["sessionId"]

    async def start(self, name, script, pipe_stdin=False):
        result = await self.call("process/start", {
            "processId": name, "argv": ["/bin/sh", "-c", script],
            "cwd": self.cwd, "env": {}, "tty": False, "pipeStdin": pipe_stdin,
            "arg0": None, "sandbox": None,
        })
        if result["processId"] != name:
            raise AssertionError("process handle mismatch")
        self.live.add(name)

    async def collect(self, name, expected_code=0, until_stdout=None):
        streams = {"stdout": bytearray(), "stderr": bytearray()}
        after = 0
        async with asyncio.timeout(self.timeout):
            while True:
                response = await self.call("process/read", {
                    "processId": name, "afterSeq": after, "maxBytes": 32768, "waitMs": 1000,
                })
                if response.get("failure") or response.get("sandboxDenied"):
                    raise AssertionError("process read failed")
                for chunk in response["chunks"]:
                    if chunk["seq"] <= after or chunk["stream"] not in streams:
                        raise AssertionError("invalid process stream/sequence")
                    after = chunk["seq"]
                    streams[chunk["stream"]].extend(base64.b64decode(chunk["chunk"], validate=True))
                    if sum(map(len, streams.values())) > 1024 * 1024:
                        raise AssertionError("fixture output exceeded one MiB")
                if until_stdout is not None and bytes(streams["stdout"]) == until_stdout:
                    return streams, response
                # exited can arrive before pipes finish draining; closed alone
                # can still accompany a truncated maxBytes page. Drain all pages.
                if response["closed"] and not response["chunks"]:
                    if not response["exited"] or response["exitCode"] != expected_code:
                        raise AssertionError(f"{name}: expected exit {expected_code}, got {response['exitCode']}")
                    self.live.discard(name)
                    return streams, response


@asynccontextmanager
async def connection(args, root):
    q = shlex.quote
    prefix = "exec env -u OPENAI_API_KEY -u CODEX_API_KEY -u CODEX_ACCESS_TOKEN "
    launch = (f"set -eu\ncd {q(root)}\nprintf '%s' $$ > executor.pid\n" + prefix +
              f"CODEX_HOME={q(root + '/home')} TMPDIR={q(root + '/tmp')} "
              f"LD_LIBRARY_PATH={q(args.runtime_lib)} {q(args.binary)} " +
              ("exec-server " if args.full_cli else "") + "--listen stdio")
    process = await asyncio.create_subprocess_exec(
        str(args.ssh), launch, stdin=asyncio.subprocess.PIPE,
        stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE, limit=2 ** 20)

    async def discard_errors():
        while await process.stderr.read(4096):
            pass

    drain = asyncio.create_task(discard_errors())
    executor = Executor(process, root, args.timeout)
    try:
        await executor.initialize()
        yield executor
    finally:
        # Best-effort cancellation only of synthetic handles started here.
        for name in sorted(executor.live):
            try:
                await asyncio.wait_for(executor.call("process/terminate", {"processId": name}), 5)
            except (TimeoutError, RuntimeError, AssertionError, BrokenPipeError, ConnectionError):
                pass
        process.stdin.close()
        try:
            await asyncio.wait_for(process.wait(), 20)
        except TimeoutError:
            process.terminate()
            try:
                await asyncio.wait_for(process.wait(), 5)
            except TimeoutError:
                process.kill()
                await process.wait()
        await drain
    if process.returncode != 0:
        raise AssertionError(f"executor transport exited {process.returncode}")


async def qualify(args):
    args.output.parent.mkdir(parents=True, exist_ok=True)
    # Create-only remote root and evidence file prevent accidental reuse.
    with args.output.open("x") as report_file:
        q = shlex.quote
        await remote(args.ssh, f"set -eu\numask 077\nmkdir {q(args.remote_root)}\n"
                     f"mkdir {q(args.remote_root + '/home')} {q(args.remote_root + '/tmp')}")
        checks = []
        started = time.monotonic()
        async with connection(args, args.remote_root) as executor:
            await executor.start("streams", "printf 'lodex-stdout\\n'; printf 'lodex-stderr\\n' >&2; exit 7")
            streams, _ = await executor.collect("streams", expected_code=7)
            if streams != {"stdout": b"lodex-stdout\n", "stderr": b"lodex-stderr\n"}:
                raise AssertionError("stdout/stderr byte mismatch")
            checks.append({"case": "separate-streams-nonzero-exit", "passed": True})

            await executor.start("long-output", "dd if=/dev/zero bs=4096 count=64 2>/dev/null; printf 'end\\n' >&2")
            streams, _ = await executor.collect("long-output")
            if streams != {"stdout": bytes(262144), "stderr": b"end\n"}:
                raise AssertionError("large output byte mismatch")
            checks.append({"case": "paged-256-kib-output", "passed": True, **digest(streams["stdout"])})

            await executor.start("stdin", 'IFS= read -r a; printf "%s\\n" "$a"; IFS= read -r b; printf "%s\\n" "$b"', True)
            for name, data in [("first", b"alpha\n"), ("first", b"alpha\n"), ("second", b"beta\n")]:
                result = await executor.call("process/write", {
                    "processId": "stdin", "chunk": encoded(data), "writeId": name})
                if result["status"] != "accepted":
                    raise AssertionError("stdin write not accepted")
            streams, _ = await executor.collect("stdin")
            if streams != {"stdout": b"alpha\nbeta\n", "stderr": b""}:
                raise AssertionError("stdin retry duplicated or lost bytes")
            checks.append({"case": "stdin-idempotent-retry", "passed": True})

            for index in range(4):
                await executor.start(f"parallel-{index}", f"printf 'parallel-{index}'")
            for index in range(4):
                streams, _ = await executor.collect(f"parallel-{index}")
                if streams != {"stdout": f"parallel-{index}".encode(), "stderr": b""}:
                    raise AssertionError("concurrent process outputs crossed")
            checks.append({"case": "four-live-process-handles", "passed": True})

            for name, action in [("interrupt", "process/signal"), ("terminate", "process/terminate")]:
                script = ("trap 'exit 42' INT; printf ready; while :; do sleep 1; done"
                          if name == "interrupt" else "printf ready; exec sleep 60")
                await executor.start(name, script)
                await executor.collect(name, until_stdout=b"ready")
                cancel_started = time.monotonic()
                params = {"processId": name}
                if name == "interrupt":
                    params["signal"] = "interrupt"
                await executor.call(action, params)
                await executor.collect(name, expected_code=42 if name == "interrupt" else 137)
                print(f"executor check passed: {name}", file=sys.stderr, flush=True)
                checks.append({"case": name, "passed": True, "seconds": time.monotonic() - cancel_started})

            file_uri = executor.cwd + "/synthetic.bin"
            payload = bytes(range(256)) * 512
            await executor.call("fs/writeFile", {"path": file_uri, "dataBase64": encoded(payload), "sandbox": None})
            response = await executor.call("fs/readFile", {"path": file_uri, "sandbox": None})
            if base64.b64decode(response["dataBase64"], validate=True) != payload:
                raise AssertionError("whole-file binary mismatch")
            checks.append({"case": "binary-whole-file", "passed": True, **digest(payload)})

            payload = bytes(range(255, -1, -1)) * 256
            await executor.call("fs/open", {"handleId": "writer", "path": file_uri, "mode": "replace", "sandbox": None})
            for offset in range(0, len(payload), 32768):
                await executor.call("fs/writeBlock", {"handleId": "writer", "offset": offset,
                                                     "chunk": encoded(payload[offset:offset + 32768])})
            await executor.call("fs/close", {"handleId": "writer"})
            await executor.call("fs/open", {"handleId": "reader", "path": file_uri, "mode": "read", "sandbox": None})
            actual = bytearray()
            for offset in range(0, len(payload), 8192):
                response = await executor.call("fs/readBlock", {"handleId": "reader", "offset": offset, "len": 8192})
                actual.extend(base64.b64decode(response["chunk"], validate=True))
            await executor.call("fs/close", {"handleId": "reader"})
            response = await executor.call("fs/readFile", {"path": file_uri, "sandbox": None})
            if actual != payload or base64.b64decode(response["dataBase64"], validate=True) != payload:
                raise AssertionError("block file replacement mismatch")
            await executor.call("fs/remove", {"path": file_uri, "sandbox": None})
            checks.append({"case": "block-file-replace-read-close-remove", "passed": True, **digest(payload)})
            methods = executor.methods

        # A fresh executor, not session migration: the process is deliberately
        # restarted after orderly stdio closure and reuses only synthetic home.
        async with connection(args, args.remote_root) as executor:
            await executor.start("restart", "printf lodex-restarted")
            streams, _ = await executor.collect("restart")
            if streams != {"stdout": b"lodex-restarted", "stderr": b""}:
                raise AssertionError("fresh executor restart failed")
        checks.append({"case": "orderly-stdio-close-and-fresh-restart", "passed": True})
        result = {"passed": True, "scope": "synthetic direct executor process/file qualification; no model or auth",
                  "total_seconds": time.monotonic() - started, "checks": checks,
                  "rpc_seconds": methods,
                  "limitations": ["No websocket/Noise session recovery or network-loss test.",
                                  "No model, inference, voice, auth-refresh or MCP conformance in this probe.",
                                  "Correctness fixture timings are not a controlled performance comparison."]}
        json.dump(result, report_file, indent=2)
        report_file.write("\n")
        print(json.dumps({"passed": True, "cases": len(checks), "total_seconds": result["total_seconds"]}), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ssh", type=Path, required=True)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--full-cli", action="store_true", help="prefix executor args with exec-server")
    parser.add_argument("--runtime-lib", required=True)
    parser.add_argument("--remote-root", required=True, help="new absolute synthetic lab directory")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--timeout", type=float, default=120)
    parser.add_argument("--hardware-lock", type=Path, default=DEFAULT_LOCK)
    args = parser.parse_args()
    if not all(Path(value).is_absolute() for value in (args.binary, args.runtime_lib, args.remote_root)):
        parser.error("remote paths must be absolute")
    with HardwareJob(args.hardware_lock):
        asyncio.run(qualify(args))


if __name__ == "__main__":
    main()
