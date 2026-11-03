# Local profile storage

`codex-lodex-local` is a small, synchronous filesystem component with only `sha2`
as a normal dependency. It has no networking or serialization API. It is not yet
wired into the launcher or model prompts. Consumers can use its eventual generic interface without putting application
features or a personality in the runtime.

An absolute, dedicated external root contains private named profile directories
with human-editable `SOUL.md` and `MEMORY.md`. Names reject traversal. Newly
created directories/files use 0700/0600; existing public roots are rejected rather
than silently changing permissions. Symlinks at the root/profile/file entries
are rejected. A privileged adversary changing paths concurrently is outside this
cooperative local-store boundary.

Reads bound SOUL to 8 KiB and MEMORY to 32 KiB and require UTF-8. Debug output
contains lengths, not contents. Memory updates use a SHA-256 revision comparison,
a cooperative file lock, temporary files, fsync, atomic rename and parent-directory
fsync. One previous bounded version is retained. An I/O error after rename can
mean a commit happened; callers must reread before retrying. Human editors must
use the same interface/lock to participate in concurrent-write protection.

Seven host tests cover update conflicts, previous versions, concurrent writers,
write interruption, bounds, traversal, symlinks and permission handling. The
`qualify` example passed on Omega's SD filesystem, including locking and reopen.
This does not prove power-loss durability on faulty hardware. Runtime writer
attribution, startup recovery policy and process-kill tests remain to implement.

Storage is independent of transmission. An explicit policy for sending selected context to the primary cloud model
is still required. There is no
automatic prompt injection or cloud synchronization. Cloud workers must receive
only explicit task payloads and never inherit these snapshots automatically.
