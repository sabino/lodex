# Remote primary with device execution

A separate Codex harness can perform orchestration while a MIPS device runs the
executor and its tools. Local harness execution remains available. Model inference
location, harness location and tool location are independent choices.

A real gpt-6.1-sol turn from a separate Linux host executed one shell tool through
Lodex v1.3 on OpenWrt. It returned exit0 and the expected `mips` architecture.
The same controller initialized a thread, discovered a remote MCP server, called
its echo tool and read its resource through the device executor. See
[the exact qualification scope](remote-primary-results.json).

This qualifies a remote Linux primary over SSH. It does not yet qualify an
OpenAI-hosted Cloud environment, voice streaming, every model/tool mode, or an
overall performance improvement.

## Controller configuration

The pinned upstream implementation reads environments.toml in CODEX_HOME.
A controller can launch a device executor over authenticated SSH stdio, avoiding
a separately exposed executor port. Replace the example host, key configuration,
installation prefix, runtime library path and private state directory for the
actual device. Use a pinned SSH host identity and noninteractive authentication.

```toml
default = "router"
include_local = false

[[environments]]
id = "router"
program = "ssh"
args = [
  "-T", "-o", "BatchMode=yes", "root@router",
  "env CODEX_HOME=/mnt/storage/lodex/executor-state LD_LIBRARY_PATH=/mnt/storage/lodex/lib /mnt/storage/lodex/bin/lodex exec-server --listen stdio"
]
```

The executor state directory and matching private runtime must already exist.
The functional test used a separate, credential-free executor home. It used an
explicit direct-tool model catalog for the controller. Other tool modes and
firmware layouts require their own qualification. The experimental executor-only image compiled as a smaller alternative to the
full local harness image. Its native startup, environment RPC and MCP tool/resource
smoke checks passed; broader process/file/cancellation/recovery testing remains.

Specify the device-native working directory when selecting an environment or
calling a shell tool. A controller's local working directory may not exist on the
device. The successful CLI model test explicitly supplied the device workdir.
App Server clients can select an environment and its cwd in thread/start and
turn/start. The separate thread/shellCommand API is intentionally a local-host
escape hatch; it is not the remote model/tool path.

## MCP and private context

The remote-MCP test used an external deterministic HTTP fixture reached through
an SSH reverse tunnel. This exercised native executor HTTP routing as well as
tool and resource operations. No Python HTTP server or extra Python package was
installed on the device. Local fixture failures are recorded separately from
runtime/protocol failures.

Keep local SOUL/MEMORY files local. Their storage component is not automatically
injected into controller prompts or worker submissions. Supplying selected
context to a primary model requires an explicit transmission policy. Remote
workers must not automatically inherit the parent transcript or private profiles.
This payload boundary is not a restriction on arbitrary root-level tool access.

A hosted cloud primary additionally needs a reachable, authenticated device
transport and a supported environment/account setup. Neither the user's SSH
sign-ins nor local files are automatically transferred into cloud tasks. A new
cloud task is not migration of an existing local thread.

Compare device CPU/RSS/swap/faults, complete startup and turn/tool latency,
connection recovery and representative MCP concurrency before choosing a default
placement. A functional route alone is not a performance result.
