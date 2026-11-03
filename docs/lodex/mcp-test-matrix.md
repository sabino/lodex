# MCP qualification matrix

Native qualification remains incomplete for the Lodex candidate. The fork's host
client suite has now passed; see the exact target counts in mcp-host-results.json.
Reusing upstream source preserves implementation coverage, not proof of passing
the new build/runtime. Local fixtures do not need production credentials.

| Area | Upstream test target or source |
|---|---|
| Legacy and newer stdio discovery, fallback, server requests | rmcp-client/tests/mcp_2026_stdio.rs |
| Child cleanup and cancellation with an in-flight call | rmcp-client/tests/process_group_cleanup.rs |
| Framing and message limits | rmcp-client/tests/stdio_message_limits.rs, mcp_2026_message_limits.rs |
| Resources and templates | rmcp-client/tests/resources.rs |
| HTTP session recovery and reconnect | rmcp-client/tests/streamable_http_recovery.rs |
| OAuth startup and refresh during calls | rmcp-client/tests/streamable_http_oauth_startup.rs, streamable_http_oauth_tool_call.rs |
| Credential-store pinning | rmcp-client/tests/streamable_http_oauth_store_pinning.rs |
| New HTTP/SSE discovery and OAuth | rmcp-client/tests/mcp_2026_sse_discovery.rs, mcp_2026_oauth_discovery.rs |
| Runtime events and notification handling | rmcp-client/tests/mcp_events.rs |
| Elicitation through App Server | app-server/tests/suite/v2/mcp_server_elicitation.rs |
| Remote executor MCP routing | app-server/tests/suite/v2/executor_mcp.rs |
| CLI config/auth secret redaction | reused cli/src/mcp_cmd_tests.rs |
| Native end-to-end | isolated Omega App Server, external stdio/HTTP fixtures; status/list, tool/call, resource/read |

Run reference transport tests on the build host, then smoke the actual MIPS
binary against fixture servers. Keep production OAuth credentials out of fixtures.
Test advertised capabilities and protocol-era behavior, not just one tool call.
Record measured buffer/connection bounds and failure results before shrinking
discovery caches or replacing the MCP transport library.

## Host client qualification

The complete codex-rmcp-client test invocation passed with its vendored SDK.
This covers legacy/newer discovery, stdio and HTTP limits, resources, process
cleanup, events, OAuth startup/refresh/store pinning and HTTP session recovery.
The remote-executor cases use the installed matching stock Codex 0.166.0 as their
exec-server fixture. Initial invocation failed those three cases because the
fixture executable was absent; rerunning with CARGO_BIN_EXE_codex set resolved
that setup failure. Do not label this as a Lodex executor CLI pass. Its minimal
entry point is being restored and still needs its own native qualification.

Ignored entries are named child-process helpers called by parent tests. See the
sanitized per-target report for their counts. Raw synthetic output stays ignored
under .lodex-build. Host passes do not establish identical MIPS process, network,
timeout, OAuth or cancellation behavior.

## Reference baseline recorded

The stock headless MIPS binary passed initialize, command/exec, stdio MCP status
and tool call/resource read, and the same MCP operations over stateless HTTP
through an SSH tunnel to a laptop fixture. See reference-results.json. This
establishes test infrastructure and a reference; it does not qualify the new
Lodex launcher or the remaining OAuth/SSE/cancellation/version cases.
