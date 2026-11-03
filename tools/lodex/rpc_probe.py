#!/usr/bin/env python3
"""Run deterministic App Server scenarios against an isolated fixture runtime."""
import argparse
import asyncio
from collections import deque
import json
from pathlib import Path
import sys
import time


class Probe:
    def __init__(self, process, timeout=120):
        self.process = process
        self.timeout = timeout
        self.next_id = 0
        self.notifications = 0
        self.pending_notifications = deque(maxlen=64)

    async def send(self, message):
        self.process.stdin.write((json.dumps(message) + "\n").encode())
        await self.process.stdin.drain()

    async def request(self, method, params):
        return (await self.request_many([(method, params)]))[0]

    async def request_many(self, requests):
        if not 1 <= len(requests) <= 16:
            raise ValueError("a parallel group must contain 1..16 requests")
        pending = {}
        responses = [None] * len(requests)
        for index, (method, params) in enumerate(requests):
            self.next_id += 1
            pending[self.next_id] = index
            await self.send({"id": self.next_id, "method": method, "params": params})
        async with asyncio.timeout(self.timeout):
            while pending:
                line = await self.process.stdout.readline()
                if not line:
                    raise RuntimeError("App Server stdout closed before response")
                message = json.loads(line)
                if "method" in message:
                    if "id" in message:
                        await self.send({"id": message["id"], "error": {
                            "code": -32601, "message": "Fixture client has no server-request capabilities"}})
                    else:
                        self.notifications += 1
                        self.pending_notifications.append(message)
                    continue
                request_id = message.get("id")
                if request_id not in pending:
                    raise RuntimeError("unexpected response ID")
                responses[pending.pop(request_id)] = message
        return responses

    async def wait_notification(self, method):
        for message in list(self.pending_notifications):
            if message.get("method") == method:
                self.pending_notifications.remove(message)
                return message
        async with asyncio.timeout(self.timeout):
            while True:
                line = await self.process.stdout.readline()
                if not line:
                    raise RuntimeError("App Server closed before required notification")
                message = json.loads(line)
                if "method" not in message or "id" in message:
                    raise RuntimeError("unexpected message while waiting for notification")
                self.notifications += 1
                if message["method"] == method:
                    return message
                self.pending_notifications.append(message)


def check_response(response, step):
    if "error" in response and not step.get("expect_error"):
        raise AssertionError(f"RPC {step['method']} failed: {response['error']}")
    if step.get("expect_error") and "error" not in response:
        raise AssertionError("expected a protocol error")
    for check in step.get("checks", []):
        value = response
        for key in check["path"]:
            value = value[key]
        if value != check["equals"]:
            raise AssertionError(f"response mismatch at {check['path']!r}")


def resolve_refs(value, variables):
    if isinstance(value, dict):
        if set(value) == {"$ref"}:
            return variables[value["$ref"]]
        return {key: resolve_refs(item, variables) for key, item in value.items()}
    if isinstance(value, list):
        return [resolve_refs(item, variables) for item in value]
    return value


async def run(command, steps, timeout, idle_seconds=0, ready_file=None, initial_variables=None,
              process_observer=None):
    run_started = time.monotonic()
    process = await asyncio.create_subprocess_exec(
        *command, stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE, limit=2 ** 20)
    probe = Probe(process, timeout)
    stderr_tail = bytearray()

    async def drain_errors():
        while chunk := await process.stderr.read(4096):
            stderr_tail.extend(chunk)
            del stderr_tail[:-65536]

    drain = asyncio.create_task(drain_errors())
    reports = []
    variables = dict(initial_variables or {})
    try:
        if process_observer is not None:
            process_observer(process.pid)
        for step in steps:
            if "wait_notification" in step:
                response = await probe.wait_notification(step["wait_notification"])
                check_response(response, {**step, "method": step["wait_notification"]})
                reports.append({"method": step["wait_notification"], "notification": response})
                continue
            params = resolve_refs(step.get("params", {}), variables)
            if step.get("notification"):
                await probe.send({"method": step["method"], "params": params})
                continue
            batch = step.get("parallel", [step])
            if not 1 <= len(batch) <= 16 or any(
                    "method" not in item or item.get("notification") or "parallel" in item
                    for item in batch):
                raise ValueError("parallel groups contain 1..16 ordinary requests")
            requests = [(item["method"], resolve_refs(item.get("params", {}), variables))
                        for item in batch]
            print(f"probe request: {', '.join(method for method, _ in requests)}",
                  file=sys.stderr, flush=True)
            started = time.monotonic()
            responses = await probe.request_many(requests)
            elapsed = time.monotonic() - started
            for item, response in zip(batch, responses, strict=True):
                check_response(response, item)
                for name, path in item.get("capture", {}).items():
                    value = response
                    for key in path:
                        value = value[key]
                    variables[name] = value
                reports.append({"method": item["method"], "elapsed_seconds": elapsed,
                                "parallel_group_size": len(batch), "response": response})
        workload_seconds = time.monotonic() - run_started
        if ready_file is not None:
            Path(ready_file).write_text(json.dumps({"passed_requests": len(reports)}) + "\n")
        if idle_seconds:
            await asyncio.sleep(idle_seconds)
    finally:
        shutdown_started = time.monotonic()
        process.stdin.close()
        try:
            await asyncio.wait_for(process.wait(), timeout=15)
        except TimeoutError:
            process.terminate()
            try:
                await asyncio.wait_for(process.wait(), timeout=5)
            except TimeoutError:
                process.kill()
                await process.wait()
        await drain
    if process.returncode != 0:
        raise RuntimeError(f"runtime exited {process.returncode}: " + stderr_tail.decode(errors="replace")[-1000:])
    return {"passed": True, "notifications": probe.notifications, "requests": reports, "captures": variables,
            "workload_seconds": workload_seconds,
            "shutdown_seconds": time.monotonic() - shutdown_started}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scenario", required=True, type=Path)
    parser.add_argument("--timeout", type=float, default=120)
    parser.add_argument("--idle-seconds", type=float, default=0)
    parser.add_argument("--ready-file", type=Path)
    parser.add_argument("--variables-file", type=Path)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        parser.error("supply runtime argv after --")
    if not 0 <= args.idle_seconds <= 3600:
        parser.error("idle seconds must be between 0 and 3600")
    variables = json.loads(args.variables_file.read_text()) if args.variables_file else None
    result = asyncio.run(run(command, json.loads(args.scenario.read_text()), args.timeout,
                            args.idle_seconds, args.ready_file, variables))
    json.dump(result, sys.stdout, indent=2)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
