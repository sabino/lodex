# Embedded runtime tuning

Lodex v1.8 keeps the local App Server, Remote, saved authentication, durable
threads, execution tools and upstream MCP client. These changes target the
MIPS32 little-endian, soft-float build; they do not move model inference onto
the device or silently offload local files.

## Executable layout and SD access

`tools/lodex/build-headless` selects size optimization, ThinLTO and a small
dynamic export table. Large-GOT addressing must apply to dependencies as well
as the final crate. The large core crate uses 256 compilation units to avoid
MIPS TLS relocation overflow. Internal Rust symbols are not an extension ABI;
removing their dynamic exports does reduce names available to backtrace tools.
Use a separate profiling build when those names are needed.

Smaller storage alone did not make the tested workload use less resident RAM.
The more useful runtime change was avoiding speculative reads of the large,
sparsely accessed executable from SD. On Linux/MIPS, the launcher applies
`MADV_RANDOM` to its main executable's read-only load segments and protected
relocation pages. The Linux 5.15 file-mapping path
[skips its mmap read-ahead for this advice](https://github.com/torvalds/linux/blob/v5.15/mm/filemap.c).
This is not a global block-device setting, cache flush or limit on memory.
Libraries, heap, stack and writable load segments retain their normal policy.

The implementation uses the loader's program headers and the process auxiliary
vector; it does not parse `/proc`, spawn a helper or keep a background task.
Advice failures are nonfatal. Set `LODEX_MEMORY_DIAGNOSTICS=1` for numeric startup
diagnostics, or `LODEX_MMAP_READAHEAD=normal` to keep the kernel's normal policy
when comparing another storage device or workload. Neither variable exposes
prompts or credentials. Other CPU targets do not apply the advice automatically.

The largest emitted code contributions in the measured image were:

| Compilation unit group | Text bytes |
| --- | ---: |
| Agent core | 27,106,680 |
| App Server | 10,253,688 |
| Configuration | 8,853,416 |
| Remote execution server | 4,366,408 |
| App Server protocol | 4,228,336 |
| Durable rollout handling | 3,743,812 |

These link-map totals include generic dependency code instantiated within a
crate. They are static code attribution, not feature usage, heap allocations or
the amount that could be removed safely. Required execution, protocol and MCP
paths still need their implementations. A runtime-disabled catalog is not
necessarily absent from the executable.

## Database working set

The `lodex-low-memory` feature retains two writable connections per database.
Each has a 32-entry prepared-statement cache and a 512 KiB suggested SQLite page
cache. Pools use a 30-second idle timeout; reopened connections receive the
same settings. Maintenance uses the same small page-cache target rather than
the general-purpose 16 MiB setting. These are per-connection targets, not a hard
process-wide RAM cap; see [SQLite's cache-size semantics](https://www.sqlite.org/pragma.html#pragma_cache_size).

Read-only diagnostic connections keep their existing opening behavior. Setting
a cache pragma while opening them can read the schema before the caller's
deadline handler is installed, which regressed the exclusive-writer-lock test.
Transaction, busy-timeout, cancellation, journal and persistence rules remain
upstream-compatible.

The low-memory state suite passes 246 tests and its doctest; the default feature
configuration passes 245 tests and its doctest. The additional test exceeds both
statement caches, replaces connections and verifies persisted data. A native
four-thread check waits through the idle timeout before reading history again.

## Measuring without overloading the device

Compile `tools/lodex/proc-sampler.c` with the matching SDK and pass its absolute
device path as `--sampler` to `tools/lodex/native_benchmark.py`. It reads bounded
process counters directly, without repeated shell/awk launches. At 2 Hz the
calibration on the test device used approximately 0.23% of one CPU. It checks
process identity and stops when that process exits. The Python parser stays on
the development host.

Use fresh synthetic homes, the same configuration and immutable binaries, and
alternate versions. Serialize uploads, hashing and tests with `HardwareJob`.
Do not mix a transfer or diagnostic memory spike into a timing trial. Keep a
production rollback and restore Remote after isolated comparisons.

Compare wall time, RSS, sampled process swap and CPU together. Read-ahead changes
can increase the number of major faults while reducing unnecessary reads and
elapsed time. A fault count alone does not measure transferred bytes or explain
latency. The tested kernel provides `RssAnon` and `RssFile`, but not usable PSS;
neither RSS category is a precise allocation/ownership breakdown.

[Optional embedded configuration](embedded-config.toml) also disables unneeded
catalogs and telemetry exporters. Merge only the desired settings into an
existing configuration so authentication, model providers and MCP declarations
are retained. This is independent of executable read-ahead and is not credited
with the benchmark's performance improvement.
