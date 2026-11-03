#!/usr/bin/env python3
"""Parse bounded /proc samples on the build host; no target Python dependency."""
import argparse
import json
from pathlib import Path


def parse_samples(text):
    samples = []
    current = None
    identity = None
    for line in text.splitlines():
        if line.startswith("sample "):
            if current is not None:
                raise ValueError("incomplete sample")
            current = {"timestamp": int(line.split()[1])}
        elif current is None:
            raise ValueError("data outside sample")
        elif line.startswith("stat "):
            raw = line[5:]
            left = raw.index("(")
            right = raw.rindex(")")
            fields = raw[right + 1:].split()
            pid = int(raw[:left].strip())
            start_ticks = int(fields[19])  # /proc stat field 22; fields starts at 3.
            this_identity = (pid, start_ticks)
            if identity is not None and identity != this_identity:
                raise ValueError("process identity changed during capture")
            identity = this_identity
            current.update(pid=pid, start_ticks=start_ticks,
                           minor_faults=int(fields[7]), major_faults=int(fields[9]),
                           cpu_ticks=int(fields[11]) + int(fields[12]))
        elif line.startswith("status "):
            _, key, value = line.split()
            names = {"VmRSS:": "rss_kib", "VmHWM:": "rss_peak_kib",
                     "VmSwap:": "swap_kib", "Threads:": "threads"}
            current[names[key]] = int(value)
        elif line == "end":
            required = {"pid", "start_ticks", "rss_kib", "rss_peak_kib",
                        "swap_kib", "threads", "cpu_ticks", "major_faults"}
            if not required <= current.keys():
                raise ValueError("sample missing counters")
            if samples and current["timestamp"] < samples[-1]["timestamp"]:
                raise ValueError("sample clock moved backwards")
            samples.append(current)
            current = None
        else:
            raise ValueError("unknown sample record")
    if current is not None or not samples:
        raise ValueError("empty or truncated capture")
    return samples


def summarize(samples):
    first, last = samples[0], samples[-1]
    return {"sample_count": len(samples), "elapsed_seconds": last["timestamp"] - first["timestamp"],
            "rss_min_kib": min(x["rss_kib"] for x in samples),
            "rss_max_kib": max(x["rss_kib"] for x in samples),
            "rss_peak_kib": max(x["rss_peak_kib"] for x in samples),
            "swap_max_kib": max(x["swap_kib"] for x in samples),
            "major_fault_delta": last["major_faults"] - first["major_faults"],
            "cpu_tick_delta": last["cpu_ticks"] - first["cpu_ticks"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("capture", type=Path)
    args = parser.parse_args()
    samples = parse_samples(args.capture.read_text())
    print(json.dumps({"summary": summarize(samples), "samples": samples}, indent=2))


if __name__ == "__main__":
    main()
