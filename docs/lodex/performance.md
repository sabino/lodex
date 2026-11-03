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

The executor CLI adds about 3.1 MB to the image while retaining a remote tools
execution path. Image size is not heap, RSS, swap or total physical RAM usage.

## Preliminary database pool comparison

Three new-home trials per variant used four concurrent durable threads, short
shell commands, history reads and five post-workload process samples. No model
calls or production prompts were used. The managed background service remained
running during both sequential batches.

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
