# Verified design references

Retrieved 2026-11-03. Source findings and proposed changes are distinguished.

- Official Codex MCP documentation:
  https://learn.chatgpt.com/docs/extend/mcp
  Confirms stdio and Streamable HTTP, bearer/OAuth and shared config. Keep these
  clients even when optional first-party app catalogs are removed.
- App Server:
  https://learn.chatgpt.com/docs/app-server
  Preserve the upstream session/control surface; Remote pairing alone is not
  evidence for every RPC.
- Instructions:
  https://learn.chatgpt.com/docs/agent-configuration/agents-md
  Standard instructions exist already. A fallback filename is not an instruction
  to concatenate every SOUL and MEMORY file; implement explicit bounded profiles.
- Cloud environments:
  https://learn.chatgpt.com/docs/environments/cloud-environments
  Cloud execution requires a suitable published environment/account. Upstream
  cloud-tasks/src/lib.rs separates noninteractive exec/status/list/diff/apply
  dispatch from the TUI, providing a candidate extraction boundary.
- MCP SDK migration:
  https://ts.sdk.modelcontextprotocol.io/v2/migration/support-2026-07-28
  Cancellation/session semantics differ by protocol era and transport.
  Preserve negotiation and test both supported paths before replacing clients.
- Processor documentation:
  https://github.com/OnionIoT/Onion-Docs/blob/master/Omega2/Projects/intro/omega2-intro.md
  Omega uses MT7688AN, little-endian MIPS 24KEc at 580 MHz. Physical cpuinfo confirms
  24KEc, MIPS32r2, DSP and one core. BogoMIPS is not a clock-frequency measurement.
- GCC MIPS options:
  https://gcc.gnu.org/onlinedocs/gcc/MIPS-Options.html
  Architecture, scheduling, float ABI and GOT options are distinct controls.

## Processor-specific experiment policy

Use the matching OpenWrt soft-float ABI and MIPS32r2 target. For C/C++ compare
24KEc scheduling and size-oriented settings against the existing SDK defaults.
Verify LLVM target CPU support rather than passing GCC CPU names blindly to Rust.
Do not assume MIPS16/DSP extensions help generic Rust/JSON/network workloads.
Avoid hard-float binaries on this system.

Because this is one core with limited cache/RAM, measure code locality and
page-fault cost rather than maximizing threads or unrolling. Compare opt-level
s/z against 2 and selective ThinLTO, retaining the working non-LTO build.
The large CLI requires xgot and smaller codegen units for relocation ranges.
These are justified experiment candidates, not verified speed improvements.

Use bounded queues, MCP connections and result payloads; lazy initialization and
connection reuse can avoid work. Avoid excessive floating-point work, repeated
full executable hashing and repeated TLS setup in hot paths, but preserve
authentication and identity checks. Do not replace cryptography with a custom
implementation to chase size. No overclock, radio change, swap change or OS
reconfiguration is part of the initial optimization.
