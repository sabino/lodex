# Profiling the OpenWrt host

Profile the whole device before changing kernel parameters. On the tested
Omega2+, a 30-second idle observation showed about 33% busy CPU, while Lodex
itself used about 0.3%. A consumer's repeated interpreter launches and display
polling dominated. Removing those launches and using event-driven display waits
reduced a subsequent observation to about 16% busy CPU. These are observations
with background network management active, not a controlled kernel benchmark.
Consumer-specific implementations belong outside Lodex.

## Separate current costs from cumulative counters

The USB interrupt total was large after hours of audio work, but the measured
idle rate was below one interrupt per second. Do not infer a current driver
bottleneck from a boot-lifetime interrupt count. Sample CPU, process creation,
process/child CPU, interrupt deltas and storage activity over the same interval.
Exclude profiling overhead and retain raw captures privately.

## Swap read-ahead is a different policy

The executable's `MADV_RANDOM` optimization is not the same as globally setting
`vm.page-cluster=0`. On Linux 5.15 the latter reduces swap read-ahead from up to
eight pages at the default value of three to one page. It can trade initial-fault
latency for additional later reads; see the
[kernel documentation](https://github.com/torvalds/linux/blob/v5.15/Documentation/admin-guide/sysctl/vm.rst)
and [implementation](https://github.com/torvalds/linux/blob/v5.15/mm/swap_state.c).

A bounded experiment paged out 32 MiB of generated anonymous data, then checked
every page in random and sequential order, with two runs per setting/pattern.
It did not establish an improvement from zero. Some default-setting runs had
no major faults, so residual swap-cache state prevents a clean cold-I/O
comparison. The original value of three was restored. Do not recommend this
global setting merely because per-executable advice helped a different workload.

## Match kernel modules and firmware

This device reports Linux 5.15.150, but its kernel package ABI hash differs from
the official OpenWrt 23.05.3 image. A matching version number alone does not make
a downloaded kernel module compatible. The official mt76x8 23.05.3
[package index](https://downloads.openwrt.org/releases/23.05.3/targets/ramips/mt76x8/packages/)
does not offer a drop-in zram module for this installation. Zram would target
swapped anonymous data, not compress clean executable mappings.

There is a concrete newer storage-driver path: OpenWrt merged
[the switch to the upstream SDHC driver](https://github.com/openwrt/openwrt/pull/15896),
and the [24.10 Omega2+ device definition](https://github.com/openwrt/openwrt/blob/openwrt-24.10/target/linux/ramips/image/mt76x8.mk)
selects `kmod-mmc-mtk`. The current vendor driver already uses DMA. A meaningful
comparison requires a complete matching firmware, identical storage workloads,
configuration backups and a tested boot-recovery path. No firmware or driver
speedup is claimed from this source inspection.
