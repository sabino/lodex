# Performance measurements

Measured on the Omega2+/MT7688 test device running OpenWrt 23.05.3. This is a
single-core, little-endian MIPS32r2/soft-float target with about 128 MiB RAM and
external SD storage. Results are specific to the workload, binary and firmware.

## Image size

| Image | Bytes |
| --- | ---: |
| Full upstream MIPS CLI reference | 307349644 |
| Stock headless App Server reference | 229765044 |
| Lodex v1.1 | 236607592 |
| Lodex v1.2, two-connection candidate | 236607236 |
| Lodex v1.3, executor entry point restored | 239722436 |
| Executor-only candidate, native qualification pending | 75614760 |

The executor CLI adds about 3.1 MB to the image while retaining a remote tools
execution path. Image size is not heap, RSS, swap or total physical RAM usage.

## Preliminary database pool comparison

Three new-home trials per variant used four concurrent durable threads, short
shell commands, history reads and five post-workload process samples. No model
calls or production prompts were used. The managed background service remained
running during each sequential batch, but was restarted between them. Its warmed
state was not held constant.

| Metric, median across three trials | Five connections | Two connections |
| --- | ---: | ---: |
| Observed process RSS high-water mark, KiB | 55096 | 52676 |
| Threads in final sample | 29 | 20 |
| Sum of measured request-group durations, seconds | 33.28 | 37.59 |

The smaller pool used fewer threads and had a lower median RSS peak, but the
request-group duration sum increased. That sum excludes notification-wait and
sampling time; it is not full user-turn latency. RSS, swap and faults varied
between runs. Background service activity and shared executable page-cache state
also limit this comparison. These data do not establish an overall speedup or a
steady-state RAM-saving guarantee.

[The preliminary data](pool-comparison-preliminary.json) retain per-trial
RSS/swap/fault/thread and request-group timing summaries. A later exploratory
alternating run had a transfer overlap one trial and is excluded from conclusions.
A clean alternating confirmation remains required before promotion.

## Initial placement observation

One sequential MCP fixture pair compared a full local harness with a separate
Linux primary using the device executor. Both used Lodex v1.3 on the device,
fresh controller homes and the same external fixture. No model inference was
included in these timing measurements.

| Device or RPC metric | Local harness | Remote primary/device executor |
| --- | ---: | ---: |
| Post-workload RSS range, KiB | 39196..45936 | 36676..36692 |
| Observed process RSS peak, KiB | 50860 | 47440 |
| Maximum sampled process swap, KiB | 5776 | 2400 |
| Sampled device threads | 16 | 6..8 |
| Device CPU ticks at first post-workload sample | 988 | 136 |
| Sum of fixture RPC request durations, seconds | 45.51 | 8.51 |

These are one-pair observations, not an overall speedup guarantee. Local ran first;
warm-cache/order and the background service are confounders. Controller resource
use, actual cloud network/inference latency, speech streaming and full user-turn
latency are not included. Request-duration sums exclude notification-wait and
sampling time. See [the counters and exact scope](placement-observation.json).
Repeated placement tests and native executor-only qualification remain work in progress.

## Execution placement

Compare the complete local harness, a remote/cloud primary with device tools,
and cloud tasks on the same representative workloads. Include device CPU, RSS,
swap and faults; startup; total turn/tool latency; output volume; MCP concurrency;
and reconnect/recovery behavior. Cloud inference, cloud harness execution and
remote tool execution are distinct. A faster cloud response cannot be inferred
from moving the harness alone, and device-only memory reduction is not a complete
end-to-end performance result.

Tools in tools/lodex use isolated fixture homes and bounded synthetic workloads.
The kernel does not expose smaps/PSS, so these measurements cannot split private
heap from shared executable pages precisely. Keep raw traces out of the repository.
