#!/usr/bin/env python3
"""Synthetic native profile CLI and process-kill recovery qualification."""
import argparse
import asyncio
import hashlib
import json
from pathlib import Path
import shlex

from hardware_job import DEFAULT_LOCK, HardwareJob
from native_benchmark import remote


async def qualify(args):
    q = shlex.quote
    base = (f"env LD_LIBRARY_PATH={q(args.runtime_lib)} {q(args.binary)} "
            f"--root {q(args.remote_root)} ")
    checks = []

    async def invoke(arguments, data=b"", success=True):
        process = await asyncio.create_subprocess_exec(
            str(args.ssh), base + shlex.join(arguments),
            stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
        try:
            output, _error = await asyncio.wait_for(process.communicate(data), 90)
        except BaseException:
            if process.returncode is None:
                process.kill()
                await process.wait()
            raise
        if (process.returncode == 0) != success:
            raise AssertionError(f"profile {arguments[0]}: unexpected exit {process.returncode}")
        return output

    async def inspect():
        output = await invoke(["inspect", "main"])
        return dict(line.split("=", 1) for line in output.decode().splitlines())

    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("x") as output:
        await remote(args.ssh, f"set -eu\n[ ! -e {q(args.remote_root)} ]\n[ ! -e {q(args.remote_root + '-crash')} ]")
        if await invoke(["init", "main"]) != b"initialized=main\n":
            raise AssertionError("profile init output mismatch")
        initial = await inspect()
        old = await invoke(["show", "main", "memory"])
        if old != b"# Memory\n" or initial["revision"] != hashlib.sha256(old).hexdigest():
            raise AssertionError("initial memory/revision mismatch")
        if b"Describe this agent here" not in await invoke(["show", "main", "soul"]):
            raise AssertionError("missing editable soul template")
        checks.append("create-and-explicit-local-read")

        text = "synthetic native memory caf\u00e9\n".encode()
        expected = hashlib.sha256(text).hexdigest()
        revision = (await invoke(["write-memory", "main", "--expect", initial["revision"],
                                  "--writer", "native:fixture"], text)).decode().strip()
        after = await inspect()
        if revision != expected or after["revision"] != expected or after["last_writer"] != "native:fixture" or after["last_write_current"] != "true":
            raise AssertionError("native attribution/revision mismatch")
        if text.strip() in (await invoke(["inspect", "main"])):
            raise AssertionError("inspection printed private text")
        if await invoke(["show", "main", "previous-memory"]) != old:
            raise AssertionError("previous version mismatch")
        checks.append("utf8-atomic-write-attribution-and-redacted-inspection")

        await invoke(["write-memory", "main", "--expect", initial["revision"], "--writer", "stale"], b"stale", False)
        await invoke(["write-memory", "main", "--expect", revision, "--writer", "oversized"], b"x" * 32769, False)
        await invoke(["write-memory", "main", "--expect", revision, "--writer", "invalid-utf8"], b"\xff", False)
        if await invoke(["show", "main", "memory"]) != text:
            raise AssertionError("failed input changed memory")
        checks.append("stale-oversized-invalid-utf8-rejected-without-change")

        async def race(label):
            command = base + shlex.join(["write-memory", "main", "--expect", revision, "--writer", label])
            process = await asyncio.create_subprocess_exec(str(args.ssh), command,
                stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
            try:
                await asyncio.wait_for(process.communicate(label.encode()), 90)
            except BaseException:
                if process.returncode is None:
                    process.kill()
                    await process.wait()
                raise
            return label, process.returncode

        racers = await asyncio.gather(race("race-one"), race("race-two"))
        winners = [name for name, code in racers if code == 0]
        if len(winners) != 1 or any(code not in (0, 1) for _, code in racers):
            raise AssertionError("concurrent writers did not resolve as one commit/one conflict")
        winner = winners[0]
        if await invoke(["show", "main", "memory"]) != winner.encode():
            raise AssertionError("race winner memory mismatch")
        if (await inspect())["last_writer"] != winner:
            raise AssertionError("race winner attribution mismatch")
        checks.append("concurrent-process-writers-have-one-winner")

        permissions = await remote(args.ssh, "for profile_path in " + " ".join(q(path) for path in [
            args.remote_root, args.remote_root + "/main", args.remote_root + "/main/SOUL.md",
            args.remote_root + "/main/MEMORY.md", args.remote_root + "/main/.memory.last-write"]) +
            '; do ls -ld "$profile_path"; done | awk \'{print $1}\'')
        if permissions.splitlines() != ["drwx------", "drwx------", "-rw-------", "-rw-------", "-rw-------"]:
            raise AssertionError("private directory/file permissions mismatch")
        checks.append("private-directory-and-file-modes")

        crash = await remote(args.ssh, f"set -eu\numask 077\nLD_LIBRARY_PATH={q(args.runtime_lib)} "
                             f"{q(args.crash_binary)} {q(args.remote_root + '-crash')}")
        stages = ["after-previous-stage", "after-intent", "during-memory-write", "after-memory", "after-backup", "after-attribution"]
        if crash.splitlines() != [f"PASS process-kill recovery {stage}" for stage in stages] + ["PASS all six process-kill commit boundaries"]:
            raise AssertionError("native crash qualification did not pass every stage")
        checks.extend("process-kill-recovery-" + stage for stage in stages)
        result = {"passed": True, "checks": checks, "scope": "native synthetic local profile CLI, writer races and six process-kill commit boundaries",
                  "limitations": ["No actual private profiles, model, network export, power-loss or SD-failure test.",
                                  "Attribution is caller supplied, not authentication or a tamper-proof audit log.",
                                  "Standalone CLI qualified; full launcher integration has a separate gate."]}
        json.dump(result, output, indent=2)
        output.write("\n")
        print(json.dumps({"passed": True, "cases": len(checks)}), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ssh", type=Path, required=True)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--crash-binary", required=True, help="lab-only fault-injection example")
    parser.add_argument("--runtime-lib", required=True)
    parser.add_argument("--remote-root", required=True, help="new absolute synthetic profile directory")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--hardware-lock", type=Path, default=DEFAULT_LOCK)
    args = parser.parse_args()
    if not all(Path(value).is_absolute() for value in (args.binary, args.crash_binary, args.runtime_lib, args.remote_root)):
        parser.error("remote paths must be absolute")
    with HardwareJob(args.hardware_lock):
        asyncio.run(qualify(args))


if __name__ == "__main__":
    main()
