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
with both the new limit and upstream's five-connection limit. The v1.3 executor
CLI entry point is being rebuilt; its own native qualification remains pending.

## Remaining work

- Repeated comparable device CPU/RSS/swap/fault/startup/tool-latency measurements,
  including loaded conversations, long output and concurrent MCP workloads.
- Native extended MCP/OAuth/cancellation, auth refresh and fault/recovery tests.
- A selectable remote/cloud primary controlling a device executor/MCP endpoint,
  while retaining the local harness and comparing end-to-end performance.
- Explicit cloud-task selection, account/environment qualification, bounded
  results and verified cancellation semantics. New tasks are not thread migration.
- Local profile CLI/runtime integration, writer attribution and crash recovery.
  No automatic private profile injection or cloud synchronization is enabled.
- Further compile-time dependency, initialization, telemetry and provider cuts,
  each justified by regression tests and measurements.
- Portable OpenWrt packaging and qualification beyond the first hardware/ABI.

Generic bounded local SOUL/MEMORY storage has seven host tests and a native SD
write/lock/revision/reopen qualification. Worker policy has thirteen host tests,
including a 64KiB submission bound and exclusion of ambient starting diffs.
Neither component is automatically attached to a live agent yet.

This is a development preview, not a completed compatibility or performance claim.
