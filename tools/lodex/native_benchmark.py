#!/usr/bin/env python3
"""Sequential synthetic App Server trials in new remote homes; no model calls."""
import argparse
import asyncio
import hashlib
import json
from pathlib import Path
import shlex

import footprint
from hardware_job import DEFAULT_LOCK, HardwareJob
import rpc_probe


async def remote(helper, command, data=None):
    process = await asyncio.create_subprocess_exec(
        str(helper), command, stdin=asyncio.subprocess.PIPE,
        stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
    try:
        stdout, stderr = await asyncio.wait_for(process.communicate(data), 90)
    except BaseException:
        if process.returncode is None:
            process.kill()
            await process.wait()
        raise
    if process.returncode:
        raise RuntimeError(stderr.decode(errors="replace")[-2000:])
    return stdout.decode()


async def trial(args, index):
    root = args.remote_root.rstrip("/") + f"/trial-{index}"
    q = shlex.quote
    await remote(args.ssh, f"set -eu\numask 077\nmkdir {q(root)}\nmkdir {q(root + '/tmp')}\ncat > {q(root + '/config.toml')}",
                 args.config.read_bytes())
    ready = args.output_dir / f"trial-{index}.ready"
    launch = (f"set -eu\ncd {q(root)}\nprintf '%s' $$ > runtime.pid\n"
              + (f"{q(args.sampler)} $$ 1200 500 > workload.proc &\n" if args.sampler else "") +
              "exec env -u OPENAI_API_KEY -u CODEX_API_KEY -u CODEX_ACCESS_TOKEN "
              f"CODEX_HOME={q(root)} TMPDIR={q(root + '/tmp')} "
              f"LD_LIBRARY_PATH={q(args.runtime_lib)} {q(args.binary)} app-server --listen stdio://")
    steps = json.loads((Path(__file__).parent / "scenarios/concurrent-history.json").read_text())
    task = asyncio.create_task(rpc_probe.run([str(args.ssh), launch], steps, 120,
                                           idle_seconds=30, ready_file=ready))
    try:
        async with asyncio.timeout(240):
            while not ready.exists():
                if task.done():
                    await task
                    raise RuntimeError("runtime ended without readiness marker")
                await asyncio.sleep(0.2)
        pid = (await remote(args.ssh, f"cat {q(root + '/runtime.pid')}")).strip()
        if not pid.isdecimal() or int(pid) < 1:
            raise ValueError("invalid sampler PID")
        if args.sampler:
            capture = await remote(args.ssh, f"{q(args.sampler)} {pid} 5 2000")
        else:
            capture = await remote(args.ssh, f"sh -s -- {pid} 5 2",
                                   (Path(__file__).parent / "sample-proc.sh").read_bytes())
        samples = footprint.parse_samples(capture)
        (args.output_dir / f"trial-{index}.proc").write_text(capture)
        result = await task
        (args.output_dir / f"trial-{index}.rpc.json").write_text(json.dumps(result, indent=2) + "\n")
        report = {"trial": index, "remote_home": root, "footprint": footprint.summarize(samples),
                  "threads": [sample["threads"] for sample in samples],
                  "workload_seconds": result["workload_seconds"],
                  "shutdown_seconds": result["shutdown_seconds"],
                  "request_group_seconds": [request.get("elapsed_seconds")
                                            for request in result["requests"]
                                            if "elapsed_seconds" in request]}
        if args.sampler:
            workload_capture = await remote(args.ssh, f"cat {q(root + '/workload.proc')}")
            (args.output_dir / f"trial-{index}.workload.proc").write_text(workload_capture)
            report["whole_run_footprint"] = footprint.summarize(footprint.parse_samples(workload_capture))
        (args.output_dir / f"trial-{index}.summary.json").write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps(report), flush=True)
        return report
    finally:
        if not task.done():
            task.cancel()
        await asyncio.gather(task, return_exceptions=True)


async def run(args):
    # Create-only paths: never delete a previous trial or touch the production home.
    args.output_dir.mkdir()
    await remote(args.ssh, f"umask 077\nmkdir {shlex.quote(args.remote_root)}")
    reports = [await trial(args, index) for index in range(1, args.repeat + 1)]
    (args.output_dir / "summary.json").write_text(json.dumps({
        "binary": args.binary, "config_sha256": hashlib.sha256(args.config.read_bytes()).hexdigest(),
        "scope": "synthetic concurrent shell/history, no model calls; idle samples after workload",
        "trials": reports}, indent=2) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ssh", required=True, type=Path, help="pinned SSH helper executable")
    parser.add_argument("--binary", required=True)
    parser.add_argument("--remote-root", required=True, help="new absolute remote trial directory")
    parser.add_argument("--config", required=True, type=Path, help="synthetic fixture config without credentials")
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--runtime-lib", required=True)
    parser.add_argument("--sampler", help="optional native proc-sampler path on the device")
    parser.add_argument("--hardware-lock", type=Path, default=DEFAULT_LOCK)
    parser.add_argument("--repeat", type=int, choices=range(1, 6), default=3)
    args = parser.parse_args()
    if not all(Path(path).is_absolute() for path in
               [args.binary, args.remote_root, args.runtime_lib] + ([args.sampler] if args.sampler else [])):
        parser.error("remote paths must be absolute")
    with HardwareJob(args.hardware_lock):
        asyncio.run(run(args))


if __name__ == "__main__":
    main()
