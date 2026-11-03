# Lodex implementation plan

Implementation is active. Lodex is an independent MIPS/OpenWrt runtime.

1. Baseline and compatibility inventory: retain native full CLI as rollback;
   measure sections, RSS, swap, faults, threads and latency. Stock headless build
   completed: 229765044 bytes versus 307349644, 25.24% smaller. Matched initial
   idle observations are in reference-results.json; loaded-use savings remain
   unverified. Management/re-exec dispatch is now implemented and qualified.
2. Build a minimal compatible headless entry point retaining App Server/Remote,
   auth and MCP administration; eliminate terminal UI and unused CLI modes at
   compile time. Preserve local execution and add cloud delegation through a small adapter.
   Compare remote-primary control of a device executor/MCP endpoint as well.
3. Instrument and reduce initialization: SQLite pools/caches, optional DB opening,
   telemetry and background catalogs, thread/runtime settings. One change per
   measured variant. Keep persistence and MCP compatibility.
4. Bounded generic profile loading and local memory interfaces after privacy
   semantics are resolved. Explicit worker payload routing with canary tests.
5. Examine dependency replacements, TLS duplication, serializers and generic code
   size. Retain upstream implementations unless a replacement wins measured
   memory/CPU tests and protocol regression tests.
6. Physical Omega qualification, external MCP fixtures, cloud worker proof,
   prolonged operation and recoverable production cutover.

Build recipe: tools/lodex/mips-cargo. Set LODEX_OPENWRT_SDK to the verified
OpenWrt 23.05.3 SDK directory; run from codex-rs. It expects Rust 1.98/rust-src.
Current known link settings: rustc --release -p codex-lodex
--bin lodex --offline -- -C target-feature=-crt-static,+xgot
-C codegen-units=256. The Lodex launcher preserves the daemon's re-exec grammar.

Port dependencies are vendored under vendor with licenses intact. Upstream
commit is recorded in baseline.json. Original MIPS port diffs are retained in
port-provenance; workspace paths in this fork point to ../vendor instead.

Development profiling:
- Stream tools/lodex/sample-proc.sh to a target shell, supplying a PID, count
  and interval; capture stdout on the host.
- Parse with tools/lodex/footprint.py CAPTURE.
- Run parser tests with python3 -m unittest discover -s tools/lodex/tests -v.
Target sampling requires only BusyBox shell/awk, not Python or extra daemons.

Lodex v1.6 is installed experimentally as the managed Remote service, with stock
rollback retained. It passed reconnect after managed stop/start, native CLI,
concurrent shell/history and process-restart recovery. See CURRENT.md and
candidate-v1.1-results.json for exact evidence and remaining limitations.

The two-connection SQLite variant compiled and passed native gates. All245 state tests and one
doctest pass with both its feature and upstream default5. Three unchanged-image
baseline trials completed before evaluating this variant. Use
tools/lodex/native_benchmark.py with an explicit pinned SSH helper, immutable
binary path, credential-free fixture config, new remote/output roots and private
runtime library path. It runs synthetic four-thread shell/history workloads,
then collects five process samples. It makes no model calls. Full request traces
remain local/ignored; sanitized results are in pool-baseline-results.json.

Broader MCP protocol qualification, cloud worker execution and local profile
runtime integration remain required. Automatic transmission of local private profiles is not enabled.
