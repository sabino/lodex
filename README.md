# Lodex

A MIPS/OpenWrt-focused, headless variant of [OpenAI Codex](https://github.com/openai/codex).
It runs the agent harness on small Linux systems while retaining Codex authentication,
Remote access, resumable threads, shell/file/Git tools and MCP clients.

The first hardware target is the Omega2+: one MT7688/MIPS24KEc core, about 128 MiB
RAM and OpenWrt 23.05.3. Compatible OpenWrt routers are a development target; other
chipsets, ABIs and firmware versions have not been qualified.

**Development preview.** The native headless service runs on the test device and
reconnects through Codex Remote. Optimization and extended compatibility testing
are ongoing. Current images are still large: external SD/USB storage is required.
This is an independent fork, not an official OpenAI distribution.

## Execution modes

- **Local harness:** run Codex locally, with tools executing on the device. Model
  inference still uses the configured model provider; this does not run an LLM on
  a router's CPU.
- **Remote control:** access the running local harness through Codex Remote or the
  App Server protocol.
- **Remote primary with device tools:** a cloud/remote harness controls the device
  through an authenticated executor or MCP endpoint. A separate Linux controller
  has passed real model-driven shell and MCP tool/resource tests over SSH; hosted
  Cloud and voice-streaming qualification remain pending. See the
  [remote-primary setup and limits](docs/lodex/remote-primary.md).
- **Cloud tasks:** an explicit, bounded task adapter is in development. A new cloud
  task is separate from moving a running local thread. Automatic private-context
  synchronization is not enabled.

The local harness remains supported. Moving computation off the device must
preserve tools, connection recovery and explicit data boundaries.

## What changes from upstream

The appliance entry point omits the terminal UI and unrelated management/debug
modes. It reuses upstream App Server, Remote, login and MCP implementations rather
than substituting a tools-only MCP shim. The MIPS port uses a matching OpenWrt
soft-float SDK, portable atomics and dependency fixes with upstream licenses intact.

A two-connection SQLite profile replaces the upstream maximum of five connections
per writable database. Both profiles pass 245 state tests and the state doctest;
repeated device performance comparisons are in progress. A generic, bounded local
SOUL/MEMORY store and a cloud-submission boundary are implemented as independent
components, but are not yet attached automatically to agent prompts.

The normal MIPS launcher graph contains no `codex-tui` or `ratatui` dependency.
Experimental `exec-server` support preserves an executor path for remote tools.
The standalone `lodex-executor` image compiled at 75614760 bytes for remote-primary
setups. Native startup, environment RPC and MCP tool/resource smoke checks passed;
process/file/cancellation/recovery stress remains to qualify. Browser automation should normally
run through remote MCP services instead of a
browser installed on the router.

## Build for the tested MIPS target

Use an x86-64 Linux build host, Rust 1.98.0 with `rust-src`, and the matching
OpenWrt 23.05.3 SDK for little-endian MIPS32r2/soft-float. Rust's standard library
is cross-built by the wrapper. Do not use the host's native CPU/float ABI.

```sh
rustup toolchain install 1.98.0 --component rust-src
export LODEX_OPENWRT_SDK=/absolute/path/to/openwrt-sdk
tools/lodex/mips-cargo rustc \
  --manifest-path codex-rs/Cargo.toml --release \
  -p codex-lodex --bin lodex --offline -- \
  -C target-feature=-crt-static,+xgot -C codegen-units=256
```

Populate the Cargo dependency cache on the build host before an offline build.
The output is `.lodex-build/mips-target/mipsel-unknown-linux-musl/release/lodex`.
`LODEX_TARGET_DIR` can select a different external build cache.

For the executor-only image, use the same build command with
`-p codex-lodex-executor --bin lodex-executor`. It exposes the shared upstream
executor CLI directly: `lodex-executor --listen stdio` (without an `exec-server`
subcommand). Keep the full `lodex` image for local harness execution.

The executable dynamically uses the matching musl/libgcc/libatomic runtime.
Keep extra libraries in a private prefix; do not replace firmware libraries.
The working port uses `+xgot` and 256 final codegen units to avoid MIPS relocation
limits. Rust warns that `xgot` is an unstable target feature. Size-oriented builds
and further dependency reductions remain experiments, not default guarantees.

Run `lodex --help` for the current headless commands. `lodex app-server --listen
stdio://` exposes the upstream App Server protocol. Remote daemon installation on
MIPS currently requires manual managed-package layout; an automatic router
installer and portable release bundle are not yet available. MIPS currently needs
direct tool mode because the V8 code-mode host is unavailable on this target.

## Verification and measurements

The native test service passed Remote reconnect, model-driven file/Git changes,
concurrent shell/history workloads and process-restart recovery. The fork's host
MCP suite passed 377 top-level tests; host passes are separate from MIPS validation.

| Build | Executable bytes |
| --- | ---: |
| Full upstream MIPS CLI reference | 307349644 |
| Upstream headless App Server reference | 229765044 |
| Lodex v1.1 headless launcher | 236607592 |
| Lodex v1.2 SQLite candidate | 236607236 |
| Lodex v1.3 full headless image with executor CLI | 239722436 |
| Executor-only candidate, basic native smoke checks passed | 75614760 |

Executable size is not RAM usage. Early pool measurements reduced thread count
and median observed RSS peak, with slower request-group timing in that workload.
See the [performance results and limits](docs/lodex/performance.md), the [measurement scope and status](docs/lodex/CURRENT.md),
[reference measurements](docs/lodex/reference-results.json),
[MCP qualification matrix](docs/lodex/mcp-test-matrix.md),
[capability contract](docs/lodex/contract.md) and [implementation plan](docs/lodex/plan.md).

Host tooling tests:

```sh
python3 -m unittest discover -s tools/lodex/tests -v
RUSTUP_TOOLCHAIN=1.98.0 cargo test --manifest-path codex-rs/Cargo.toml \
  -p codex-lodex-local -p codex-lodex-policy
```

Device probes use synthetic workloads and isolated homes. They do not collect
production prompts, transcripts or credentials. CPU, RSS, swap, faults, startup,
tool latency and recovery all matter when comparing execution placement.

## License

Derived from Codex 0.166.0, upstream commit
`6ff1e1ca3b30fc8b914720681fcf5fe62e95f856`.
Upstream [Apache-2.0 licensing](LICENSE), [notices](NOTICE) and vendored dependency
licenses are retained. Changes target headless deployment and MIPS/OpenWrt support.
