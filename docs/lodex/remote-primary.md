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
full local harness image. Native process/file integrity, stdin retries,
interruption, termination and restart passed. A capability-token listener behind
pinned SSH rejected missing/wrong credentials and Origin-bearing requests, and
resumed the same process session after abrupt WebSocket stream loss without
duplicating stdin. See [the qualification record](executor-qualification.json).
This does not test SSH/Wi-Fi loss, power failure, hosted Cloud or auth refresh.

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

## Synthetic executor qualification

`tools/lodex/executor_probe.py` takes a pinned SSH helper, an immutable lab image,
a new absolute remote directory, a private runtime library path and a create-only
output file. It makes no model calls and does not use the managed service home.
`--full-cli` tests the full image's `exec-server` entry point instead.

`tools/lodex/executor_websocket_probe.py` tests a separately started, synthetic
capability-token listener through a loopback SSH forward. Its only extra package,
`websockets==15.0.1`, belongs in a controller-side virtual environment. No Python
package is required on the device. Supply a synthetic token file with
`--token-file`; do not use an account token. The test leaves listener/tunnel
lifecycle to its caller, which must clean up those owned processes afterward.

Uploads, native benchmarks and both probes share a fail-fast controller lock.
Use the same `--hardware-lock` path for all jobs targeting a device. Other native
commands can run under `python3 tools/lodex/hardware_job.py -- COMMAND ARGS...`.
This is cooperative exclusion on one controller, not coordination across hosts
or automatic remote cleanup. Inspect owned jobs after an abnormal termination.

A [balanced native startup/MCP comparison](placement-balanced-results.json)
passed three measured trials per placement after warmup. It includes device and
host process counters and a complete fixture clock. It favors the smaller
executor for that workload while showing the remote controller's memory cost;
it does not measure cloud inference or voice.
