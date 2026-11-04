//! Small platform adaptations for constrained headless hosts.

/// Kernel advice changes access policy, without discarding or unmapping pages.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemoryAdviceReport {
    pub regions: usize,
    pub bytes: usize,
    pub failures: usize,
}

/// Avoid speculative reads around sparse executable accesses on SD-backed hosts.
///
/// Only the main image's read-only load segments and protected relocation data
/// receive MADV_RANDOM. Heap, stack, libraries and writable data are untouched.
/// Errors are nonfatal; the original mapping and execution remain valid.
#[cfg(target_os = "linux")]
pub fn advise_executable_random_access() -> MemoryAdviceReport {
    linux::advise()
}

#[cfg(not(target_os = "linux"))]
pub fn advise_executable_random_access() -> MemoryAdviceReport {
    MemoryAdviceReport::default()
}

#[cfg(target_os = "linux")]
mod linux {
    use super::MemoryAdviceReport;
    use std::ffi::c_void;

    struct Context {
        main_phdr: usize,
        page_size: usize,
        report: MemoryAdviceReport,
    }

    fn aligned_range(
        base: usize,
        offset: usize,
        size: usize,
        page: usize,
        round_end_up: bool,
    ) -> Option<(usize, usize)> {
        if size == 0 || !page.is_power_of_two() {
            return None;
        }
        let first = base.checked_add(offset)?;
        let last = first.checked_add(size)?;
        let last = if round_end_up {
            last.checked_add(page - 1)?
        } else {
            last
        } & !(page - 1);
        let first = first & !(page - 1);
        (last > first).then_some((first, last.checked_sub(first)?))
    }

    fn should_advise(kind: u32, flags: u32) -> bool {
        (kind == libc::PT_LOAD && flags & libc::PF_R != 0 && flags & libc::PF_W == 0)
            || kind == libc::PT_GNU_RELRO
    }

    unsafe extern "C" fn visit(
        info: *mut libc::dl_phdr_info,
        _size: libc::size_t,
        data: *mut c_void,
    ) -> libc::c_int {
        // dl_iterate_phdr owns this live metadata for the duration of the call;
        // data points to the Context borrowed by advise(). No pointer escapes.
        let (info, context) = unsafe { (&*info, &mut *data.cast::<Context>()) };
        if info.dlpi_phdr as usize != context.main_phdr {
            return 0;
        }
        let headers =
            unsafe { std::slice::from_raw_parts(info.dlpi_phdr, usize::from(info.dlpi_phnum)) };
        for header in headers {
            if !should_advise(header.p_type, header.p_flags) {
                continue;
            }
            let Some((address, size)) = aligned_range(
                info.dlpi_addr as usize,
                header.p_vaddr as usize,
                header.p_memsz as usize,
                context.page_size,
                header.p_type != libc::PT_GNU_RELRO,
            ) else {
                continue;
            };
            context.report.regions += 1;
            context.report.bytes = context.report.bytes.saturating_add(size);
            // MADV_RANDOM changes readahead policy only. It does not invalidate
            // references, change permissions, or read the mapped pages.
            if unsafe { libc::madvise(address as *mut c_void, size, libc::MADV_RANDOM) } != 0 {
                context.report.failures += 1;
            }
        }
        1 // The main image was found; do not walk shared libraries.
    }

    pub(super) fn advise() -> MemoryAdviceReport {
        // Both queries return process-local kernel/loader metadata, not file data.
        let main_phdr = unsafe { libc::getauxval(libc::AT_PHDR) } as usize;
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        let Ok(page_size) = usize::try_from(page_size) else {
            return MemoryAdviceReport::default();
        };
        if main_phdr == 0 || !page_size.is_power_of_two() {
            return MemoryAdviceReport::default();
        }
        let mut context = Context {
            main_phdr,
            page_size,
            report: MemoryAdviceReport::default(),
        };
        unsafe {
            libc::dl_iterate_phdr(Some(visit), (&mut context as *mut Context).cast());
        }
        context.report
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn ranges_cover_unaligned_segments_without_wrapping() {
            assert_eq!(
                aligned_range(0x1000, 1, 4096, 4096, true),
                Some((0x1000, 8192))
            );
            assert_eq!(
                aligned_range(0x1000, 1, 4096, 4096, false),
                Some((0x1000, 4096))
            );
            assert_eq!(aligned_range(0, 0, 0, 4096, true), None);
            assert_eq!(aligned_range(0, 1, 1, 3, true), None);
            assert_eq!(aligned_range(usize::MAX, 1, 1, 4096, true), None);
            assert_eq!(aligned_range(0, usize::MAX - 1, 1, 4096, true), None);
        }

        #[test]
        fn writable_load_segments_are_excluded() {
            assert!(should_advise(libc::PT_LOAD, libc::PF_R | libc::PF_X));
            assert!(should_advise(libc::PT_LOAD, libc::PF_R));
            assert!(!should_advise(libc::PT_LOAD, libc::PF_R | libc::PF_W));
            assert!(!should_advise(libc::PT_NOTE, libc::PF_R));
            assert!(should_advise(libc::PT_GNU_RELRO, libc::PF_R));
        }

        #[test]
        fn kernel_accepts_advice_for_the_live_main_image() {
            let report = advise();
            assert!(report.regions > 0);
            assert!(report.bytes > 0);
            assert_eq!(report.failures, 0);
            // Continue using the executable and ordinary writable state.
            let mut values = vec![1, 2, 3];
            values.push(4);
            assert_eq!(values.iter().sum::<u32>(), 10);
        }
    }
}
