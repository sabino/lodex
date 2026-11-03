# Explicit worker routing

The codex-lodex-policy component and bounded noninteractive cloud-task/API
adapters are implemented. A saved-login cloud worker completed a read-only task;
native qualification and API credentials are tracked separately. Main orchestration
remains local by default. No automatic personality or memory loading has been enabled.

The cloud-task input accepts only environment_id, an optional git_ref, and task.
There are no parent-thread, transcript, attachment or private-profile fields;
unknown fields are rejected. Task text is limited to 32 KiB, identifiers to
256 bytes, and the stream reader caps encoded requests at 64 KiB before parsing.
Task text is omitted from Debug output. Thirteen policy cases and the cloud adapter's protocol/CLI cases pass, with
Clippy warnings denied for these packages. This is the adapter's payload boundary, not semantic DLP or a
restriction on arbitrary file/network operations by an unsandboxed root process.

The new-task wire encoder accepts only that validated task. It produces one user
message, one selected environment/ref and no automatic attachments. A `git_ref` is required at submission; omitted refs are rejected before network
access. The adapter does not infer a branch from local Git state. Encoded output is bounded to
64KiB before any network submission, including JSON escaping. A child-process
canary test verifies that CODEX_STARTING_DIFF is not forwarded: the upstream
cloud-task UI helper can attach this ambient diff, so it must not be reused as
the new adapter's submission constructor.

Local execution remains supported. A remote/cloud primary controlling an
authenticated device executor/MCP endpoint is an additional placement option;
the separate Linux primary and bounded startup fixture are qualified. Hosted
Cloud and loaded-turn tests remain; it does not mean that an LLM runs on the device CPU.

## Verified upstream boundary

Official CLI reference: https://learn.chatgpt.com/docs/cli/reference
Cloud environments: https://learn.chatgpt.com/docs/environments/cloud-environments
Environment modes: https://learn.chatgpt.com/docs/environments/modes

The upstream noninteractive cloud exec/status/list/diff/apply paths are separate
from its UI. codex-cloud-tasks-client provides a UI-free CloudBackend interface.
New task submission takes environment, prompt, git ref, QA mode and attempt count.
Do not silently replace an active local thread with a newly created cloud task.
The pinned CloudBackend API has no cancellation method exposed. The adapter
reports that limitation explicitly and does not mark a task stopped. Live
account/environment and native gates are recorded separately.

The adapter uses the existing ChatGPT auth provider and trusted backend origin
rules, with one bounded explicit-401 refresh retry. Never forward saved ChatGPT credentials to an arbitrary
URL. Do not copy the cloud-task UI's error.log behavior that records account
metadata and raw response bodies into the small appliance controller.

## Memory issue to address before adding polling

The current cloud-tasks-client summary path obtains typed task details plus a
raw response string, parses the same body again into serde_json::Value and clones
status-display data. It can retain transcript/diff content merely to report status.
The underlying HTTP request type already supports response_body_limit_bytes.
The Lodex adapter instead deserializes a minimal typed status projection once,
without retaining parent prompts or diffs. It preserves the selected assistant
text result path with separate raw-body and output-text bounds. Verify schema compatibility against fixture responses and the live
account before replacing it. Larger results should produce an explicit limit
error or an explicit retrieval path, not silently truncated successful output.

## Noninteractive CLI

The headless launcher exposes `lodex cloud`; the optional standalone binary is
`lodex-cloud` with the same subcommands. Both reuse the shared HTTP transport and
its proxy/CA policy. Credentials, response bodies and request URLs are not logged.

With an existing ChatGPT login:

```sh
lodex cloud capabilities
lodex cloud environments --repo owner/repository
lodex cloud exec --env ENVIRONMENT_ID --branch EXPLICIT_REF < selected-task.txt
lodex cloud status TASK_ID
lodex cloud result TASK_ID
```

`submit` alternatively reads one bounded JSON object with `environment_id`,
`git_ref` and `task` from stdin. `exec --attempts` accepts only 1. Results contain
assistant text, not user prompts or generated diffs. This adapter deliberately
omits diff/apply and automatic ambient starting-diff attachment. The original
local `exec` and MCP client interfaces remain available. Selecting a cloud task
creates a new task; it does not migrate the current local thread or connect that
task to a local device executor.

Authentication uses the saved ChatGPT identity, including the upstream refresh
provider. Only HTTPS chatgpt.com/chat.openai.com backend origins are accepted;
redirects are disabled. Whole operations have a 30-second bound. Metadata is
limited to 256 KiB, result bodies to 1 MiB and selected assistant text to 64 KiB.
An oversized response fails explicitly. An ambiguous POST outcome is reported
without an automatic duplicate submission. The legacy task API has no verified
cancellation endpoint; `cancel` returns an explicit unsupported error.

