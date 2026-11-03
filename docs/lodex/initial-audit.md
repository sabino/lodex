# Lodex footprint audit

Initial source and physical-device audit, 2026-11-03 UTC. No production
executable, settings, service, authentication, or conversations were modified.

## Measured baseline

Native Codex 0.166.0, release commit
6ff1e1ca3b30fc8b914720681fcf5fe62e95f856, MIPS32r2/musl, root, direct tools,
Remote app-server. Observed while the user was independently using Codex.

| Measurement | Result |
|---|---:|
| Executable | approximately 294 MiB |
| Machine-code section | 251.79 MiB |
| Read-only data section | 14.14 MiB |
| Unwind table plus index | approximately 11.8 MiB |
| Initial observed process RSS | 46.55 MiB |
| Three subsequent RSS samples | 49.24, 55.45, 52.11 MiB |
| Process historical RSS high-water mark | 62.80 MiB |
| Process swap in subsequent samples | 15.21, 14.05, 14.06 MiB |
| Threads in those samples | 26, 26, 24 |
| Major page faults over 21 seconds | 294 |

Raw local artifacts: build/codex-audit/baseline.json,
runtime-samples.txt, thread-names.txt, dynamic-symbols.txt.

These are observations, not controlled idle/active benchmarks. RSS includes
file-backed executable pages, which can be reclaimed. Process swap is separate
from total system swap. Virtual size is not physical RAM consumption. This
kernel has no smaps/smaps_rollup, so exact per-map PSS and heap attribution are
unavailable. Major faults can involve executable pages or swap; this evidence
does not separate them.

## Source findings

- cli/Cargo.toml links the full TUI, cloud tasks, exec-server CLI, app-server
  test client, daemon management, and many other modes into the same executable.
  A Remote-only entry point is a concrete starting point.
- arg0/src/lib.rs creates the main thread and Tokio runtime workers with
  codex_async_utils::THREAD_STACK_SIZE_BYTES. async-utils/src/lib.rs sets
  that value to 16 MiB. Reducing it primarily reduces reservations; actual RAM
  savings and overflow safety require measurements.
- state/src/sqlite/connection.rs allows five connections per opened database.
  The live process has many sqlx-sqlite worker threads. Pool limits, per-connection
  statement/page caches, and lazily opening optional databases deserve A/B tests.
  The number five is a maximum, not proof all pools are full.
- The process has OpenTelemetry and telemetry-shutdown worker threads.
  Feature removal can eliminate background work as well as code.
- core and app-server retain optional integrations as ordinary dependencies.
  Runtime feature flags alone do not prove that code is excluded.
- MIPS uses the system allocator here. The upstream jemalloc selection only
  covers x86-64 and AArch64 musl. Allocator tuning is an experiment, not an
  assumed improvement.
- The installed binary is already stripped and uses panic=abort. Running strip
  again or disabling panic unwinding again is not a substantial new optimization.

## Preliminary cut order

This order assumes an appliance controlled mainly through the mobile app.
It reflects relevance to that use case, not measured personal usage frequency.

| Priority | Candidate | Expected effect | Main cost or risk |
|---|---|---|---|
| 1 | Remove TUI, terminal rendering/highlighting, cloud-task UI, test/debug CLI modes from the appliance executable | Smaller image and startup footprint | Local interactive TUI moves to a separate optional build |
| 2 | Compile out telemetry exporters and unused app/plugin catalogs | Less background work, fewer dependencies | Reduced diagnostics and integrations |
| 3 | Lower SQLite pool/cache limits and open optional databases lazily | Direct candidate for heap/thread reduction | Must preserve concurrent writes, history, recovery and Remote enrollment |
| 4 | Make unused hooks, advanced memory features, multi-agent/guardian integrations optional | Smaller dependency graph and less initialization | Remove only explicitly unwanted workflows; inspect shared dependencies |
| 5 | Prune redundant TLS/network stacks and platform-specific parsers | Smaller code/data; possibly faster startup | Must preserve the TLS/WebSocket stack Remote actually uses |
| 6 | Size-oriented compilation, selective LTO, unwind-table policy, allocator and stack tuning | Cross-cutting code/working-set reduction | May trade CPU speed or diagnostics for size; stack reductions need stress tests |
| Keep | Remote enrollment, transport, authentication refresh, core agent loop, shell, file editing, Git, resumable conversations | Required appliance function | Regression tests must cover the entire mobile path |

No per-feature RAM savings are claimed yet. Library archives include compiler
metadata, unused functions and duplicate/generic instantiations. For example,
the TUI archive is 94.5 MiB but its object text/read-only-data total is about
23.4 MiB; neither number is its exact linked or resident contribution.
The dynamic-symbol scan covers only about 18.7 MiB of the 251.8 MiB code section.
An exact size ranking needs a linker map and comparison builds.

## Comparison builds and acceptance criteria

A separate stock headless codex-app-server build is underway on the laptop:
build/codex-audit/headless-build.log. This is a baseline comparison, not a
deployable replacement: the existing daemon launches the CLI's app-server
subcommand, so a small compatible launcher/controller still needs design.
No size result is recorded until the build succeeds. Production remains the
verified CLI.

Measure one change at a time with identical compiler settings and a linker map.
Compare idle Remote, a short shell task, a file-edit/Git task, thread resume,
and a long-output task. Record RSS/high-water mark, process swap, major faults,
startup time, task latency, and image sections. Repeat runs, separating cold
and warm starts. Do not replace a running user session for profiling.

Before deployment, prove mobile pairing, reconnect, authentication refresh,
tool execution, file edits, history/resume, restart recovery, and clean shutdown.
Keep the current binary/config as rollback. No percentage reduction is promised
before these experiments.

## Fork shape

Lodex is a reasonable working name. Prefer a small patch series on upstream
with an appliance feature profile over a full rewrite. Retain upstream notices
and protocol compatibility. A GitHub repository has not been created or
published; first choose which optional capabilities the appliance should retain.
