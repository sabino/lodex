# Local profile storage

`codex-lodex-local` provides bounded local SOUL/MEMORY files and a storage CLI.
The storage library has only `sha2` as a normal default dependency; the optional
`cli` feature adds Clap. There is no network client or profile serialization API.
The launcher exposes `lodex profile`; a separate `lodex-profile` utility performs
the same operations without starting an agent runtime. Main-model prompt loading
and automatic worker export are not enabled.

## Commands

Choose an absolute private root outside the checkout. Its parent must exist.
Use `LODEX_PROFILE_ROOT` or pass `--root` before the storage subcommand:

```sh
export LODEX_PROFILE_ROOT=/mnt/storage/private/lodex-profiles
lodex profile init main
lodex profile inspect main
lodex profile paths main
```

`paths` prints the local SOUL.md and MEMORY.md locations for a human editor.
Edit SOUL.md to describe the desired personality. MEMORY updates should use the
locked writer interface to participate in conflict detection. `inspect` prints
byte counts, a SHA-256 revision and last managed writer metadata, without printing
profile text. For an explicit local read, use `show main soul`, `show main memory`
or `show main previous-memory`. `show` intentionally writes the selected private
text to stdout; it is not a remote/model operation.

For a memory update, copy the `revision` from `inspect` and supply it explicitly:

```sh
lodex profile write-memory main --expect REVISION_FROM_INSPECT --writer operator < new-memory.md
```

The command reads at most 32,769 bytes, requires UTF-8 and rejects content over 32 KiB.
It prints only the new revision. A stale revision fails without overwriting newer
content. Repeating an identical write is a no-op and preserves the last writer.
Replace `lodex profile` with `lodex-profile` for the small standalone utility.
Neither command automatically feeds a model, selects an agent profile, changes
AGENTS.md precedence or submits a cloud task.

## Files and transaction recovery

An absolute external root contains private named profile directories. Names
reject traversal. Newly created directories/files use 0700/0600; existing public
roots are rejected rather than silently chmodded. Root/profile/file symlinks are
rejected. Another privileged process changing paths concurrently is outside this
cooperative local-store boundary.

Reads bound SOUL to 8 KiB and MEMORY to 32 KiB and require UTF-8. Debug output
contains lengths rather than contents. An update holds the profile's kernel file
lock and checks the expected SHA-256 revision. It fsyncs a staged previous version
and a bounded intent record before atomically replacing MEMORY.md. It then commits
the previous version and attribution, removes intent/staging files and syncs the
parent directory. The record contains caller writer ID, original process ID,
wall-clock milliseconds and before/after revisions; it contains no memory text.
The old memory remains in a private bounded staging file until the commit finishes.

Opening a profile under its lock completes interrupted commits whose MEMORY hash
matches the new revision, or discards uncommitted preparation if it still matches
the old revision. The last committed previous version/writer survive an aborted
update. Exact reserved `.write-PID-N` staging files are cleaned after recovery.
Conflicting/corrupt transaction state produces an error and preserves evidence;
it does not guess which external edit to discard. On an I/O error after rename,
reread before retrying because the new memory may already be committed.

Attribution is caller supplied, not authentication or a tamper-proof audit chain.
`last_write_current` shows whether the last managed record matches the current
memory revision. A manual editor can bypass the cooperative lock and attribution.
Legacy library `write_memory` records a process-based writer; applications should
use `write_memory_as` with an explicit bounded writer ID. Root processes are not
confined from reading these local files through other tools.

Ten default-library host tests and eleven with the CLI feature pass. One ignored
child entry is exercised by the parent process-kill test. All-target/all-feature
Clippy passes with warnings denied for this package. Native CLI checks include
UTF-8 updates, attribution, non-content inspection, invalid/stale/oversized input,
concurrent writer processes and permissions. Native kill tests cover preparation,
intent, a partial temporary write, committed MEMORY, backup commit and attribution
commit. See local-profile-results.json for exact images and evidence. These are
process-death tests, not power-loss or failing-SD durability proof. Profile
initialization is create-only but does not yet have atomic-directory recovery.

The `fault-injection` feature and `crash-recovery` example are lab-only. Normal
builds do not include the rendezvous hooks. The native example kills only its own
synthetic child at six explicit boundaries in a new test root.

Build the small utility using the tested OpenWrt SDK setup from the main README:

```sh
tools/lodex/mips-cargo rustc --manifest-path codex-rs/Cargo.toml --release \
  -p codex-lodex-local --features cli --bin lodex-profile --offline -- \
  -C target-feature=-crt-static,+xgot -C codegen-units=256
```

The lab recovery executable uses the same command with `--features fault-injection
--example crash-recovery` instead of `--features cli --bin lodex-profile`. Run it
only with a new absolute synthetic root. `tools/lodex/profile_probe.py --help`
documents the combined native CLI and recovery gate; its Python runs on the
controller and uses pinned SSH plus BusyBox on the device.

## Model and cloud boundary

Storage is independent of transmission. An explicit policy for sending selected
context to the primary cloud model is still required. No automatic prompt
injection or cloud synchronization is enabled. Cloud workers receive only explicit
task payloads and never inherit profile snapshots automatically. A CLI for local
files does not by itself integrate personality into agent execution.