## Separate API-backed hosted harness

The API adapter is selected explicitly with `lodex cloud api`. It requires
`OPENAI_API_KEY` in the controller's environment and does not load the ChatGPT
auth provider or use ChatGPT tokens as API credentials. Normal local
`lodex exec` retains upstream API-provider support as well.

```sh
lodex cloud api capabilities
lodex cloud api create < selected-api-task.json
lodex cloud api status SESSION_ID
lodex cloud api result SESSION_ID
lodex cloud api result SESSION_ID --after NEXT_AFTER
lodex cloud api cancel SESSION_ID
```

An explicit creation payload is:

```json
{
  "agent": { "model": "EXPLICIT_MODEL" },
  "environment": { "type": "openai_hosted" },
  "input": "The selected task only"
}
```

Supported environment variants are `none`, `openai_hosted`, and
`self_hosted` with an explicit absolute `workspace_directory` and optional
`capability_directories`. Optional `agent.instructions` is explicit input,
bounded to 8 KiB; the runtime never reads local SOUL/MEMORY for submission.
Unknown fields are rejected. This initial adapter does not expose arbitrary
Agents API settings, follow-up input, event streaming or hosted MCP definitions.
The local full harness and device executor retain their upstream MCP interfaces.

API requests use only https://api.openai.com/v1 with `OpenAI-Beta: agents=v1`;
redirects are disabled. Creation/status/result limits match the legacy adapter.
Each result request is one bounded page of up to 100 items. Only assistant
output text is returned, with message/turn status and phase; tool arguments,
reasoning and user input are discarded. `has_more` and `next_after` make
pagination explicit. Session `idle` is not interpreted as a successful turn.

API `cancel` submits `agent.session.input.cancel` to the session events endpoint.
The response acknowledges event submission, not a terminal cancellation outcome;
check status/items afterward. It never deletes the session. Failed or ambiguous
mutations are not blindly retried. Follow-up input and durable idempotency need
separate implementation before reliable send/retry support can be claimed.

For a self-hosted session, creation returns the environment ID and remote URL.
Pass the returned URL unchanged to the standalone executor's `--remote` and the
ID to `--environment-id`. That executor requires a separate restricted
environment key, configured as `CODEX_API_KEY` or through its supported rotating
credential interface. Keep the broader application key on the controller.
The actual API-backed cloud-harness/device connection remains unqualified until
both appropriately scoped keys are configured locally. Existing subscription
cloud tasks do not substitute for this API registration flow.

Official references checked 2026-11-03:

- [Agents API self-hosted environments](https://developers.openai.com/api/docs/guides/agents-api/environments/self-hosted)
- [Running sessions and cancellation events](https://developers.openai.com/api/docs/guides/agents-api/sessions)
- [Create session schema](https://developers.openai.com/api/reference/resources/beta/subresources/agents/subresources/sessions/methods/create)
- [Paginated session items](https://developers.openai.com/api/reference/resources/beta/subresources/agents/subresources/sessions/subresources/items/methods/list)

## Qualification scope

See [cloud-results.json](cloud-results.json). A saved-login read-only worker
completed on an available cloud environment with an explicit ref. Both native
images subsequently retrieved its completed status and expected assistant result.
Twelve full-launcher CLI gates, native cloud/API preflight and App Server shell
execution passed. A native local `exec` turn passed against synthetic Responses
SSE with one API-key-authenticated request and no ChatGPT account header.
The direct fixture route timed out before a request reached the mock server;
the successful fixture used reverse forwarding over the existing SSH connection.
No firewall, router or Wi-Fi settings changed. These are functional checks, not
loaded-turn, voice, WAN or overall resource-performance measurements.

The standalone cloud helper is 56895940 bytes; the full v1.5 launcher is
240249956 bytes. The helper still uses upstream configuration/auth dependencies;
it is not a minimal HTTP-only client. Neither image size establishes RSS or a
performance improvement. The working managed Remote launcher remains v1.1 while
v1.5 is an isolated candidate. A standalone helper can coexist without replacing
the managed server or its enrollment identity.

API wire/limit fixtures and missing-key preflight pass. No live application or
restricted executor key was configured, so hosted API execution and the actual
API-harness/device-executor connection remain unqualified. Live auth expiry,
long-running recovery and API event streaming remain separate work.
