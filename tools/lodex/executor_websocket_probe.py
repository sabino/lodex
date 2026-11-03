#!/usr/bin/env python3
"""Synthetic auth and process-session recovery against a lab executor listener.

Controller-only dependency: websockets==15.0.1. Use a loopback listener and pinned
SSH forwarding for a remote device. No model, real token store or audio is used.
"""
import argparse
import asyncio
import json
from pathlib import Path
import time

from websockets.asyncio.client import connect
from websockets.exceptions import InvalidStatus

from executor_probe import Executor, encoded
from hardware_job import DEFAULT_LOCK, HardwareJob


class WebsocketWire:
    """Adapt one JSON WebSocket message to the shared line-oriented test client."""

    def __init__(self, websocket):
        self.websocket = websocket
        self.stdin = self.stdout = self
        self.pending = None

    def write(self, data):
        if self.pending is not None or not data.endswith(b"\n"):
            raise AssertionError("invalid fixture write")
        self.pending = data[:-1].decode()

    async def drain(self):
        await self.websocket.send(self.pending)
        self.pending = None

    async def readline(self):
        data = await self.websocket.recv()
        return (data.encode() if isinstance(data, str) else data) + b"\n"


async def qualify(args):
    token = args.token_file.read_text().strip()
    if not token or "\n" in token or "\r" in token:
        raise ValueError("expected a nonempty single-line synthetic token")
    options = {"proxy": None, "compression": None, "max_size": 2 ** 20,
               "open_timeout": 10, "close_timeout": 2}
    authorization = {"Authorization": "Bearer " + token}
    checks = []

    async def rejected(headers, status, origin=None):
        try:
            async with connect(args.url, additional_headers=headers, origin=origin, **options):
                raise AssertionError("unauthorized websocket was accepted")
        except InvalidStatus as error:
            if error.response.status_code != status:
                raise AssertionError(f"expected HTTP {status}, got {error.response.status_code}") from None

    def client(websocket):
        return Executor(WebsocketWire(websocket), args.cwd, args.timeout)

    with args.output.open("x") as output:
        started = time.monotonic()
        await rejected({}, 401)
        await rejected({"Authorization": "Bearer invalid-synthetic-token"}, 401)
        await rejected(authorization, 403, "https://fixture.invalid")
        checks.append({"case": "missing-wrong-bearer-and-origin-rejected", "passed": True})

        async with connect(args.url, additional_headers=authorization, **options) as socket:
            first = client(socket)
            session = await first.initialize()
            async with connect(args.url, additional_headers=authorization, **options) as duplicate:
                response = await client(duplicate).probe.request("initialize", {
                    "clientName": "lodex-session-conflict-fixture", "resumeSessionId": session})
                if response.get("error", {}).get("code") != -32010:
                    raise AssertionError("active session was attached twice")
            checks.append({"case": "active-session-duplicate-attach-rejected", "passed": True})

            await first.start("recover-stdin", 'printf "ready\\n"; IFS= read -r a; printf "%s\\n" "$a"; IFS= read -r b; printf "%s\\n" "$b"', True)
            await first.collect("recover-stdin", until_stdout=b"ready\n")
            response = await first.call("process/write", {
                "processId": "recover-stdin", "chunk": encoded(b"alpha\n"), "writeId": "first"})
            if response["status"] != "accepted":
                raise AssertionError("initial input not accepted")
            # Abruptly lose this TCP stream, without stopping the executor or SSH.
            socket.transport.abort()

        recovery_started = time.monotonic()
        async with connect(args.url, additional_headers=authorization, **options) as socket:
            resumed = client(socket)
            if await resumed.initialize(session) != session:
                raise AssertionError("resumed a different session")
            for name, data in [("first", b"alpha\n"), ("second", b"beta\n")]:
                response = await resumed.call("process/write", {
                    "processId": "recover-stdin", "chunk": encoded(data), "writeId": name})
                if response["status"] != "accepted":
                    raise AssertionError("resumed stdin not accepted")
            streams, _ = await resumed.collect("recover-stdin")
            if streams != {"stdout": b"ready\nalpha\nbeta\n", "stderr": b""}:
                raise AssertionError("output replay or stdin retry changed bytes after reconnect")
            checks.append({"case": "abrupt-stream-loss-session-output-and-stdin-recovery", "passed": True,
                           "seconds": time.monotonic() - recovery_started})

        async with connect(args.url, additional_headers=authorization, **options) as socket:
            invalid = client(socket)
            response = await invalid.probe.request("initialize", {
                "clientName": "lodex-unknown-session-fixture", "resumeSessionId": "nonexistent-synthetic-session"})
            if response.get("error", {}).get("code") != -32600:
                raise AssertionError("unknown session not rejected")
        checks.append({"case": "unknown-session-rejected", "passed": True})

        result = {"passed": True, "cases": len(checks), "checks": checks,
                  "total_seconds": time.monotonic() - started,
                  "scope": "capability-token listener behind pinned SSH; synthetic process-session socket-loss recovery",
                  "limitations": ["Does not interrupt SSH, Wi-Fi, power or the executor process.",
                                  "No Noise relay, hosted cloud, OAuth refresh, JWT expiry or detached-session TTL test.",
                                  "No model, MCP, voice or controlled performance comparison."]}
        json.dump(result, output, indent=2)
        output.write("\n")
        print(json.dumps({"passed": True, "cases": len(checks), "total_seconds": result["total_seconds"]}), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--url", required=True, help="lab WebSocket URL, normally an SSH loopback forward")
    parser.add_argument("--token-file", type=Path, required=True, help="synthetic lab capability token only")
    parser.add_argument("--cwd", required=True, help="absolute synthetic lab directory on executor")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--timeout", type=float, default=120)
    parser.add_argument("--hardware-lock", type=Path, default=DEFAULT_LOCK)
    args = parser.parse_args()
    if not Path(args.cwd).is_absolute():
        parser.error("executor cwd must be absolute")
    with HardwareJob(args.hardware_lock):
        asyncio.run(qualify(args))


if __name__ == "__main__":
    main()
