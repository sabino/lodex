# Lodex capability contract

Status: implementation started. Required capabilities below are acceptance
criteria, not claims that the new fork has passed them.

## Runtime and consumer boundary

Lodex provides Codex-compatible headless CLI/App Server, Remote/mobile transport,
authentication and refresh, thread persistence/resume, core tools and MCP clients.
Applications wrap those interfaces and supply their own integrations and
personalities. The runtime stays independently useful.

Keep protocol behavior based on the pinned upstream release. Keep public APIs,
wire data, identifiers and enrollment behavior compatible; document supported
and intentionally omitted CLI commands. A stock headless server is not yet a
drop-in replacement for the daemon's codex app-server invocation.

## MCP is required

Preserve upstream negotiated protocol support, not a tools/call-only JSON shim.
The verification matrix must include:

- Local stdio process startup, framing, stdout/stderr separation and shutdown.
- Streamable HTTP, SSE/session handling, reconnect and negotiated version behavior.
- Bearer authentication, OAuth credential refresh and failure behavior.
- Initialization/capability negotiation; legacy and newer revision paths supported
  by the pinned upstream client; do not assume latest protocol behavior is identical.
- Tools/list and tools/call, pagination and list-change notifications.
- Resources/templates/read and prompts/list/get where the server advertises them.
- Concurrent requests, progress, cancellation, timeout and error propagation.
- Supported elicitation/server requests, roots and sampling behavior according
  to the negotiated version and upstream capability surface.
- Bounded discovery/results/concurrency, backpressure and lazy connections;
  size limits produce explicit errors rather than silent truncation/corruption.

Browser access normally uses an external MCP endpoint or remote browser executor.
No Chromium, desktop GUI or general JavaScript browser runtime is required on Omega.
A local HTML/text fetcher, if added, must disclose that it is not a full browser.
MCP endpoints execute tools remotely; supplying their tool descriptions to the
main cloud model does not relocate the local agent harness.

## Personality and memory

Provide generic, bounded profiles with human-editable SOUL.md, MEMORY.md and
agent instructions in a private, external local directory. Keep the standard
Codex instruction hierarchy; do not replace the model's base prompt accidentally.
The consumer supplies its own content; no application personality is built in.

Storage location and prompt transmission are separate controls. Selected context may reach the main cloud model only under an explicit
prompt-transmission policy. No automatic injection or cloud export is enabled.

Memory writes must be local, atomic, bounded, recoverable and attributable.
Remote worker submissions use explicit task payloads: no automatic parent
transcript, repository-wide copy, SOUL/MEMORY attachment or credential forwarding.
A prompt instruction alone is not a security boundary; enforce routing at the
payload construction layer and test with canary values.

## Execution placement

Retain a complete local headless harness. Add a selectable remote/cloud primary
that can control the device through authenticated executor/MCP interfaces when
reachable, plus explicit local versus cloud/remote worker execution with
cancellation, status and bounded results. Verify what the account and upstream Cloud APIs actually support.
Cloud tasks and in-place migration of an existing local thread are different.
Do not label a new cloud task as a migrated thread without demonstrating parity.

Keep a small noninteractive cloud-task client/controller if needed; remove its
terminal UI, not the entire capability. Cloud worker environments receive only
selected task inputs and return bounded results. No automatic local-memory sync.
Do not enable automatic offloading until the data boundary and costs are defined.

## Acceptance

Before production replacement: Remote pairing/reconnect, auth refresh, shell
and file/Git tasks, thread resume after restart, MCP conformance cases, clean
cancellation, SD failure behavior and rollback. Compare idle, short-turn,
file-edit, long-output and concurrent MCP workloads on the physical Omega.
No claimed memory reduction without comparable live measurements. Compare local,
remote-primary/device-executor and cloud-task placement on the same workloads,
including device CPU/RSS/swap, startup, faults, total turn/tool latency and
connection-loss recovery. Model inference location is separate from harness and
tool execution location.

## Realtime consumer interface

Keep the existing experimental App Server realtime interface available to
consumers that wrap Lodex: thread/realtime/start, appendAudio, appendText,
appendSpeech, stop and listVoices, plus their event notifications. Removing the
terminal audio UI must not remove this protocol bridge. Audio device access,
wake detection, speaker output and application voice policy remain consumer
responsibilities. These methods are present in the source; streaming and quota
behavior still need isolated protocol qualification before being claimed tested.
