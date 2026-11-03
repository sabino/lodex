# Implementation status

Experimental MIPS/OpenWrt fork based on Codex 0.166.0,
upstream commit 6ff1e1ca3b30fc8b914720681fcf5fe62e95f856.

## Verified on hardware

The tested device is an Omega2+ with one MT7688/MIPS24KEc core, little-endian
MIPS32r2, soft-float, OpenWrt23.05.3 and approximately128MiB RAM. External SD
storage and swap were available. Other routers/firmware ABIs are not qualified.

The v1.1 headless launcher passed managed Remote startup and stop/start reconnect,
CLI administration, concurrent shell/history and four-thread process-restart
recovery. Existing authentication and enrollment identity were preserved. The
stock executable remains available for rollback on the test device.

Basic MCP stdio/HTTP calls and resource reads, actual model-driven file edits and
Git operations passed in the first candidate. The fork's complete host MCP client
suite now passes377 top-level tests, including OAuth/session recovery and remote
executor paths. The remote-executor fixture uses matching stock Codex0.166.0.
These host results do not prove all those paths on MIPS. See mcp-host-results.json
and mcp-test-matrix.md for the exact scope.

The v1.2 two-connection SQLite candidate compiled, passed native CLI gates and is
undergoing repeated hardware comparison. All245 state tests and one doctest pass
with both the new limit and upstream's five-connection limit. The v1.3 executor entry point compiled and passed ten native CLI gates. A separate
Linux primary then passed real model-driven shell and MCP tool/resource tests
through the device executor. An executor-only image compiled at 75614760 bytes as a smaller remote-primary
option. Native CLI, environment RPC, remote MCP, direct process/file and
cancellation checks passed. Capability-token rejection and process-session
recovery after abrupt WebSocket stream loss also passed; this kept SSH and the
executor alive. See executor-qualification.json for the exact scope. Prolonged
stress, auth refresh and broader failure modes remain pending.
Preliminary pool results show a resource/latency tradeoff; see
performance.md. A clean alternating pool comparison remains required.

A balanced three-mode startup/MCP comparison now passed three measured trials per
mode after warmup, with no overlapping hardware jobs and the managed service
stopped during measurement. Median complete fixture times were 32.64/11.14/7.58
seconds for local/full-executor/small-executor placement, and device RSS peaks
were 61896/53556/33044 KiB. Remote controller RSS was about 310 MiB. This is a
synthetic fixture result, excluding model inference and voice. See performance.md
and placement-balanced-results.json. The separate SQLite pool comparison still
needs clean alternating confirmation.

## Cloud task and API adapter

The headless `cloud` command now submits one explicit environment/ref/task using
saved ChatGPT authentication and retrieves bounded status/assistant text. A live
read-only cloud worker completed with the expected marker. The API subcommand
separately implements hosted/self-hosted session creation, minimal status,
paginated assistant results and actual cancellation-event submission. It requires
an API key and does not substitute the saved subscription login for that key.
Twenty-two cloud protocol/CLI tests and thirteen payload-policy tests pass, with
package Clippy warnings denied. Both native images passed cloud/API preflight
and saved-login cloud readback; a real local MIPS `exec` turn passed against a
synthetic API response server over SSH forwarding. The full candidate passed
twelve CLI gates and App Server shell execution. Live API keys were unavailable,
so this does not qualify hosted API execution or device registration. Exact native gates and remaining live API limits
are recorded in [cloud-results.json](cloud-results.json). Command examples and
credential/data boundaries are in [worker-routing.md](worker-routing.md).

## Remaining work

- Repeated comparable device CPU/RSS/swap/fault/startup/tool-latency measurements,
  including loaded conversations, long output and concurrent MCP workloads.
- Native extended MCP/OAuth/cancellation, auth refresh and fault/recovery tests.
- A selectable hosted-cloud primary controlling a device executor/MCP endpoint,
  while retaining the local harness. A separate Linux primary passed real
  model-driven shell and MCP tool/resource tests; loaded-turn placement benchmarks
  remain beyond the measured synthetic startup fixture.
- Live hosted API-harness/device registration once appropriately scoped API keys
  are configured. API event streaming, durable follow-up submission and real auth
  expiry/recovery remain; the legacy task API exposes no verified cancel method.
  Explicit new cloud tasks are not migration of a running local thread.
- Agent profile selection/prompt policy, atomic initialization recovery and
  SD-failure tests. Local storage CLI, writer attribution and process-kill update
  recovery are implemented. No automatic profile injection or sync is enabled.
- Further compile-time dependency, initialization, telemetry and provider cuts,
  each justified by regression tests and measurements.
- Portable OpenWrt packaging and qualification beyond the first hardware/ABI.

Generic bounded local SOUL/MEMORY storage has ten default-library host tests,
eleven with its CLI feature, and native CLI/concurrent-writer/process-kill
qualification. A standalone profile utility is under 1 MiB, and the full launcher
has a profile subcommand. Main-model prompt loading is not enabled. Worker policy
has thirteen host tests, including a 64 KiB submission bound and exclusion of
ambient starting diffs. Neither component automatically supplies profile context
to an agent or worker.

This is a development preview, not a completed compatibility or performance claim.
