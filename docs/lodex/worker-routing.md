# Explicit worker routing

The initial codex-lodex-policy component is implemented and host-tested. It is
not yet wired into the CLI/cloud backend. Main orchestration remains local by
default. No automatic personality or memory loading has been enabled.

The cloud-task input accepts only environment_id, an optional git_ref, and task.
There are no parent-thread, transcript, attachment or private-profile fields;
unknown fields are rejected. Task text is limited to 32 KiB, identifiers to
256 bytes, and the stream reader caps encoded requests at 64 KiB before parsing.
Task text is omitted from Debug output. Thirteen unit cases and Clippy with warnings
denied pass. This is the adapter's payload boundary, not semantic DLP or a
restriction on arbitrary file/network operations by an unsandboxed root process.

The new-task wire encoder accepts only that validated task. It produces one user
message, one selected environment/ref and no automatic attachments. Omitted refs
use the constant `main`, not local Git discovery. Encoded output is bounded to
64KiB before any network submission, including JSON escaping. A child-process
canary test verifies that CODEX_STARTING_DIFF is not forwarded: the upstream
cloud-task UI helper can attach this ambient diff, so it must not be reused as
the new adapter's submission constructor.

Local execution remains supported. A remote/cloud primary controlling an
authenticated device executor/MCP endpoint is an additional placement option to
implement and benchmark; it does not mean that an LLM runs on the device CPU.

## Verified upstream boundary

Official CLI reference: https://learn.chatgpt.com/docs/cli/reference
Cloud environments: https://learn.chatgpt.com/docs/environments/cloud-environments
Environment modes: https://learn.chatgpt.com/docs/environments/modes

The upstream noninteractive cloud exec/status/list/diff/apply paths are separate
from its UI. codex-cloud-tasks-client provides a UI-free CloudBackend interface.
New task submission takes environment, prompt, git ref, QA mode and attempt count.
Do not silently replace an active local thread with a newly created cloud task.
Account/environment availability and cancellation capabilities remain to verify;
the pinned CloudBackend API has no cancellation method exposed.

The future adapter must use the existing ChatGPT auth provider and trusted
backend origin rules. Never forward saved ChatGPT credentials to an arbitrary
URL. Do not copy the cloud-task UI's error.log behavior that records account
metadata and raw response bodies into the small appliance controller.

## Memory issue to address before adding polling

The current cloud-tasks-client summary path obtains typed task details plus a
raw response string, parses the same body again into serde_json::Value and clones
status-display data. It can retain transcript/diff content merely to report status.
The underlying HTTP request type already supports response_body_limit_bytes.
Use bounded, minimal summaries/results rather than importing this polling path
unchanged. Verify schema compatibility against fixture responses and the live
account before replacing it. Larger results should produce an explicit limit
error or an explicit retrieval path, not silently truncated successful output.
